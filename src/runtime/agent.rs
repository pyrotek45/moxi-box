use super::*;
use crate::domain::AgentActivity;
use lm_studio::ToolCall;

const MAX_TURNS: usize = 16;
const MAX_OBSERVATION_BYTES: usize = 4000;

pub(super) fn run_agent(
    store: &Store,
    runtime: &Runtime,
    events: &EventSender,
    commands: &Receiver<AppCommand>,
    deferred: &mut VecDeque<AppCommand>,
    request: AgentRequest,
    cancelled: &AtomicBool,
) {
    let AgentRequest {
        settings,
        content,
        auto_approve,
    } = request;
    let Some((mut user_id, reply_id)) = begin_run(store, events, &content) else {
        return;
    };
    let history = match context::prepare(store, runtime, &settings, events, cancelled) {
        Ok(history) => history,
        Err(error) => return fail_run(store, events, error),
    };
    let prompt = lm_studio::agent_prompt(settings.web_access);
    let mut definitions = tools::definitions();
    if settings.web_access {
        definitions.extend(crate::web::definitions());
    }
    let mut messages = lm_studio::provider_messages(&prompt, &history);
    let mut reply = Reply::new(reply_id);
    let mut turns = 0;
    while turns < MAX_TURNS {
        turns += 1;
        if let Err(error) = context::compact_agent(
            store,
            runtime,
            &settings,
            events,
            &mut messages,
            user_id,
            cancelled,
        ) {
            return fail_run(store, events, error);
        }
        reply.begin_turn();
        let mut progress_at = std::time::Instant::now();
        let response = match runtime.block_on(lm_studio::agent_turn(
            &settings,
            &messages,
            definitions.clone(),
            cancelled,
            |delta| match delta {
                Some(text) => reply.stream(store, events, text),
                // Invisible output still proves the model is working, not stuck.
                None if progress_at.elapsed() >= std::time::Duration::from_millis(250) => {
                    progress_at = std::time::Instant::now();
                    send(events, AppEvent::RunProgress);
                }
                None => {}
            },
        )) {
            Ok(response) => response,
            Err(error) => {
                let _ = reply.save(store);
                return provider_failed(store, events, cancelled, error);
            }
        };
        if let Err(error) = reply.save(store) {
            return fail_run(store, events, error);
        }
        if response.tool_calls.is_empty() {
            messages.push(serde_json::json!({"role":"assistant","content":response.content}));
        } else {
            lm_studio::append_tool_calls(&mut messages, &response);
            for call in &response.tool_calls {
                let tool = Tool {
                    store,
                    runtime,
                    events,
                    settings: &settings,
                    message_id: reply.id,
                    offset: reply.text.len(),
                    auto_approve: &auto_approve,
                    cancelled,
                };
                match tool.run(commands, deferred, call) {
                    Ok(Some(observation)) => {
                        lm_studio::append_tool_result(&mut messages, call, &observation)
                    }
                    Ok(None) => {
                        let _ = reply.push(store, events, "Tool operation was denied by the user.");
                        return reply.finish(store, events);
                    }
                    Err(error) => return fail_run(store, events, error),
                }
            }
        }
        // Messages sent during the run steer it at this safe point, after a complete tool round.
        let steering = if response.tool_calls.is_empty() {
            take_steering(commands, deferred)
        } else {
            String::new()
        };
        if !steering.is_empty() {
            let Some((steer_id, reply_id)) = begin_run(store, events, &steering) else {
                return;
            };
            messages.push(serde_json::json!({"role":"user","content":steering}));
            user_id = steer_id;
            reply = Reply::new(reply_id);
            turns = 0;
        } else if response.tool_calls.is_empty() {
            return reply.finish(store, events);
        }
    }
    let _ = reply.push(
        store,
        events,
        "Stopped after the sixteen-turn limit. Send a new message to continue.",
    );
    reply.finish(store, events);
}

/// Collect chat messages queued during the run; other commands stay deferred in order.
fn take_steering(commands: &Receiver<AppCommand>, deferred: &mut VecDeque<AppCommand>) -> String {
    deferred.extend(commands.try_iter());
    let mut steering = Vec::new();
    deferred.retain(|command| match command {
        AppCommand::SendChat {
            content, cancelled, ..
        } => {
            if !cancelled.load(Ordering::Acquire) && !content.trim().is_empty() {
                steering.push(content.trim().to_owned());
            }
            false
        }
        _ => true,
    });
    steering.join("\n\n")
}

/// The visible assistant reply, streamed as it arrives and checkpointed to storage.
struct Reply {
    id: i64,
    text: String,
    /// No text has arrived yet in the current model turn.
    fresh_turn: bool,
    saved_at: std::time::Instant,
}

impl Reply {
    fn new(id: i64) -> Self {
        Self {
            id,
            text: String::new(),
            fresh_turn: true,
            saved_at: std::time::Instant::now(),
        }
    }

    fn begin_turn(&mut self) {
        self.fresh_turn = true;
    }

    /// Append streamed text; turns are separated by a blank line.
    fn stream(&mut self, store: &Store, events: &EventSender, text: &str) {
        let mut delta = text.to_owned();
        if self.fresh_turn {
            delta = delta.trim_start().to_owned();
            if delta.is_empty() {
                return;
            }
            if !self.text.is_empty() {
                delta.insert_str(0, "\n\n");
            }
            self.fresh_turn = false;
        }
        self.text.push_str(&delta);
        send(
            events,
            AppEvent::ChatDelta {
                id: self.id,
                text: delta,
            },
        );
        if self.saved_at.elapsed() >= std::time::Duration::from_millis(250) {
            let _ = self.save(store);
        }
    }

    fn save(&mut self, store: &Store) -> Result<(), String> {
        self.saved_at = std::time::Instant::now();
        store.replace_message_content(self.id, &self.text)
    }

    fn push(&mut self, store: &Store, events: &EventSender, content: &str) -> Result<(), String> {
        let content = content.trim();
        if content.is_empty() {
            return Ok(());
        }
        let delta = if self.text.is_empty() {
            content.to_owned()
        } else {
            format!("\n\n{content}")
        };
        self.text.push_str(&delta);
        store.replace_message_content(self.id, &self.text)?;
        send(
            events,
            AppEvent::ChatDelta {
                id: self.id,
                text: delta,
            },
        );
        Ok(())
    }

    fn finish(mut self, store: &Store, events: &EventSender) {
        if self.text.is_empty() {
            if let Err(error) = self.push(store, events, "Agent run completed.") {
                return fail_run(store, events, error);
            }
        }
        send(events, AppEvent::ChatFinished { id: self.id });
    }
}

struct Tool<'a> {
    store: &'a Store,
    runtime: &'a Runtime,
    events: &'a EventSender,
    settings: &'a Settings,
    message_id: i64,
    offset: usize,
    auto_approve: &'a AtomicBool,
    cancelled: &'a AtomicBool,
}

impl Tool<'_> {
    /// Ok(None) means the user denied the call; Err stops the run.
    fn run(
        &self,
        commands: &Receiver<AppCommand>,
        deferred: &mut VecDeque<AppCommand>,
        call: &ToolCall,
    ) -> Result<Option<String>, String> {
        self.check_cancelled()?;
        let approval =
            tools::requires_approval(&call.name) && !self.auto_approve.load(Ordering::Acquire);
        let label = tools::label(&call.name, &call.arguments, false);
        let mut step = AgentActivity {
            message_id: self.message_id,
            offset: self.offset,
            call_id: call.id.clone(),
            title: if approval {
                format!("Awaiting approval · {label}")
            } else {
                format!("{label}…")
            },
            detail: tools::call_preview(&call.name, &call.arguments),
            running: true,
        };
        self.record(&step)?;
        if approval {
            send(
                self.events,
                AppEvent::ApprovalRequested {
                    request_id: call.id.clone(),
                    summary: tools::approval_summary(&call.name, &call.arguments),
                    preview: step.detail.clone(),
                },
            );
            if !wait_for_approval(commands, deferred, &call.id, self.cancelled)? {
                step.running = false;
                step.title = format!("Denied · {label}");
                self.record(&step)?;
                return Ok(None);
            }
            step.title = format!("{label}…");
            self.record(&step)?;
        }
        let result = self.execute(call);
        step.running = false;
        let observation = match result {
            Ok(result) => {
                step.title = tools::label(&call.name, &call.arguments, true);
                step.detail = tools::result_detail(&call.name, &call.arguments, &result);
                result
            }
            Err(error) => {
                step.title = format!("Failed · {label}");
                step.detail = error.clone();
                format!("Tool error: {error}")
            }
        };
        self.record(&step)?;
        self.check_cancelled()?;
        Ok(Some(if call.name.ends_with("_tasks") {
            observation
        } else {
            bounded(observation)
        }))
    }

    fn execute(&self, call: &ToolCall) -> Result<String, String> {
        if !call.arguments.is_object() {
            return Err(format!(
                "Arguments must be a JSON object. Received: {}",
                call.arguments
            ));
        }
        match call.name.as_str() {
            "list_tasks" | "update_tasks" => {
                let tasks = if call.name == "update_tasks" {
                    self.store.replace_tasks(&call.arguments)?
                } else {
                    self.store.tasks()?
                };
                let result = serde_json::to_string(&tasks).map_err(|e| e.to_string())?;
                send(self.events, AppEvent::TasksUpdated(tasks));
                Ok(result)
            }
            "remember" => {
                let note = call.arguments["note"]
                    .as_str()
                    .map(str::trim)
                    .filter(|note| !note.is_empty() && note.len() <= 1000)
                    .ok_or("Use a nonempty memory note under 1,000 bytes.")?;
                let memory = format!("{}\n- {note}", self.store.memory()?);
                self.store.save_memory(memory.trim())?;
                Ok("Project note saved for future conversations in this workspace.".to_owned())
            }
            name if crate::web::is_web_tool(name) => {
                if !self.settings.web_access {
                    return Err(
                        "Web access is off. The user can turn it on in Settings.".to_owned()
                    );
                }
                let future = async {
                    if name == "web_search" {
                        crate::web::search(call.arguments["query"].as_str().unwrap_or_default())
                            .await
                    } else {
                        crate::web::fetch(call.arguments["url"].as_str().unwrap_or_default()).await
                    }
                };
                self.runtime
                    .block_on(lm_studio::cancellable(self.cancelled, future))
            }
            name => {
                if self.settings.workspace_root.is_empty() {
                    return Err("No folder is attached to this workspace. Ask the user to attach one in the sidebar.".to_owned());
                }
                let result = tools::execute(self.settings, name, &call.arguments, self.cancelled);
                if tools::requires_approval(name) {
                    // Writes and commands can change the tree shown in the sidebar.
                    if let Ok(entries) = tools::workspace_entries(&self.settings.workspace_root) {
                        send(self.events, AppEvent::WorkspaceLoaded(entries));
                    }
                }
                result
            }
        }
    }

    fn record(&self, step: &AgentActivity) -> Result<(), String> {
        self.store.record_tool(step)?;
        send(self.events, AppEvent::ToolActivity(step.clone()));
        Ok(())
    }

    fn check_cancelled(&self) -> Result<(), String> {
        if self.cancelled.load(Ordering::Acquire) {
            Err("Stopped by user.".to_owned())
        } else {
            Ok(())
        }
    }
}

/// Model-facing tool output stays bounded; task lists are already capped and must stay valid JSON.
fn bounded(observation: String) -> String {
    if observation.len() <= MAX_OBSERVATION_BYTES {
        return observation;
    }
    let mut end = MAX_OBSERVATION_BYTES - 500;
    while !observation.is_char_boundary(end) {
        end -= 1;
    }
    format!(
        "{}\n[Output truncated for context. Read a smaller range or use a narrower query.]",
        &observation[..end]
    )
}

fn wait_for_approval(
    commands: &Receiver<AppCommand>,
    deferred: &mut VecDeque<AppCommand>,
    request_id: &str,
    cancelled: &AtomicBool,
) -> Result<bool, String> {
    loop {
        if cancelled.load(Ordering::Acquire) {
            return Err("Stopped by user.".to_owned());
        }
        let command = match commands.recv_timeout(std::time::Duration::from_millis(50)) {
            Ok(command) => command,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(_) => return Err("Agent approval channel closed.".to_owned()),
        };
        match command {
            AppCommand::ResolveApproval {
                request_id: received,
                approved,
            } if received == request_id => return Ok(approved),
            AppCommand::ResolveApproval { .. } => {}
            command => deferred.push_back(command),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn approval_preserves_unrelated_commands_and_ignores_stale_ids() {
        let (tx, rx) = mpsc::channel();
        tx.send(AppCommand::AddTask("keep me".into())).unwrap();
        tx.send(AppCommand::ResolveApproval {
            request_id: "old".into(),
            approved: true,
        })
        .unwrap();
        tx.send(AppCommand::ResolveApproval {
            request_id: "current".into(),
            approved: false,
        })
        .unwrap();
        let mut deferred = VecDeque::new();
        assert!(
            !wait_for_approval(&rx, &mut deferred, "current", &AtomicBool::new(false)).unwrap()
        );
        assert!(
            matches!(deferred.pop_front(), Some(AppCommand::AddTask(title)) if title == "keep me")
        );
        assert!(deferred.is_empty());
    }

    #[test]
    fn steering_takes_queued_chat_and_keeps_other_commands() {
        let (tx, rx) = mpsc::channel();
        let chat = |content: &str, stopped: bool| AppCommand::SendChat {
            settings: Settings::default(),
            content: content.into(),
            agent_mode: true,
            auto_approve: Default::default(),
            cancelled: std::sync::Arc::new(AtomicBool::new(stopped)),
        };
        tx.send(chat("use tabs", false)).unwrap();
        tx.send(AppCommand::AddTask("keep".into())).unwrap();
        tx.send(chat("dropped by stop", true)).unwrap();
        tx.send(chat("and add tests", false)).unwrap();
        let mut deferred = VecDeque::new();
        assert_eq!(
            take_steering(&rx, &mut deferred),
            "use tabs\n\nand add tests"
        );
        assert!(matches!(deferred.pop_front(), Some(AppCommand::AddTask(_))));
        assert!(deferred.is_empty());
        assert_eq!(take_steering(&rx, &mut deferred), "");
    }

    #[test]
    fn long_observations_are_truncated_on_a_character_boundary() {
        let text = "é".repeat(5000);
        let bounded = bounded(text);
        assert!(bounded.len() < MAX_OBSERVATION_BYTES);
        assert!(bounded.ends_with("narrower query.]"));
        assert_eq!(super::bounded("short".into()), "short");
    }
}

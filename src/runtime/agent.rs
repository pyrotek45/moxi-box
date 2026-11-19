use super::*;
use crate::domain::AgentActivity;
use lm_studio::ToolCall;

const MAX_OBSERVATION_BYTES: usize = 4000;
/// Tools offered in Chat mode, besides web tools when Web access is on.
const CHAT_TOOLS: [&str; 2] = ["list_tasks", "update_tasks"];
/// Identical consecutive tool rounds before the model is warned, and before recovery mode.
const REPEAT_WARNING: usize = 3;
const REPEAT_RECOVERY: usize = 5;
/// Recovery attempts per run before a still-repeating agent is stopped.
const MAX_RECOVERIES: usize = 3;

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
        chat,
    } = request;
    let Some((mut user_id, reply_id)) = begin_run(store, events, &content) else {
        return;
    };
    let mut learned_limit = match context::active_context_limit(store, &settings) {
        Ok(limit) => limit,
        Err(error) => return fail_run(store, events, error),
    };
    let history =
        match context::prepare(store, runtime, &settings, events, cancelled, learned_limit) {
            Ok(history) => history,
            Err(error) => return fail_run(store, events, error),
        };
    let prompt = if chat {
        lm_studio::chat_prompt(settings.web_access)
    } else {
        lm_studio::agent_prompt(settings.web_access)
    };
    let mut definitions = tools::definitions();
    if chat {
        definitions.retain(|definition| {
            CHAT_TOOLS.contains(&definition["function"]["name"].as_str().unwrap_or_default())
        });
    }
    if settings.web_access {
        definitions.extend(crate::web::definitions());
    }
    let mut messages = lm_studio::provider_messages(&prompt, &history);
    let mut reply = Reply::new(reply_id);
    let mut context_retries = 0;
    let mut recovery = Recovery::default();
    // No turn cap: the run ends when the model stops calling tools and its task list is done
    // (or it explains why not), when it loops on identical calls, or when the user presses Stop.
    let mut gate = TaskGate::default();
    let mut repeats = Repeats::default();
    let mut recoveries = 0;
    let mut status_corrections = 0;
    if let Err(error) = reason(
        store,
        runtime,
        events,
        &settings,
        &mut messages,
        cancelled,
        reply.id,
    ) {
        let _ = reply.save(store);
        return provider_failed(store, events, cancelled, error);
    }
    loop {
        apply_task_commands(store, events, commands, deferred);
        if let Err(error) = context::compact_agent(
            store,
            runtime,
            &settings,
            events,
            &mut messages,
            cancelled,
            (user_id, learned_limit, &prompt),
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
                if context_retries < 2 && reply.fresh_turn && lm_studio::is_context_limit(&error) {
                    let limit =
                        match context::learn_context_limit(store, &settings, events, &messages) {
                            Ok(limit) => limit,
                            Err(error) => return fail_run(store, events, error),
                        };
                    learned_limit = Some(limit);
                    context_retries += 1;
                    let has_tool_round = messages.iter().any(|message| message["role"] == "tool");
                    if has_tool_round {
                        if let Err(error) = context::compact_agent(
                            store,
                            runtime,
                            &settings,
                            events,
                            &mut messages,
                            cancelled,
                            (user_id, learned_limit, &prompt),
                        ) {
                            return fail_run(store, events, error);
                        }
                    } else {
                        let history = match context::prepare(
                            store,
                            runtime,
                            &settings,
                            events,
                            cancelled,
                            learned_limit,
                        ) {
                            Ok(history) => history,
                            Err(error) => return fail_run(store, events, error),
                        };
                        messages = lm_studio::provider_messages(&prompt, &history);
                    }
                    continue;
                }
                return provider_failed(store, events, cancelled, error);
            }
        };
        if let Err(error) = reply.save(store) {
            return fail_run(store, events, error);
        }
        if response.tool_calls.is_empty() {
            messages.push(serde_json::json!({"role":"assistant","content":response.content}));
        } else {
            let repeated = repeats.observe(&response.tool_calls);
            if repeated >= REPEAT_RECOVERY {
                if recoveries >= MAX_RECOVERIES {
                    let _ = reply.push(
                        store,
                        events,
                        &format!("Stopped: the model kept repeating the same tool call even after {MAX_RECOVERIES} recovery attempts. Send a message to redirect it."),
                    );
                    return reply.finish(store, events);
                }
                recoveries += 1;
                // Recovery mode: skip the repeated calls and tell the model exactly what is wrong.
                let _ = reply.push(
                    store,
                    events,
                    &format!("*Recovery mode {recoveries}/{MAX_RECOVERIES}: the same tool call was repeated {repeated} times, so it was skipped and the agent was asked to change approach.*"),
                );
                send(
                    events,
                    AppEvent::Notice(
                        "The agent was looping; recovery mode asked it to change approach.".into(),
                    ),
                );
                lm_studio::append_tool_calls(&mut messages, &response);
                let names = response
                    .tool_calls
                    .iter()
                    .map(|call| call.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                for call in &response.tool_calls {
                    lm_studio::append_tool_result(
                        &mut messages,
                        call,
                        &format!("NOT RUN. Harness recovery mode: you have made this exact call ({names}) with identical arguments {repeated} times in a row, and repeating it cannot give a different result. Stop and diagnose: 1) restate what you are trying to achieve and what the earlier results told you; 2) check the task list and the last error; 3) choose a different action, such as reading the relevant file, running a test to see the real error, changing the arguments, using another tool, or searching the web if it is on; 4) if the work is already done, mark tasks completed and summarize; if you are truly blocked, explain the blocker to the user. Recovery attempt {recoveries} of {MAX_RECOVERIES}."),
                    );
                }
                continue;
            }
            lm_studio::append_tool_calls(&mut messages, &response);
            for (index, call) in response.tool_calls.iter().enumerate() {
                if call.name == "update_tasks" && !chat {
                    gate.active = true;
                }
                let blocked_memory = call.name == "remember" && recovery.has_failed_mutation();
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
                // Checked before approval so Chat mode never prompts for a tool it cannot run.
                let chat_blocked = chat
                    && !CHAT_TOOLS.contains(&call.name.as_str())
                    && !crate::web::is_web_tool(&call.name);
                let outcome = if chat_blocked {
                    tool.reject(call, &format!("{} is not available in Chat mode. Ask the user to switch to Agent mode for file access and commands.", call.name))
                } else if blocked_memory {
                    tool.reject(
                        call,
                        "Project memory was not changed because an earlier file or command operation failed. Resolve and verify that operation before recording its result.",
                    )
                } else {
                    tool.run(commands, deferred, call)
                };
                match outcome {
                    Ok(Some((mut observation, success))) => {
                        // Only failed changes need a recovery check; a failed read or web fetch is
                        // already visible to the model, and re-prompting about it confuses it.
                        if !blocked_memory && tools::requires_approval(&call.name) {
                            recovery.observe(&call.name, success);
                        }
                        if repeated >= REPEAT_WARNING && index + 1 == response.tool_calls.len() {
                            observation.push_str(&format!("\n\n[Harness note: this exact tool call has now been made {repeated} times in a row with the same arguments. Repeating it will not change the result. Use what you learned, try a different approach, or finish and explain.]"));
                        }
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
        if response.tool_calls.is_empty()
            && status_only(&response.content)
            && status_corrections < 2
        {
            // The model wrote a status line ("[Updated task list]") instead of acting or
            // answering. Tell it what went wrong rather than ending the run on a non-answer.
            status_corrections += 1;
            messages.push(serde_json::json!({"role":"user","content":"Harness check: your last reply was only a bracketed status line, which does nothing. Nothing was run and no tool was called. To act, call the tool itself (for example update_tasks, read_file, or run_command). If the work is finished, reply with a normal short summary of what you did and what remains."}));
            let _ = reply.push(
                store,
                events,
                "*Recovery: the reply was only a status line, so the agent was asked to act or answer properly.*",
            );
            continue;
        }
        if response.tool_calls.is_empty() {
            if let Some(instruction) = recovery.follow_up() {
                messages.push(serde_json::json!({"role":"user","content":instruction}));
                send(
                    events,
                    AppEvent::Notice(
                        "Giving the model another chance to address failed tools…".into(),
                    ),
                );
                continue;
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
            recovery = Recovery::default();
            repeats = Repeats::default();
        } else if response.tool_calls.is_empty() {
            // The task list is the stop gate: unfinished tasks send the model back to work.
            let tasks = match store.tasks() {
                Ok(tasks) => tasks,
                Err(error) => return fail_run(store, events, error),
            };
            if let Some(instruction) = gate.check(&tasks) {
                messages.push(serde_json::json!({"role":"user","content":instruction}));
                send(
                    events,
                    AppEvent::Notice("Tasks remain open; the agent is continuing…".into()),
                );
                continue;
            }
            return reply.finish(store, events);
        }
    }
}

/// Optional thinking passes before the tool loop (Settings: Reasoning). The thoughts appear as
/// one collapsible step instead of reply text, a trivial request skips the remaining passes,
/// and a pass that returns nothing is ignored rather than failing the run.
fn reason(
    store: &Store,
    runtime: &Runtime,
    events: &EventSender,
    settings: &Settings,
    messages: &mut Vec<serde_json::Value>,
    cancelled: &AtomicBool,
    message_id: i64,
) -> Result<(), String> {
    let passes = settings.reasoning.min(3) as usize;
    if passes == 0 {
        return Ok(());
    }
    let original = messages.clone();
    let mut step = AgentActivity {
        message_id,
        offset: 0,
        call_id: "reasoning".to_owned(),
        title: "Thinking it through…".to_owned(),
        detail: String::new(),
        running: true,
    };
    let record = |step: &AgentActivity| -> Result<(), String> {
        store.record_tool(step)?;
        send(events, AppEvent::ToolActivity(step.clone()));
        Ok(())
    };
    record(&step)?;
    let mut thoughts = Vec::new();
    for pass in 1..=passes {
        let instruction = if pass == 1 {
            format!("[Think before acting (step 1 of {passes}). Do not call tools and do not answer yet. In a few short bullet points: what exactly is being asked, what you need to inspect first, whether it needs a multi-step plan (list the steps) or is a single quick action, the main risk, and how you will verify the result. If the message is a greeting or a trivial question, reply with only the word SIMPLE.]")
        } else {
            format!("[Think before acting (step {pass} of {passes}). Still no tools. Briefly check the plan above for gaps, wrong assumptions, or a simpler approach, and give the corrected plan.]")
        };
        if pass == 1 {
            // Fold the instruction into the latest user turn so roles keep alternating.
            if let Some(content) = messages
                .last_mut()
                .and_then(|message| message.get_mut("content"))
                .filter(|content| content.is_string())
            {
                let text = format!("{}\n\n{instruction}", content.as_str().unwrap_or(""));
                *content = serde_json::Value::String(text);
            }
        } else {
            messages.push(serde_json::json!({"role":"user","content":instruction}));
        }
        let mut progress_at = std::time::Instant::now();
        let result = runtime.block_on(lm_studio::agent_turn(
            settings,
            messages,
            Vec::new(),
            cancelled,
            |_| {
                if progress_at.elapsed() >= std::time::Duration::from_millis(250) {
                    progress_at = std::time::Instant::now();
                    send(events, AppEvent::RunProgress);
                }
            },
        ));
        let thought = match result {
            Ok(response) => response.content.trim().to_owned(),
            // A thinking model may put everything in hidden reasoning; that is not a failure.
            Err(error) if error.contains("empty assistant turn") => String::new(),
            Err(error) => {
                step.running = false;
                step.title = "Thinking interrupted".to_owned();
                let _ = record(&step);
                return Err(error);
            }
        };
        let simple = thought
            .trim_matches(|c: char| !c.is_alphanumeric())
            .eq_ignore_ascii_case("simple");
        if thought.is_empty() || simple {
            break;
        }
        messages.push(serde_json::json!({"role":"assistant","content":thought}));
        thoughts.push(thought);
    }
    // The tool loop starts from the original request plus the agreed plan as one clean turn.
    *messages = original;
    step.running = false;
    if thoughts.is_empty() {
        step.title = "Thinking skipped: simple request".to_owned();
    } else {
        step.title = format!(
            "Thought it through ({} step{})",
            thoughts.len(),
            if thoughts.len() == 1 { "" } else { "s" }
        );
        step.detail = thoughts.join("\n\n");
        if let Some(content) = messages
            .last_mut()
            .and_then(|message| message.get_mut("content"))
            .filter(|content| content.is_string())
        {
            let text = format!(
                "{}\n\n[Your plan from thinking it through. Follow it; if it has several steps, record them with update_tasks first.]\n{}",
                content.as_str().unwrap_or(""),
                thoughts.last().unwrap()
            );
            *content = serde_json::Value::String(text);
        }
    }
    record(&step)
}

/// True for a reply made only of bracketed status lines such as "[Tools used: ...]" or
/// "[Updated task list]": an imitation of harness notes, not an answer.
fn status_only(content: &str) -> bool {
    let mut lines = content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .peekable();
    lines.peek().is_some() && lines.all(|line| line.starts_with('[') && line.ends_with(']'))
}

/// Apply sidebar task edits made during a run before the next model turn; other commands stay
/// deferred in order.
fn apply_task_commands(
    store: &Store,
    events: &EventSender,
    commands: &Receiver<AppCommand>,
    deferred: &mut VecDeque<AppCommand>,
) {
    deferred.extend(commands.try_iter());
    let (task_edits, rest): (VecDeque<_>, VecDeque<_>) = std::mem::take(deferred)
        .into_iter()
        .partition(is_task_command);
    *deferred = rest;
    for command in task_edits {
        task_command(store, events, command);
    }
}

/// Once the agent plans with update_tasks, it may not end its run while tasks are open. Each
/// unchanged list gets a bounded number of reminders so a blocked agent can still stop and explain.
#[derive(Default)]
struct TaskGate {
    active: bool,
    open: Vec<String>,
    reminders: usize,
}

impl TaskGate {
    const MAX_REMINDERS: usize = 2;

    fn check(&mut self, tasks: &[crate::domain::Task]) -> Option<String> {
        let open = tasks
            .iter()
            .filter(|task| task.status != crate::domain::TaskStatus::Completed)
            .map(|task| format!("- {} ({})", task.title, task.status.as_str()))
            .collect::<Vec<_>>();
        if !self.active || open.is_empty() {
            return None;
        }
        if open != self.open {
            self.open = open.clone();
            self.reminders = 0;
        }
        if self.reminders >= Self::MAX_REMINDERS {
            return None;
        }
        self.reminders += 1;
        Some(format!("Harness task check: your task list still has unfinished items:\n{}\nIf work remains, continue now with the next item using tools. If an item is already done, verify it and mark it completed with update_tasks. If an item is blocked or no longer needed, update the list accordingly and explain the blocker to the user. Do not end your reply while items you can still do remain.", open.join("\n")))
    }
}

/// Counts consecutive identical tool rounds (same tools, same arguments).
#[derive(Default)]
struct Repeats {
    last: String,
    count: usize,
}

impl Repeats {
    fn observe(&mut self, calls: &[ToolCall]) -> usize {
        let signature = calls
            .iter()
            .map(|call| format!("{}:{}", call.name, call.arguments))
            .collect::<Vec<_>>()
            .join("\n");
        if signature == self.last {
            self.count += 1;
        } else {
            self.last = signature;
            self.count = 1;
        }
        self.count
    }
}

/// Bounded correction opportunities; never replay a tool or bypass approval.
#[derive(Default)]
struct Recovery {
    failed: std::collections::BTreeSet<String>,
    follow_ups: usize,
}
impl Recovery {
    fn observe(&mut self, tool: &str, success: bool) {
        if success {
            self.failed.remove(tool);
        } else {
            self.failed.insert(tool.to_owned());
        }
    }
    fn follow_up(&mut self) -> Option<String> {
        if self.failed.is_empty() || self.follow_ups >= 2 {
            return None;
        }
        self.follow_ups += 1;
        Some(format!("Harness recovery check: these tool types have failed without a later successful call: {}. Inspect their error results and the current state. If the user's request remains unfinished, correct the arguments, read the current file, or choose another appropriate tool and continue. Commands may have partially changed state: inspect before retrying. Do not repeat unchanged failing calls, bypass approval, or claim success without evidence. If recovery is impossible or an alternative already completed the work, explain that clearly and stop.", self.failed.iter().cloned().collect::<Vec<_>>().join(", ")))
    }

    fn has_failed_mutation(&self) -> bool {
        self.failed.iter().any(|tool| {
            matches!(
                tool.as_str(),
                "write_file" | "edit_file" | "replace_all" | "delete_path" | "run_command"
            )
        })
    }
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
    ) -> Result<Option<(String, bool)>, String> {
        self.check_cancelled()?;
        // Commands that may reach outside the folder always need a person, even with Auto-approve.
        let escapes = if call.name == "run_command" {
            tools::command_escapes(&self.settings.workspace_root, &call.arguments)
        } else {
            Vec::new()
        };
        let approval = tools::requires_approval(&call.name)
            && (!self.auto_approve.load(Ordering::Acquire) || !escapes.is_empty());
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
                    forced: !escapes.is_empty(),
                    summary: if escapes.is_empty() {
                        tools::approval_summary(&call.name, &call.arguments)
                    } else {
                        format!(
                            "Warning: this command may reach outside the workspace. It {}. Approve only if you intended that.",
                            escapes.join("; it ")
                        )
                    },
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
        let success = result.is_ok();
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
        Ok(Some((
            if call.name.ends_with("_tasks") {
                observation
            } else if call.name == "run_command" {
                bounded_command(observation)
            } else {
                bounded(observation)
            },
            success,
        )))
    }

    fn reject(&self, call: &ToolCall, reason: &str) -> Result<Option<(String, bool)>, String> {
        let step = AgentActivity {
            message_id: self.message_id,
            offset: self.offset,
            call_id: call.id.clone(),
            title: format!(
                "Failed · {}",
                tools::label(&call.name, &call.arguments, false)
            ),
            detail: reason.to_owned(),
            running: false,
        };
        self.record(&step)?;
        Ok(Some((format!("Tool error: {reason}"), false)))
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
                let shared = tools::remember_shared(&call.arguments)?;
                let result = if shared {
                    let playbook = format!("{}\n- {note}", self.store.playbook()?);
                    self.store.save_playbook(playbook.trim())?;
                    "Lesson saved to the shared playbook for every workspace."
                } else {
                    let memory = format!("{}\n- {note}", self.store.memory()?);
                    self.store.save_memory(memory.trim())?;
                    "Project note saved for future conversations in this workspace."
                };
                send(
                    self.events,
                    AppEvent::MemoryUpdated {
                        memory: self.store.memory()?,
                        playbook: self.store.playbook()?,
                    },
                );
                Ok(result.to_owned())
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
                    send(self.events, AppEvent::WorkspaceChanged);
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

/// Command output keeps its start (exit status) and its end, where errors and tracebacks appear.
fn bounded_command(observation: String) -> String {
    if observation.len() <= MAX_OBSERVATION_BYTES {
        return observation;
    }
    let mut head = 800;
    while !observation.is_char_boundary(head) {
        head -= 1;
    }
    let mut tail = observation.len() - (MAX_OBSERVATION_BYTES - 1200);
    while !observation.is_char_boundary(tail) {
        tail += 1;
    }
    format!(
        "{}\n[… {} bytes of output omitted …]\n{}",
        &observation[..head],
        tail - head,
        &observation[tail..]
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
    fn command_output_keeps_status_and_final_error() {
        let output = format!("exit status: 1\n{}Traceback: ValueError", "é".repeat(5000));
        let bounded = bounded_command(output);
        assert!(bounded.len() <= MAX_OBSERVATION_BYTES);
        assert!(bounded.starts_with("exit status: 1"));
        assert!(bounded.ends_with("Traceback: ValueError"));
        assert!(bounded.contains("omitted"));
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

#[cfg(test)]
mod run_tests {
    use super::*;
    use serde_json::Value;
    use std::io::{Read, Write};

    /// Serve one scripted JSON reply per request and return each request body.
    fn mock_provider(replies: Vec<Value>) -> (String, std::thread::JoinHandle<Vec<String>>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/v1", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let mut bodies = Vec::new();
            for reply in replies {
                let (mut socket, _) = listener.accept().unwrap();
                let mut request = Vec::new();
                let mut buffer = [0u8; 8192];
                let body_start = loop {
                    let size = socket.read(&mut buffer).unwrap();
                    request.extend_from_slice(&buffer[..size]);
                    if let Some(end) = request.windows(4).position(|part| part == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&request[..end]).to_lowercase();
                        let length: usize = headers
                            .lines()
                            .find_map(|line| line.strip_prefix("content-length:"))
                            .map(|value| value.trim().parse().unwrap())
                            .unwrap();
                        while request.len() < end + 4 + length {
                            let size = socket.read(&mut buffer).unwrap();
                            request.extend_from_slice(&buffer[..size]);
                        }
                        break end + 4;
                    }
                };
                bodies.push(String::from_utf8_lossy(&request[body_start..]).into_owned());
                let body = serde_json::json!({ "choices": [{ "message": reply }] }).to_string();
                write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
            }
            bodies
        });
        (url, server)
    }

    fn tasks_call(id: usize, tasks: Value) -> Value {
        serde_json::json!({"content": null, "tool_calls": [{
            "id": format!("call{id:05}"), "type": "function",
            "function": {"name": "update_tasks", "arguments": serde_json::json!({"tasks": tasks}).to_string()}
        }]})
    }

    fn run(store: &Store, url: String, chat: bool) -> Vec<AppEvent> {
        run_with(store, url, chat, 0)
    }

    fn run_with(store: &Store, url: String, chat: bool, reasoning: u8) -> Vec<AppEvent> {
        let (sender, receiver) = std::sync::mpsc::channel();
        let events = EventSender {
            channel: sender,
            wake: Box::new(|| {}),
        };
        let (_commands_tx, commands) = std::sync::mpsc::channel();
        let settings = Settings {
            lm_studio_url: url,
            selected_model: Some("mock".into()),
            reasoning,
            ..Settings::default()
        };
        run_agent(
            store,
            &Runtime::new().unwrap(),
            &events,
            &commands,
            &mut VecDeque::new(),
            AgentRequest {
                settings,
                content: "build it".into(),
                auto_approve: Default::default(),
                chat,
            },
            &AtomicBool::new(false),
        );
        receiver.try_iter().collect()
    }

    #[test]
    fn open_tasks_keep_the_agent_working_past_sixteen_turns() {
        let pending = |n: usize| {
            serde_json::json!((0..n)
                .map(|i| serde_json::json!({"title": format!("step {i}"), "status": "pending"}))
                .collect::<Vec<_>>())
        };
        // Twenty distinct planning turns exceed the old sixteen-turn cap.
        let mut replies = (1..=20)
            .map(|n| tasks_call(n, pending(n)))
            .collect::<Vec<_>>();
        replies.push(serde_json::json!({"content": "I think I'm done."}));
        replies.push(tasks_call(
            99,
            serde_json::json!([{"title":"step 0","status":"completed"}]),
        ));
        replies.push(serde_json::json!({"content": "All finished."}));
        let (url, server) = mock_provider(replies);
        let store = Store::in_memory();
        let events = run(&store, url, false);
        let bodies = server.join().unwrap();
        assert_eq!(bodies.len(), 23);
        assert!(bodies[21].contains("Harness task check"));
        assert!(bodies[21].contains("step 19 (pending)"));
        assert!(store
            .tasks()
            .unwrap()
            .iter()
            .all(|task| task.status == crate::domain::TaskStatus::Completed));
        assert!(events
            .iter()
            .any(|event| matches!(event, AppEvent::ChatFinished { .. })));
        let reply = store
            .conversation_state()
            .unwrap()
            .messages
            .last()
            .unwrap()
            .content
            .clone();
        assert!(reply.ends_with("All finished."), "{reply}");
        assert!(!reply.contains("limit"));
    }

    #[test]
    fn chat_mode_manages_tasks_but_cannot_touch_files() {
        let read = serde_json::json!({"content": null, "tool_calls": [{
            "id": "call00002", "type": "function",
            "function": {"name": "run_command", "arguments": "{\"command\":\"ls\"}"}
        }]});
        let (url, server) = mock_provider(vec![
            tasks_call(
                1,
                serde_json::json!([{"title":"Buy milk","status":"pending"}]),
            ),
            read,
            serde_json::json!({"content": "Added it."}),
        ]);
        let store = Store::in_memory();
        run(&store, url, true);
        let bodies = server.join().unwrap();
        assert!(bodies[0].contains("\"update_tasks\"") && !bodies[0].contains("\"run_command\""));
        assert!(bodies[2].contains("not available in Chat mode"));
        // Chat mode has no stop gate: an open task does not hold the reply open.
        assert_eq!(bodies.len(), 3);
        assert_eq!(store.tasks().unwrap()[0].title, "Buy milk");
    }

    #[test]
    fn reasoning_passes_run_without_tools_and_hand_over_one_clean_turn() {
        let (url, server) = mock_provider(vec![
            serde_json::json!({"content": "It is a one-line change."}),
            serde_json::json!({"content": "Plan: edit one line, then run the test."}),
            serde_json::json!({"content": "Done."}),
        ]);
        let store = Store::in_memory();
        let events = run_with(&store, url, false, 2);
        let bodies = server.join().unwrap();
        assert_eq!(bodies.len(), 3);
        let first: Value = serde_json::from_str(&bodies[0]).unwrap();
        assert!(first.get("tools").is_none());
        assert!(bodies[0].contains("step 1 of 2"));
        assert!(bodies[1].contains("step 2 of 2"));
        let last: Value = serde_json::from_str(&bodies[2]).unwrap();
        assert!(last.get("tools").is_some());
        let roles = last["messages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|message| message["role"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        assert_eq!(roles, ["system", "user"]);
        let request = last["messages"][1]["content"].as_str().unwrap();
        assert!(request.contains("Plan: edit one line") && !request.contains("step 1 of"));
        // Thoughts live in a collapsible step, not in the reply text.
        let reply = store
            .conversation_state()
            .unwrap()
            .messages
            .last()
            .unwrap()
            .content
            .clone();
        assert_eq!(reply, "Done.");
        assert!(events.iter().any(|event| matches!(event,
            AppEvent::ToolActivity(step) if !step.running && step.title.starts_with("Thought it through (2 steps)")
                && step.detail.contains("one-line change"))));
    }

    #[test]
    fn simple_requests_and_empty_thoughts_skip_reasoning() {
        for first in [
            serde_json::json!({"content": "SIMPLE"}),
            serde_json::json!({"content": ""}),
        ] {
            let (url, server) =
                mock_provider(vec![first, serde_json::json!({"content": "Hi there!"})]);
            let store = Store::in_memory();
            run_with(&store, url, false, 3);
            let bodies = server.join().unwrap();
            assert_eq!(bodies.len(), 2);
            let last: Value = serde_json::from_str(&bodies[1]).unwrap();
            assert_eq!(last["messages"].as_array().unwrap().len(), 2);
            assert!(!bodies[1].contains("Think before acting"));
            let reply = store
                .conversation_state()
                .unwrap()
                .messages
                .last()
                .unwrap()
                .content
                .clone();
            assert_eq!(reply, "Hi there!");
        }
    }

    #[test]
    fn status_line_replies_are_corrected_not_accepted() {
        assert!(status_only("[Updated task list]"));
        assert!(status_only("  [Tools used: Listed files; Read a.py]\n\n"));
        assert!(!status_only("Done. [see notes]"));
        assert!(!status_only(""));
        let (url, server) = mock_provider(vec![
            serde_json::json!({"content": "[Updated task list]"}),
            serde_json::json!({"content": "Updated the plan and finished."}),
        ]);
        let store = Store::in_memory();
        run(&store, url, false);
        let bodies = server.join().unwrap();
        assert_eq!(bodies.len(), 2);
        assert!(bodies[1].contains("only a bracketed status line"));
        let reply = store
            .conversation_state()
            .unwrap()
            .messages
            .last()
            .unwrap()
            .content
            .clone();
        assert!(reply.ends_with("Updated the plan and finished."), "{reply}");
    }

    #[test]
    fn a_failed_web_fetch_does_not_trigger_recovery_prompts() {
        let fetch = serde_json::json!({"content": null, "tool_calls": [{
            "id": "call00003", "type": "function",
            "function": {"name": "fetch_url", "arguments": "{\"url\":\"https://example.com\"}"}
        }]});
        let (url, server) = mock_provider(vec![
            fetch,
            serde_json::json!({"content": "I could not load live weather."}),
        ]);
        let store = Store::in_memory();
        run(&store, url, true);
        let bodies = server.join().unwrap();
        // The answer ends the run: no "Harness recovery check" turns about the failed fetch.
        assert_eq!(bodies.len(), 2);
        let reply = store
            .conversation_state()
            .unwrap()
            .messages
            .last()
            .unwrap()
            .content
            .clone();
        assert_eq!(reply, "I could not load live weather.");
    }

    #[test]
    fn identical_tool_loops_get_recovery_then_stop() {
        let same = || tasks_call(7, serde_json::json!([{"title":"a","status":"pending"}]));
        // Loops into recovery, recovers by doing something different, then loops for good.
        let mut replies = (0..REPEAT_RECOVERY).map(|_| same()).collect::<Vec<_>>();
        replies.push(tasks_call(
            8,
            serde_json::json!([{"title":"b","status":"pending"}]),
        ));
        replies.extend((0..REPEAT_RECOVERY + MAX_RECOVERIES - 1).map(|_| same()));
        let total = replies.len();
        let (url, server) = mock_provider(replies);
        let store = Store::in_memory();
        run(&store, url, false);
        let bodies = server.join().unwrap();
        assert_eq!(bodies.len(), total);
        assert!(bodies[REPEAT_WARNING].contains("Harness note"));
        assert!(bodies[REPEAT_RECOVERY].contains("Harness recovery mode"));
        assert!(bodies[REPEAT_RECOVERY].contains("Recovery attempt 1 of"));
        let reply = store
            .conversation_state()
            .unwrap()
            .messages
            .last()
            .unwrap()
            .content
            .clone();
        assert!(
            reply.contains("Recovery mode 1/") && reply.contains("even after"),
            "{reply}"
        );
    }
}

#[cfg(test)]
mod recovery_tests {
    use super::*;
    use crate::domain::{Task, TaskStatus};

    fn task(title: &str, status: TaskStatus) -> Task {
        Task {
            id: 0,
            title: title.into(),
            status,
        }
    }

    #[test]
    fn task_gate_holds_open_work_and_lets_a_blocked_agent_stop() {
        let mut gate = TaskGate::default();
        let open = [
            task("a", TaskStatus::Completed),
            task("b", TaskStatus::Pending),
        ];
        // Inactive until the agent plans with update_tasks; stale lists never hold a chat open.
        assert!(gate.check(&open).is_none());
        gate.active = true;
        assert!(gate.check(&open).unwrap().contains("- b (pending)"));
        assert!(gate.check(&open).is_some());
        assert!(gate.check(&open).is_none());
        // Progress on the list earns fresh reminders.
        let progressed = [task("b", TaskStatus::InProgress)];
        assert!(gate.check(&progressed).is_some());
        assert!(gate.check(&[task("b", TaskStatus::Completed)]).is_none());
        assert!(gate.check(&[]).is_none());
    }

    #[test]
    fn repeats_count_only_identical_consecutive_rounds() {
        let call = |path: &str| ToolCall {
            id: "abcdefghi".into(),
            name: "read_file".into(),
            arguments: serde_json::json!({ "path": path }),
        };
        let mut repeats = Repeats::default();
        assert_eq!(repeats.observe(&[call("a")]), 1);
        assert_eq!(repeats.observe(&[call("a")]), 2);
        assert_eq!(repeats.observe(&[call("b")]), 1);
        assert_eq!(repeats.observe(&[call("b")]), 2);
    }
    #[test]
    fn recovery_is_bounded_and_success_clears_only_matching_tool() {
        let mut recovery = Recovery::default();
        assert!(recovery.follow_up().is_none());
        recovery.observe("edit_file", false);
        assert!(recovery.has_failed_mutation());
        recovery.observe("read_file", true);
        assert!(recovery.follow_up().unwrap().contains("edit_file"));
        assert!(recovery.follow_up().is_some());
        recovery.observe("edit_file", false);
        assert!(recovery.follow_up().is_none());
        let mut recovery = Recovery::default();
        recovery.observe("edit_file", false);
        recovery.observe("edit_file", true);
        assert!(!recovery.has_failed_mutation());
        assert!(recovery.follow_up().is_none());
        recovery.observe("read_file", false);
        assert!(!recovery.has_failed_mutation());
    }
}

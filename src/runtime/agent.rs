use super::*;
use crate::domain::AgentActivity;

pub(super) fn run_agent(
    store: &Store,
    runtime: &Runtime,
    events: &EventSender,
    commands: &Receiver<AppCommand>,
    deferred: &mut VecDeque<AppCommand>,
    request: AgentRequest,
    cancelled: &AtomicBool,
) {
    let AgentRequest { settings, content } = request;
    if content.trim().is_empty() {
        return;
    }
    let user_id = match store.add_message(MessageRole::User, content.trim()) {
        Ok(id) => id,
        Err(error) => {
            send(events, AppEvent::RunFailed(error));
            return;
        }
    };
    send(
        events,
        AppEvent::ChatUser(crate::domain::ChatMessage {
            id: user_id,
            role: MessageRole::User,
            content: content.trim().to_owned(),
        }),
    );
    let assistant_id = match store.add_message(MessageRole::Assistant, "") {
        Ok(id) => id,
        Err(error) => {
            fail_run(store, events, error);
            return;
        }
    };
    send(events, AppEvent::ChatStarted { id: assistant_id });
    let mut final_text = String::new();
    let mut emitted = 0;
    let history = match context::prepare(store, runtime, &settings, events, cancelled) {
        Ok(messages) => messages,
        Err(error) => {
            fail_run(store, events, error);
            return;
        }
    };
    let mut messages = lm_studio::agent_history(&history);
    'run: for turn in 0..16 {
        if let Err(error) = context::compact_agent(
            store,
            runtime,
            &settings,
            events,
            &mut messages,
            user_id,
            cancelled,
        ) {
            fail_run(store, events, error);
            return;
        }
        let response = match runtime.block_on(lm_studio::agent_turn(
            &settings,
            &messages,
            tools::definitions(),
            cancelled,
        )) {
            Ok(response) => response,
            Err(error) => {
                if !cancelled.load(Ordering::Acquire) {
                    send(events, AppEvent::ConnectionChanged(false));
                }
                fail_run(store, events, error);
                return;
            }
        };
        send(events, AppEvent::ConnectionChanged(true));
        if !response.content.is_empty() {
            if !final_text.is_empty() {
                final_text.push_str("\n\n");
            }
            final_text.push_str(&response.content);
        }
        if let Err(error) = store.replace_message_content(assistant_id, &final_text) {
            fail_run(store, events, error);
            return;
        }
        if !response.content.is_empty() {
            send(
                events,
                AppEvent::ChatDelta {
                    id: assistant_id,
                    text: format!(
                        "{}{}",
                        if final_text == response.content {
                            ""
                        } else {
                            "\n\n"
                        },
                        response.content
                    ),
                },
            );
        }
        emitted = final_text.len();
        if response.tool_calls.is_empty() {
            break;
        }
        lm_studio::append_tool_calls(&mut messages, &response);
        for call in response.tool_calls {
            if cancelled.load(Ordering::Acquire) {
                fail_run(store, events, "Stopped by user.".to_owned());
                return;
            }
            let mut step = AgentActivity {
                message_id: assistant_id,
                call_id: call.id.clone(),
                title: format!(
                    "{} {}",
                    if tools::requires_approval(&call.name) {
                        "Awaiting approval ·"
                    } else {
                        "Running"
                    },
                    call.name
                ),
                detail: call.arguments.to_string(),
                running: true,
            };
            if let Err(error) = store.record_tool(&step) {
                fail_run(store, events, error);
                return;
            }
            send(events, AppEvent::ToolActivity(step.clone()));
            if tools::requires_approval(&call.name) {
                send(
                    events,
                    AppEvent::ApprovalRequested {
                        request_id: call.id.clone(),
                        summary: tools::approval_summary(&call.name, &call.arguments),
                    },
                );
                match wait_for_approval(commands, deferred, &call.id, cancelled) {
                    Ok(true) => {}
                    Ok(false) => {
                        lm_studio::append_tool_result(
                            &mut messages,
                            &call,
                            "Denied by the user. Do not retry this operation.",
                        );
                        final_text.push_str("\n\nTool operation was denied by the user.");
                        step.running = false;
                        step.title = format!("Denied · {}", call.name);
                        if let Err(error) = store.record_tool(&step) {
                            fail_run(store, events, error);
                            return;
                        }
                        send(events, AppEvent::ToolActivity(step));
                        break 'run;
                    }
                    Err(error) => {
                        fail_run(store, events, error);
                        return;
                    }
                }
            }
            if cancelled.load(Ordering::Acquire) {
                fail_run(store, events, "Stopped by user.".to_owned());
                return;
            }
            if tools::requires_approval(&call.name) {
                step.title = format!("Running {}", call.name);
                send(events, AppEvent::ToolActivity(step.clone()));
            }
            let result = if matches!(call.name.as_str(), "list_tasks" | "update_tasks") {
                let tasks = if call.name == "update_tasks" {
                    store.replace_tasks(&call.arguments)
                } else {
                    store.tasks()
                };
                tasks.and_then(|tasks| {
                    let result = serde_json::to_string(&tasks).map_err(|e| e.to_string())?;
                    send(events, AppEvent::TasksUpdated(tasks));
                    Ok(result)
                })
            } else if call.name == "remember" {
                call.arguments["note"]
                    .as_str()
                    .ok_or_else(|| "Missing memory note.".to_owned())
                    .and_then(|note| {
                        if note.len() > 1000 || note.trim().is_empty() {
                            return Err("Use a nonempty memory note under 1,000 bytes.".to_owned());
                        }
                        let memory = format!("{}\n- {}", store.memory()?, note.trim());
                        store.save_memory(&settings.workspace_root, memory.trim())?;
                        Ok(
                            "Project note saved for future conversations in this workspace."
                                .to_owned(),
                        )
                    })
            } else {
                tools::execute(&settings, &call.name, &call.arguments, cancelled)
            };
            let (observation, success) = match result {
                Ok(result) => (result, true),
                Err(error) => (format!("Tool error: {error}"), false),
            };
            let text = format!("Arguments:\n{}\n\nResult:\n{}", call.arguments, observation);
            let visible = if observation.len() > 4000
                && !matches!(call.name.as_str(), "list_tasks" | "update_tasks")
            {
                format!("{}\n[Output truncated for context. Read a smaller range or use a narrower query.]",observation.chars().take(3500).collect::<String>())
            } else {
                observation.clone()
            };
            lm_studio::append_tool_result(&mut messages, &call, &visible);
            step.title = format!(
                "{} {}",
                if success { "Finished" } else { "Failed" },
                call.name
            );
            step.running = false;
            step.detail = text;
            if let Err(error) = store.record_tool(&step) {
                fail_run(store, events, error);
                return;
            }
            send(events, AppEvent::ToolActivity(step));
            if cancelled.load(Ordering::Acquire) {
                fail_run(store, events, "Stopped by user.".to_owned());
                return;
            }
        }
        if turn == 15 {
            final_text.push_str(
                "\n\nStopped after the sixteen-turn limit. Send a new message to continue.",
            );
        }
    }
    if final_text.is_empty() {
        final_text = "Agent run completed.".to_owned();
    }

    if emitted < final_text.len() {
        send(
            events,
            AppEvent::ChatDelta {
                id: assistant_id,
                text: final_text[emitted..].to_owned(),
            },
        );
    }
    match store.replace_message_content(assistant_id, &final_text) {
        Ok(()) => send(events, AppEvent::ChatFinished { id: assistant_id }),
        Err(error) => fail_run(store, events, error),
    }
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
}

use super::*;

pub(super) fn run_agent(
    store: &Store,
    runtime: &Runtime,
    events: &Sender<AppEvent>,
    commands: &Receiver<AppCommand>,
    deferred: &mut VecDeque<AppCommand>,
    settings: Settings,
    content: String,
) {
    if content.trim().is_empty() {
        return;
    }
    let user_id = match store.add_message(MessageRole::User, content.trim()) {
        Ok(id) => id,
        Err(error) => {
            fail_run(events, error);
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
            fail_run(events, error);
            return;
        }
    };
    send(events, AppEvent::ChatStarted { id: assistant_id });
    let mut final_text = String::new();
    let history = match store.load() {
        Ok((_, _, messages)) => messages,
        Err(error) => {
            fail_run(events, error);
            return;
        }
    };
    let mut messages = lm_studio::agent_history(&history);
    for turn in 0..8 {
        let response = match runtime.block_on(lm_studio::agent_turn(
            &settings,
            &messages,
            tools::definitions(),
        )) {
            Ok(response) => response,
            Err(error) => {
                send(events, AppEvent::ConnectionChanged(false));
                fail_run(events, error);
                return;
            }
        };
        send(events, AppEvent::ConnectionChanged(true));
        if !response.content.is_empty() {
            final_text.push_str(&response.content);
        }
        if response.tool_calls.is_empty() {
            break;
        }
        lm_studio::append_tool_calls(&mut messages, &response);
        for call in response.tool_calls {
            send(
                events,
                AppEvent::ToolActivity(format!("{} {}", call.name, call.arguments)),
            );
            if tools::requires_approval(&call.name) {
                send(
                    events,
                    AppEvent::ApprovalRequested {
                        request_id: call.id.clone(),
                        summary: tools::approval_summary(&call.name, &call.arguments),
                    },
                );
                match wait_for_approval(commands, deferred, &call.id) {
                    Ok(true) => {}
                    Ok(false) => {
                        lm_studio::append_tool_result(
                            &mut messages,
                            &call,
                            "Denied by the user. Do not retry this operation.",
                        );
                        final_text.push_str("\n\nTool operation was denied by the user.");
                        continue;
                    }
                    Err(error) => {
                        fail_run(events, error);
                        return;
                    }
                }
            }
            let observation = match tools::execute(&settings, &call.name, &call.arguments) {
                Ok(result) => result,
                Err(error) => format!("Tool error: {error}"),
            };
            let text = format!("Tool {} result:\n{}", call.name, observation);
            lm_studio::append_tool_result(&mut messages, &call, &observation);
            send(events, AppEvent::ToolActivity(text));
        }
        if turn == 7 {
            final_text.push_str(
                "\n\nStopped after the eight-turn limit. Send a new message to continue.",
            );
        }
    }
    if final_text.is_empty() {
        final_text = "Agent run completed.".to_owned();
    }
    send(
        events,
        AppEvent::ChatDelta {
            id: assistant_id,
            text: final_text.clone(),
        },
    );
    match store.replace_message_content(assistant_id, &final_text) {
        Ok(()) => send(events, AppEvent::ChatFinished { id: assistant_id }),
        Err(error) => fail_run(events, error),
    }
}

fn wait_for_approval(
    commands: &Receiver<AppCommand>,
    deferred: &mut VecDeque<AppCommand>,
    request_id: &str,
) -> Result<bool, String> {
    loop {
        match commands
            .recv()
            .map_err(|_| "Agent approval channel closed.".to_owned())?
        {
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
        assert!(!wait_for_approval(&rx, &mut deferred, "current").unwrap());
        assert!(
            matches!(deferred.pop_front(), Some(AppCommand::AddTask(title)) if title == "keep me")
        );
        assert!(deferred.is_empty());
    }
}

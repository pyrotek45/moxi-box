use std::{
    sync::mpsc::{Receiver, Sender},
    thread,
};

use tokio::runtime::Runtime;

use crate::{
    core::{AppCommand, AppEvent, MessageRole, Settings},
    models,
    storage::Store,
    tools,
};

pub fn start(command_receiver: Receiver<AppCommand>, event_sender: Sender<AppEvent>) {
    thread::Builder::new()
        .name("moxi-box-backend".to_owned())
        .spawn(move || {
            let runtime = match Runtime::new() {
                Ok(runtime) => runtime,
                Err(error) => {
                    let _ = event_sender.send(AppEvent::Error(error.to_string()));
                    return;
                }
            };
            let store = match Store::open() {
                Ok(store) => store,
                Err(error) => {
                    let _ = event_sender.send(AppEvent::Error(format!(
                        "Cannot open local storage: {error}"
                    )));
                    return;
                }
            };
            while let Ok(command) = command_receiver.recv() {
                handle(command, &store, &runtime, &event_sender, &command_receiver);
            }
        })
        .expect("backend worker thread should start");
}

fn handle(
    command: AppCommand,
    store: &Store,
    runtime: &Runtime,
    events: &Sender<AppEvent>,
    commands: &Receiver<AppCommand>,
) {
    match command {
        AppCommand::Load => match store.load() {
            Ok((settings, tasks, messages)) => send(
                events,
                AppEvent::Loaded {
                    settings,
                    tasks,
                    messages,
                },
            ),
            Err(error) => fail(events, error),
        },
        AppCommand::SaveSettings(settings) => match store.save_settings(&settings) {
            Ok(()) => send(events, AppEvent::Notice("Settings saved.".to_owned())),
            Err(error) => fail(events, error),
        },
        AppCommand::RefreshModels { base_url } => {
            match runtime.block_on(models::list_models(&base_url)) {
                Ok(models) => {
                    send(events, AppEvent::ConnectionChanged(true));
                    send(events, AppEvent::ModelsLoaded(models));
                }
                Err(error) => {
                    send(events, AppEvent::ConnectionChanged(false));
                    fail(events, error)
                }
            }
        }
        AppCommand::BrowseWorkspace { root } => match tools::workspace_entries(&root) {
            Ok(entries) => send(events, AppEvent::WorkspaceLoaded(entries)),
            Err(error) => fail(events, error),
        },
        AppCommand::ReadPreview { root, path } => match tools::preview(&root, &path) {
            Ok(content) => send(events, AppEvent::PreviewLoaded { path, content }),
            Err(error) => fail(events, error),
        },
        AppCommand::AddTask(title) => {
            if !title.trim().is_empty() {
                match store.add_task(title.trim()) {
                    Ok(task) => send(events, AppEvent::TaskAdded(task)),
                    Err(error) => fail(events, error),
                }
            }
        }
        AppCommand::SetTaskComplete { id, complete } => match store.set_task_complete(id, complete)
        {
            Ok(()) => send(events, AppEvent::TaskChanged { id, complete }),
            Err(error) => fail(events, error),
        },
        AppCommand::SendChat {
            settings,
            content,
            agent_mode,
        } => {
            if agent_mode {
                run_agent(store, runtime, events, commands, settings, content)
            } else {
                send_chat(store, runtime, events, settings, content)
            }
        }
        AppCommand::ResolveApproval { .. } => {
            fail(events, "No tool operation is awaiting approval.".to_owned())
        }
    }
}

fn run_agent(
    store: &Store,
    runtime: &Runtime,
    events: &Sender<AppEvent>,
    commands: &Receiver<AppCommand>,
    settings: Settings,
    content: String,
) {
    if content.trim().is_empty() {
        return;
    }
    let user_id = match store.add_message(MessageRole::User, content.trim()) {
        Ok(id) => id,
        Err(error) => {
            fail(events, error);
            return;
        }
    };
    send(
        events,
        AppEvent::ChatUser(crate::core::ChatMessage {
            id: user_id,
            role: MessageRole::User,
            content: content.trim().to_owned(),
        }),
    );
    let assistant_id = match store.add_message(MessageRole::Assistant, "") {
        Ok(id) => id,
        Err(error) => {
            fail(events, error);
            return;
        }
    };
    send(events, AppEvent::ChatStarted { id: assistant_id });
    let mut final_text = String::new();
    for _ in 0..8 {
        let history = match store.load() {
            Ok((_, _, messages)) => messages,
            Err(error) => {
                fail(events, error);
                return;
            }
        };
        let response = match runtime.block_on(models::agent_turn(
            &settings,
            &history,
            tools::definitions(),
        )) {
            Ok(response) => response,
            Err(error) => {
                send(events, AppEvent::ConnectionChanged(false));
                fail(events, error);
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
                match wait_for_approval(commands, &call.id) {
                    Ok(true) => {}
                    Ok(false) => {
                        final_text.push_str("\n\nTool operation was denied by the user.");
                        continue;
                    }
                    Err(error) => {
                        fail(events, error);
                        return;
                    }
                }
            }
            let observation = match tools::execute(&settings, &call.name, &call.arguments) {
                Ok(result) => result,
                Err(error) => format!("Tool error: {error}"),
            };
            let text = format!("Tool {} result:\n{}", call.name, observation);
            if let Err(error) = store.add_message(MessageRole::System, &text) {
                fail(events, error);
                return;
            }
            send(events, AppEvent::ToolActivity(text));
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
        Err(error) => fail(events, error),
    }
}

fn wait_for_approval(commands: &Receiver<AppCommand>, request_id: &str) -> Result<bool, String> {
    loop {
        match commands
            .recv()
            .map_err(|_| "Agent approval channel closed.".to_owned())?
        {
            AppCommand::ResolveApproval {
                request_id: received,
                approved,
            } if received == request_id => return Ok(approved),
            _ => {}
        }
    }
}

fn send_chat(
    store: &Store,
    runtime: &Runtime,
    events: &Sender<AppEvent>,
    settings: crate::core::Settings,
    content: String,
) {
    if content.trim().is_empty() {
        return;
    }
    let user_id = match store.add_message(MessageRole::User, content.trim()) {
        Ok(id) => id,
        Err(error) => {
            fail(events, error);
            return;
        }
    };
    send(
        events,
        AppEvent::ChatUser(crate::core::ChatMessage {
            id: user_id,
            role: MessageRole::User,
            content: content.trim().to_owned(),
        }),
    );
    let assistant_id = match store.add_message(MessageRole::Assistant, "") {
        Ok(id) => id,
        Err(error) => {
            fail(events, error);
            return;
        }
    };
    send(events, AppEvent::ChatStarted { id: assistant_id });
    let history = match store.load() {
        Ok((_, _, messages)) => messages,
        Err(error) => {
            fail(events, error);
            return;
        }
    };
    let result = runtime.block_on(models::stream_chat(&settings, &history, |text| {
        send(
            events,
            AppEvent::ChatDelta {
                id: assistant_id,
                text,
            },
        )
    }));
    match result {
        Ok(content) => match store.replace_message_content(assistant_id, &content) {
            Ok(()) => {
                send(events, AppEvent::ConnectionChanged(true));
                send(events, AppEvent::ChatFinished { id: assistant_id });
            }
            Err(error) => fail(events, error),
        },
        Err(error) => {
            send(events, AppEvent::ConnectionChanged(false));
            fail(events, error);
        }
    }
}

fn send(events: &Sender<AppEvent>, event: AppEvent) {
    let _ = events.send(event);
}
fn fail(events: &Sender<AppEvent>, error: String) {
    send(events, AppEvent::Error(error));
}

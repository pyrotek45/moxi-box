mod agent;

use std::{
    collections::VecDeque,
    sync::mpsc::{Receiver, Sender},
    thread,
};

use tokio::runtime::Runtime;

use crate::{
    domain::{AppCommand, AppEvent, MessageRole, Settings},
    provider::lm_studio,
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
            let mut deferred = VecDeque::new();
            while let Some(command) = deferred
                .pop_front()
                .or_else(|| command_receiver.recv().ok())
            {
                handle(
                    command,
                    &store,
                    &runtime,
                    &event_sender,
                    &command_receiver,
                    &mut deferred,
                );
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
    deferred: &mut VecDeque<AppCommand>,
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
            match runtime.block_on(lm_studio::list_models(&base_url)) {
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
                agent::run_agent(
                    store, runtime, events, commands, deferred, settings, content,
                )
            } else {
                send_chat(store, runtime, events, settings, content)
            }
        }
        AppCommand::ResolveApproval { .. } => {
            fail(events, "No tool operation is awaiting approval.".to_owned())
        }
    }
}

fn send_chat(
    store: &Store,
    runtime: &Runtime,
    events: &Sender<AppEvent>,
    settings: crate::domain::Settings,
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
    let history = match store.load() {
        Ok((_, _, messages)) => messages,
        Err(error) => {
            fail_run(events, error);
            return;
        }
    };
    let result = runtime.block_on(lm_studio::stream_chat(&settings, &history, |text| {
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
            Err(error) => fail_run(events, error),
        },
        Err(error) => {
            send(events, AppEvent::ConnectionChanged(false));
            fail_run(events, error);
        }
    }
}

fn send(events: &Sender<AppEvent>, event: AppEvent) {
    let _ = events.send(event);
}
fn fail(events: &Sender<AppEvent>, error: String) {
    send(events, AppEvent::Error(error));
}

fn fail_run(events: &Sender<AppEvent>, error: String) {
    send(events, AppEvent::RunFailed(error));
}

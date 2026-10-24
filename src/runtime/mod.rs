mod agent;
mod context;

use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, Sender},
    },
    thread,
};

use tokio::runtime::Runtime;

use crate::{
    domain::{AppCommand, AppEvent, MessageRole, Settings},
    provider::lm_studio,
    storage::Store,
    tools,
};

struct EventSender {
    channel: Sender<AppEvent>,
    wake: Box<dyn Fn() + Send>,
}
impl EventSender {
    fn send(&self, event: AppEvent) -> bool {
        let result = self.channel.send(event).is_ok();
        (self.wake)();
        result
    }
}

pub(super) struct AgentRequest {
    settings: Settings,
    content: String,
}

pub fn start(
    command_receiver: Receiver<AppCommand>,
    event_sender: Sender<AppEvent>,
    wake: impl Fn() + Send + 'static,
) {
    let event_sender = EventSender {
        channel: event_sender,
        wake: Box::new(wake),
    };
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
    events: &EventSender,
    commands: &Receiver<AppCommand>,
    deferred: &mut VecDeque<AppCommand>,
) {
    match command {
        AppCommand::Load => match store.load() {
            Ok((settings, tasks, messages)) => {
                send(
                    events,
                    AppEvent::Loaded {
                        settings,
                        tasks,
                        messages,
                    },
                );
                sync_conversation(store, events);
            }
            Err(error) => fail(events, error),
        },
        AppCommand::SaveSettings(settings) => match store.save_settings(&settings) {
            Ok(()) => {}
            Err(error) => fail(events, error),
        },
        AppCommand::RefreshModels {
            request_id,
            base_url,
            cancelled,
        } => {
            let result = runtime.block_on(lm_studio::list_models(&base_url, &cancelled));
            send(events, AppEvent::ModelsLoaded { request_id, result });
        }
        AppCommand::NewConversation { workspace } => match store.new_conversation(&workspace) {
            Ok(()) => sync_conversation(store, events),
            Err(error) => fail(events, error),
        },
        AppCommand::SelectConversation { workspace, id } => {
            match store.select_conversation(&workspace, id) {
                Ok(()) => sync_conversation(store, events),
                Err(error) => fail(events, error),
            }
        }
        AppCommand::SaveMemory { workspace, content } => {
            match store.save_memory(&workspace, &content) {
                Ok(()) => sync_conversation(store, events),
                Err(error) => fail(events, error),
            }
        }
        AppCommand::ResetWorkspace { path } => {
            let result = tools::reset_workspace(&path).and_then(|backup| {
                store.new_conversation(&path)?;
                store.add_message(
                    MessageRole::Assistant,
                    &format!("Workspace reset. Original files are archived at:\n{backup}"),
                )?;
                send(
                    events,
                    AppEvent::Notice(format!(
                        "Project reset. Original files archived at {backup}"
                    )),
                );
                tools::open_workspace(&path, false)
            });
            let success = result.is_ok();
            send(events, AppEvent::WorkspaceOpened(result));
            if success {
                sync_conversation(store, events);
            }
        }
        AppCommand::OpenWorkspace { path, create } => {
            let result = tools::open_workspace(&path, create).and_then(|(root, entries)| {
                store.select_workspace(&root)?;
                Ok((root, entries))
            });
            let success = result.is_ok();
            send(events, AppEvent::WorkspaceOpened(result));
            if success {
                sync_conversation(store, events);
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
            cancelled,
        } => {
            if agent_mode {
                agent::run_agent(
                    store,
                    runtime,
                    events,
                    commands,
                    deferred,
                    AgentRequest { settings, content },
                    &cancelled,
                )
            } else {
                send_chat(store, runtime, events, settings, content, &cancelled)
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
    events: &EventSender,
    settings: crate::domain::Settings,
    content: String,
    cancelled: &AtomicBool,
) {
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
    let history = match context::prepare(store, runtime, &settings, events, cancelled) {
        Ok(messages) => messages,
        Err(error) => {
            fail_run(store, events, error);
            return;
        }
    };
    let mut partial = String::new();
    let mut checkpoint = std::time::Instant::now();
    let mut checkpoint_error = None;
    let result = runtime.block_on(lm_studio::stream_chat(
        &settings,
        &history,
        cancelled,
        |text| {
            partial.push_str(&text);
            if checkpoint.elapsed() >= std::time::Duration::from_millis(250) {
                if let Err(error) = store.replace_message_content(assistant_id, &partial) {
                    checkpoint_error = Some(error);
                }
                checkpoint = std::time::Instant::now();
            }
            send(
                events,
                AppEvent::ChatDelta {
                    id: assistant_id,
                    text,
                },
            )
        },
    ));
    if let Err(error) = store.replace_message_content(assistant_id, &partial) {
        fail_run(store, events, error);
        return;
    }
    if let Some(error) = checkpoint_error {
        fail_run(store, events, error);
        return;
    }
    match result {
        Ok(content) => match store.replace_message_content(assistant_id, &content) {
            Ok(()) => {
                send(events, AppEvent::ConnectionChanged(true));
                send(events, AppEvent::ChatFinished { id: assistant_id });
            }
            Err(error) => fail_run(store, events, error),
        },
        Err(error) => {
            if !cancelled.load(Ordering::Acquire) {
                send(events, AppEvent::ConnectionChanged(false));
            }
            fail_run(store, events, error);
        }
    }
}

fn send(events: &EventSender, event: AppEvent) {
    let _ = events.send(event);
}
fn fail(events: &EventSender, error: String) {
    send(events, AppEvent::Error(error));
}

fn fail_run(store: &Store, events: &EventSender, error: String) {
    if let Ok(state) = store.conversation_state() {
        if let Some(message) = state
            .messages
            .last()
            .filter(|m| m.role == MessageRole::Assistant)
        {
            let suffix = format!("\n\n[Run interrupted: {error}]");
            let _ = store
                .replace_message_content(message.id, &format!("{}{}", message.content, suffix));
            send(
                events,
                AppEvent::ChatDelta {
                    id: message.id,
                    text: suffix,
                },
            );
        }
    }
    send(events, AppEvent::RunFailed(error));
}

fn sync_conversation(store: &Store, events: &EventSender) {
    match store.tasks() {
        Ok(tasks) => send(events, AppEvent::TasksUpdated(tasks)),
        Err(error) => fail(events, error),
    }
    match store.conversation_state() {
        Ok(state) => send(events, AppEvent::ConversationLoaded(state)),
        Err(error) => fail(events, error),
    }
}

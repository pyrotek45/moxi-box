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
    auto_approve: std::sync::Arc<AtomicBool>,
    /// Chat mode: the same loop with only task and web tools, and no task stop gate.
    chat: bool,
}

pub fn start(
    command_receiver: Receiver<AppCommand>,
    event_sender: Sender<AppEvent>,
    wake: impl Fn() + Send + Sync + 'static,
) {
    // Route read-only browsing independently of the long-running inference worker.
    let wake = std::sync::Arc::new(wake);
    let browse_wake = wake.clone();
    let browse_events = event_sender.clone();
    let (backend_sender, backend_receiver) = std::sync::mpsc::channel();
    thread::spawn(move || {
        while let Ok(command) = command_receiver.recv() {
            let event = match command {
                AppCommand::ReadPreview { root, path } => match tools::preview(&root, &path) {
                    Ok(content) => AppEvent::PreviewLoaded { path, content },
                    Err(error) => AppEvent::Notice(error),
                },
                AppCommand::BrowseWorkspace { root, expanded } => browse(root, &expanded),
                command => {
                    if backend_sender.send(command).is_err() {
                        break;
                    }
                    continue;
                }
            };
            if browse_events.send(event).is_err() {
                break;
            }
            browse_wake();
        }
    });
    let command_receiver = backend_receiver;
    let event_sender = EventSender {
        channel: event_sender,
        wake: Box::new(move || wake()),
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
            match store.recover_interrupted_run() {
                Ok(true) => send(
                    &event_sender,
                    AppEvent::Notice(
                        "Recovered from an unexpected shutdown. The unfinished run is marked interrupted."
                            .to_owned(),
                    ),
                ),
                Ok(false) => {}
                Err(error) => fail(&event_sender, error),
            }
            let mut deferred = VecDeque::new();
            while let Some(command) = deferred
                .pop_front()
                .or_else(|| command_receiver.recv().ok())
            {
                let running = matches!(command, AppCommand::SendChat { .. });
                // A bug in one command must not take down the worker; SQLite keeps data consistent.
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    handle(
                        command,
                        &store,
                        &runtime,
                        &event_sender,
                        &command_receiver,
                        &mut deferred,
                    )
                }));
                if outcome.is_err() {
                    let error = "Internal error; the action was stopped. Details are in crash.log beside the database.".to_owned();
                    if running {
                        fail_run(&store, &event_sender, error);
                    } else {
                        fail(&event_sender, error);
                    }
                }
                if running {
                    let _ = store.set_active_run(None);
                }
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
        AppCommand::NewWorkspace(name) => {
            sync_after(store, events, store.create_named_workspace(&name))
        }
        AppCommand::RenameWorkspace { id, name } => {
            sync_after(store, events, store.rename_workspace(id, &name))
        }
        AppCommand::SelectWorkspace(id) => sync_after(store, events, store.select(id)),
        AppCommand::DeleteWorkspace(id) => sync_after(store, events, store.delete_workspace(id)),
        AppCommand::SaveMemory(content) => sync_after(store, events, store.save_memory(&content)),
        AppCommand::SavePlaybook(content) => {
            sync_after(store, events, store.save_playbook(&content))
        }
        AppCommand::ExportSettings(path) => match store.export_settings(&path) {
            Ok(()) => send(
                events,
                AppEvent::Notice(format!("Settings exported to {}", path.display())),
            ),
            Err(error) => fail(events, error),
        },
        AppCommand::ImportSettings(path) => match store.import_settings(&path) {
            Ok(settings) => {
                send(events, AppEvent::SettingsImported(settings));
                send(events, AppEvent::Notice("Settings imported.".to_owned()));
                sync_conversation(store, events);
            }
            Err(error) => fail(events, error),
        },
        AppCommand::SetGoal(goal) => sync_after(store, events, store.save_goal(&goal)),
        AppCommand::ResetWorkspace { path } => {
            let result = tools::reset_workspace(&path).and_then(|backup| {
                store.add_message(
                    MessageRole::Assistant,
                    &lm_studio::harness_note(&format!(
                        "Folder reset. The original files are archived at {backup}"
                    )),
                )?;
                send(
                    events,
                    AppEvent::Notice(format!("Folder reset. Original files archived at {backup}")),
                );
                tools::open_workspace(&path, false)
            });
            let success = result.is_ok();
            send(events, AppEvent::WorkspaceOpened(result));
            if success {
                sync_conversation(store, events);
            }
        }
        AppCommand::AttachFolder { path, create } => {
            let result = if path.is_empty() {
                store.set_folder("").map(|()| (String::new(), Vec::new()))
            } else {
                tools::open_workspace(&path, create).and_then(|(root, entries)| {
                    store.set_folder(&root)?;
                    Ok((root, entries))
                })
            };
            let success = result.is_ok();
            send(events, AppEvent::WorkspaceOpened(result));
            if success {
                sync_conversation(store, events);
            }
        }
        AppCommand::BrowseWorkspace { root, expanded } => send(events, browse(root, &expanded)),
        AppCommand::ReadPreview { root, path } => match tools::preview(&root, &path) {
            Ok(content) => send(events, AppEvent::PreviewLoaded { path, content }),
            Err(error) => fail(events, error),
        },
        command @ (AppCommand::AddTask(_)
        | AppCommand::SetTaskComplete { .. }
        | AppCommand::DeleteTask(_)
        | AppCommand::ClearTasks { .. }) => task_command(store, events, command),
        AppCommand::SendChat {
            mut settings,
            content,
            agent_mode,
            auto_approve,
            cancelled,
        } => {
            // Queued behind a run the user stopped.
            if cancelled.load(Ordering::Acquire) {
                return;
            }
            // The store, not a possibly stale UI copy, decides which folder tools may touch.
            match store.folder() {
                Ok(folder) => settings.workspace_root = folder,
                Err(error) => return fail(events, error),
            }
            agent::run_agent(
                store,
                runtime,
                events,
                commands,
                deferred,
                AgentRequest {
                    settings,
                    content,
                    auto_approve,
                    chat: !agent_mode,
                },
                &cancelled,
            )
        }
        // A late click after the run ended or was stopped has nothing left to resolve.
        AppCommand::ResolveApproval { .. } | AppCommand::AnswerQuestion { .. } => {}
    }
}

/// Periodic and quiet: a folder that vanished shows as empty rather than spamming notices.
fn browse(root: String, expanded: &[String]) -> AppEvent {
    let entries = tools::tree_entries(&root, expanded).unwrap_or_default();
    AppEvent::WorkspaceLoaded { root, entries }
}

/// Task edits from the sidebar; also applied between agent turns so the user can steer a run.
fn task_command(store: &Store, events: &EventSender, command: AppCommand) {
    let result = match command {
        AppCommand::AddTask(title) if title.trim().is_empty() => return,
        AppCommand::AddTask(title) => store.add_task(title.trim()).map(|_| ()),
        AppCommand::SetTaskComplete { id, complete } => store.set_task_complete(id, complete),
        AppCommand::DeleteTask(id) => store.delete_task(id),
        AppCommand::ClearTasks { completed_only } => store.clear_tasks(completed_only),
        _ => return,
    };
    if let Err(error) = result {
        fail(events, error);
    }
    match store.tasks() {
        Ok(tasks) => send(events, AppEvent::TasksUpdated(tasks)),
        Err(error) => fail(events, error),
    }
}

fn is_task_command(command: &AppCommand) -> bool {
    matches!(
        command,
        AppCommand::AddTask(_)
            | AppCommand::SetTaskComplete { .. }
            | AppCommand::DeleteTask(_)
            | AppCommand::ClearTasks { .. }
    )
}

/// Save the user message and an empty reply placeholder; returns (user_id, reply_id).
fn begin_run(store: &Store, events: &EventSender, content: &str) -> Option<(i64, i64)> {
    let content = content.trim();
    if content.is_empty() {
        return None;
    }
    let user_id = match store.add_message(MessageRole::User, content) {
        Ok(id) => id,
        Err(error) => {
            send(events, AppEvent::RunFailed(error));
            return None;
        }
    };
    send(
        events,
        AppEvent::ChatUser(crate::domain::ChatMessage {
            id: user_id,
            role: MessageRole::User,
            content: content.to_owned(),
        }),
    );
    match store
        .add_message(MessageRole::Assistant, "")
        .and_then(|id| store.set_active_run(Some(id)).map(|()| id))
    {
        Ok(reply_id) => {
            send(events, AppEvent::ChatStarted { id: reply_id });
            Some((user_id, reply_id))
        }
        Err(error) => {
            fail_run(store, events, error);
            None
        }
    }
}

/// Only a transport failure means the provider is gone; an HTTP error proves it answered.
fn provider_failed(store: &Store, events: &EventSender, cancelled: &AtomicBool, error: String) {
    if !cancelled.load(Ordering::Acquire) && lm_studio::is_unreachable(&error) {
        send(events, AppEvent::ProviderUnreachable);
    }
    fail_run(store, events, error);
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

fn sync_after(store: &Store, events: &EventSender, result: Result<(), String>) {
    match result {
        Ok(()) => sync_conversation(store, events),
        Err(error) => fail(events, error),
    }
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

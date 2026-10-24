use std::sync::{
    atomic::AtomicBool,
    mpsc::{self, Receiver, Sender},
    Arc,
};

use eframe::egui;

use crate::{
    domain::{
        AgentActivity, AppCommand, AppEvent, ChatMessage, MessageRole, Settings, Task, Workspace,
        WorkspaceEntry, APPLICATION_NAME, DEFAULT_WINDOW_HEIGHT, DEFAULT_WINDOW_WIDTH,
    },
    runtime,
    ui::{theme, widgets::ProviderStatus},
};

mod connection;
mod history;
mod view;
mod workspaces;

use connection::Connection;
use history::PromptHistory;
use workspaces::WorkspaceDialog;

pub fn run() -> eframe::Result<()> {
    eframe::run_native(
        APPLICATION_NAME,
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([DEFAULT_WINDOW_WIDTH, DEFAULT_WINDOW_HEIGHT])
                .with_min_inner_size([640.0, 480.0]),
            ..Default::default()
        },
        Box::new(|context| Ok(Box::new(MoxiBoxApp::new(&context.egui_ctx)))),
    )
}

pub struct MoxiBoxApp {
    commands: Sender<AppCommand>,
    cancelled: Arc<AtomicBool>,
    show_settings: bool,
    events: Receiver<AppEvent>,
    settings: Settings,
    models: Vec<String>,
    tasks: Vec<Task>,
    messages: Vec<ChatMessage>,
    composer: String,
    new_task: String,
    agent_mode: bool,
    streaming_id: Option<i64>,
    notice: Option<String>,
    connection: Connection,
    provider_draft: Option<Settings>,
    provider_form_error: Option<String>,
    workspace_dialog: Option<WorkspaceDialog>,
    workspace_path: String,
    workspace_error: Option<String>,
    workspace_pending: bool,
    workspace_name: String,
    folder_picker: Option<(workspaces::FolderPick, Receiver<Option<std::path::PathBuf>>)>,
    history: PromptHistory,
    /// (request id, summary, preview) of the tool call awaiting approval.
    approval: Option<(String, String, String)>,
    auto_approve: Arc<AtomicBool>,
    activity: Vec<AgentActivity>,
    workspaces: Vec<Workspace>,
    workspace_id: Option<i64>,
    /// A workspace switch, creation, or deletion is in flight.
    switching: bool,
    summary: String,
    compacted_through: i64,
    show_full_history: bool,
    memory: String,
    memory_draft: Option<String>,
    workspace_entries: Vec<WorkspaceEntry>,
    expanded_folders: std::collections::HashSet<String>,
    preview: Option<(String, String)>,
    run_pending: bool,
    show_workspace: bool,
    worker_lost: bool,
    /// Messages sent during a run, shown until the worker picks them up.
    queued: std::collections::VecDeque<String>,
    /// When the file tree was last requested; it is re-scanned periodically to follow outside changes.
    tree_requested: std::time::Instant,
    tree_pending: bool,
    /// Last sign of progress in the current run, for the waiting timer.
    activity_at: std::time::Instant,
}

impl MoxiBoxApp {
    fn new(context: &egui::Context) -> Self {
        theme::install_theme(context);
        let (command_sender, command_receiver) = mpsc::channel();
        let (event_sender, event_receiver) = mpsc::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        let repaint = context.clone();
        runtime::start(command_receiver, event_sender, move || {
            repaint.request_repaint()
        });
        let app = Self {
            commands: command_sender,
            cancelled,
            show_settings: false,
            events: event_receiver,
            settings: Settings::default(),
            models: Vec::new(),
            tasks: Vec::new(),
            messages: Vec::new(),
            composer: String::new(),
            new_task: String::new(),
            agent_mode: false,
            streaming_id: None,
            notice: None,
            connection: Connection::default(),
            provider_draft: None,
            provider_form_error: None,
            workspace_dialog: None,
            workspace_path: String::new(),
            workspace_error: None,
            workspace_pending: false,
            workspace_name: String::new(),
            folder_picker: None,
            history: PromptHistory::default(),
            approval: None,
            auto_approve: Arc::new(AtomicBool::new(false)),
            activity: Vec::new(),
            workspaces: Vec::new(),
            workspace_id: None,
            switching: false,
            summary: String::new(),
            compacted_through: 0,
            show_full_history: false,
            memory: String::new(),
            memory_draft: None,
            workspace_entries: Vec::new(),
            expanded_folders: Default::default(),
            preview: None,
            run_pending: false,
            show_workspace: true,
            worker_lost: false,
            queued: Default::default(),
            tree_requested: std::time::Instant::now(),
            tree_pending: false,
            activity_at: std::time::Instant::now(),
        };
        app.command(AppCommand::Load);
        app
    }

    fn command(&self, command: AppCommand) {
        let _ = self.commands.send(command);
    }

    fn handle_events(&mut self, context: &egui::Context) {
        loop {
            let event = match self.events.try_recv() {
                Ok(event) => event,
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    if self.run_pending || !self.worker_lost {
                        self.worker_lost = true;
                        self.run_pending = false;
                        self.streaming_id = None;
                        self.approval = None;
                        self.notice = Some("The background worker stopped. Your conversations are saved; restart Moxi Box to continue.".to_owned());
                    }
                    break;
                }
            };
            match event {
                AppEvent::Loaded {
                    settings,
                    tasks,
                    messages,
                } => {
                    self.settings = settings;
                    self.tasks = tasks;
                    self.messages = messages;
                    self.history = PromptHistory::from_messages(&self.messages);
                    self.refresh_models(false);
                }
                AppEvent::ModelsLoaded { request_id, result } => {
                    self.models_loaded(request_id, result)
                }
                AppEvent::WorkspaceOpened(result) => self.workspace_opened(result),
                AppEvent::ProviderUnreachable => self.connection.disconnect(),
                AppEvent::WorkspaceLoaded(entries) => {
                    self.tree_pending = false;
                    self.workspace_entries = entries;
                }
                AppEvent::PreviewLoaded { path, content } => self.preview = Some((path, content)),
                AppEvent::ChatUser(message) => {
                    // Several queued messages may arrive combined into one turn.
                    while self
                        .queued
                        .front()
                        .is_some_and(|queued| message.content.contains(queued.as_str()))
                    {
                        self.queued.pop_front();
                    }
                    self.messages.push(message);
                }
                AppEvent::ChatStarted { id } => {
                    self.messages.push(ChatMessage {
                        id,
                        role: MessageRole::Assistant,
                        content: String::new(),
                    });
                    self.streaming_id = Some(id);
                    self.run_pending = true;
                    self.activity_at = std::time::Instant::now();
                }
                AppEvent::ChatDelta { id, text } => {
                    self.activity_at = std::time::Instant::now();
                    if let Some(message) = self.messages.iter_mut().find(|message| message.id == id)
                    {
                        message.content.push_str(&text);
                    }
                }
                AppEvent::ChatFinished { id } => {
                    if self.streaming_id == Some(id) {
                        self.streaming_id = None;
                    }
                    self.run_pending = false;
                    // Refresh titles and the saved summary after each completed turn.
                    if let Some(id) = self.workspace_id {
                        self.command(AppCommand::SelectWorkspace(id));
                    }
                }
                AppEvent::RunProgress => self.activity_at = std::time::Instant::now(),
                AppEvent::ToolActivity(activity) => {
                    self.activity_at = std::time::Instant::now();
                    if let Some(old) = self.activity.iter_mut().find(|old| {
                        old.message_id == activity.message_id && old.call_id == activity.call_id
                    }) {
                        *old = activity;
                    } else {
                        self.activity.push(activity);
                    }
                }
                AppEvent::ContextCompacted { summary, through } => {
                    self.compacted_through = through;
                    self.summary = summary;
                    self.notice =
                        Some("Earlier turns summarized. Full history remains saved.".to_owned());
                }
                AppEvent::ConversationLoaded(state) => {
                    if self.settings.workspace_root != state.folder {
                        self.settings.workspace_root = state.folder;
                        self.workspace_entries.clear();
                        self.expanded_folders.clear();
                        self.preview = None;
                        self.tree_pending = false;
                        self.tree_requested =
                            std::time::Instant::now() - std::time::Duration::from_secs(10);
                    }
                    self.workspace_id = Some(state.current);
                    self.workspaces = state.workspaces;
                    self.messages = state.messages;
                    self.history = PromptHistory::from_messages(&self.messages);
                    self.summary = state.summary;
                    self.compacted_through = state.compacted_through;
                    self.memory = state.memory;
                    self.activity = state.activity;
                    if self.switching {
                        self.notice = None;
                        self.show_full_history = false;
                        self.composer.clear();
                    }
                    self.switching = false;
                }
                AppEvent::ApprovalRequested {
                    request_id,
                    summary,
                    preview,
                } => {
                    self.preview = None;
                    self.approval = Some((request_id, summary, preview));
                }
                AppEvent::TasksUpdated(tasks) => self.tasks = tasks,
                AppEvent::TaskAdded(task) => self.tasks.push(task),
                AppEvent::TaskChanged { id, complete } => {
                    if let Some(task) = self.tasks.iter_mut().find(|task| task.id == id) {
                        task.status = if complete {
                            crate::domain::TaskStatus::Completed
                        } else {
                            crate::domain::TaskStatus::Pending
                        };
                    }
                }
                AppEvent::Notice(message) => self.notice = Some(message),
                AppEvent::Error(message) => {
                    self.switching = false;
                    self.notice = Some(message);
                }
                AppEvent::RunFailed(message) => {
                    for activity in self.activity.iter_mut().filter(|step| step.running) {
                        activity.running = false;
                        activity.title = format!("Interrupted · {}", activity.title);
                    }
                    self.streaming_id = None;
                    self.run_pending = false;
                    self.approval = None;
                    self.notice = Some(message);
                }
            }
            context.request_repaint();
        }
    }

    fn send_message(&mut self) {
        if self.switching || self.workspace_pending || self.composer.trim().is_empty() {
            return;
        }
        if self.workspace_id.is_none() {
            self.notice = Some("Create a workspace first.".to_owned());
            return;
        }
        if !self.connection.connected {
            self.notice =
                Some("Choose a provider and click Connect before sending a message.".to_owned());
            return;
        }
        if self.settings.selected_model.is_none() {
            self.notice = Some("Load and select a model before sending a message.".to_owned());
            return;
        }
        self.notice = None;
        let content = std::mem::take(&mut self.composer);
        self.history.record(&content);
        if self.run_pending {
            // Steering: the running agent reads this at its next step; Stop also drops it.
            self.queued.push_back(content.trim().to_owned());
        } else {
            self.run_pending = true;
            self.cancelled = Arc::new(AtomicBool::new(false));
        }
        self.command(AppCommand::SendChat {
            settings: self.settings.clone(),
            content,
            agent_mode: self.agent_mode,
            auto_approve: self.auto_approve.clone(),
            cancelled: self.cancelled.clone(),
        });
    }

    /// Re-scan the folder every two seconds so the tree follows edits made anywhere.
    fn refresh_tree(&mut self, context: &egui::Context) {
        const INTERVAL: std::time::Duration = std::time::Duration::from_secs(2);
        if self.settings.workspace_root.is_empty() || self.workspace_pending || self.worker_lost {
            return;
        }
        if !self.tree_pending && self.tree_requested.elapsed() >= INTERVAL {
            self.tree_pending = true;
            self.tree_requested = std::time::Instant::now();
            self.command(AppCommand::BrowseWorkspace {
                root: self.settings.workspace_root.clone(),
            });
        }
        context.request_repaint_after(INTERVAL);
    }

    /// Counts seconds since the model last sent anything, so slow is distinguishable from stuck.
    fn waiting_label(&self, base: &str) -> String {
        match self.activity_at.elapsed().as_secs() {
            seconds if seconds >= 5 => format!("{base} · no output for {seconds}s"),
            _ => base.to_owned(),
        }
    }

    fn stop(&mut self) {
        self.cancelled
            .store(true, std::sync::atomic::Ordering::Release);
        self.queued.clear();
        self.notice = Some("Stopping…".to_owned());
    }

    fn resolve_approval(&mut self, approved: bool) {
        if let Some((request_id, _, _)) = self.approval.take() {
            self.command(AppCommand::ResolveApproval {
                request_id,
                approved,
            });
        }
    }

    fn set_auto_approve(&mut self, enabled: bool) {
        self.auto_approve
            .store(enabled, std::sync::atomic::Ordering::Release);
        if enabled {
            self.resolve_approval(true);
        }
    }

    fn provider_status(&self) -> ProviderStatus {
        if self.connection.checking {
            ProviderStatus::Checking
        } else if self.connection.connected && self.settings.selected_model.is_none() {
            ProviderStatus::OnlineNoModel
        } else if self.connection.connected {
            ProviderStatus::Ready
        } else if self.connection.available {
            ProviderStatus::Available
        } else {
            ProviderStatus::Offline
        }
    }
}

#[cfg(test)]
mod tests;

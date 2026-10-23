use std::sync::{
    atomic::AtomicBool,
    mpsc::{self, Receiver, Sender},
    Arc,
};

use eframe::egui;

use crate::{
    domain::{
        AgentActivity, AppCommand, AppEvent, ChatMessage, Conversation, MessageRole, Settings,
        Task, WorkspaceEntry, APPLICATION_NAME, DEFAULT_WINDOW_HEIGHT, DEFAULT_WINDOW_WIDTH,
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
    show_toolbelt: bool,
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
    folder_entries: Vec<String>,
    folder_request: u64,
    folder_loading: bool,
    history: PromptHistory,
    approval: Option<(String, String)>,
    activity: Vec<AgentActivity>,
    conversations: Vec<Conversation>,
    conversation_id: Option<i64>,
    conversation_pending: bool,
    summary: String,
    compacted_through: i64,
    show_full_history: bool,
    memory: String,
    memory_draft: Option<String>,
    workspace_entries: Vec<WorkspaceEntry>,
    preview: Option<(String, String)>,
    run_pending: bool,
    show_workspace: bool,
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
            show_toolbelt: false,
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
            folder_entries: Vec::new(),
            folder_request: 0,
            folder_loading: false,
            history: PromptHistory::default(),
            approval: None,
            activity: Vec::new(),
            conversations: Vec::new(),
            conversation_id: None,
            conversation_pending: false,
            summary: String::new(),
            compacted_through: 0,
            show_full_history: false,
            memory: String::new(),
            memory_draft: None,
            workspace_entries: Vec::new(),
            preview: None,
            run_pending: false,
            show_workspace: true,
        };
        app.command(AppCommand::Load);
        app
    }

    fn command(&self, command: AppCommand) {
        let _ = self.commands.send(command);
    }

    fn handle_events(&mut self, context: &egui::Context) {
        while let Ok(event) = self.events.try_recv() {
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
                    self.remember_workspace();
                    self.refresh_models(false);
                    if !self.settings.workspace_root.is_empty() {
                        self.command(AppCommand::BrowseWorkspace {
                            root: self.settings.workspace_root.clone(),
                        });
                    }
                }
                AppEvent::ModelsLoaded { request_id, result } => {
                    self.models_loaded(request_id, result)
                }
                AppEvent::WorkspaceOpened(result) => self.workspace_opened(result),
                AppEvent::FoldersListed { request_id, result } => {
                    if request_id == self.folder_request {
                        self.folder_loading = false;
                        match result {
                            Ok((path, entries)) => {
                                self.workspace_path = path;
                                self.folder_entries = entries;
                                self.workspace_error = None;
                            }
                            Err(error) => self.workspace_error = Some(error),
                        }
                    }
                }
                AppEvent::ConnectionChanged(connected) => {
                    if !connected {
                        self.connection.disconnect();
                    }
                }
                AppEvent::WorkspaceLoaded(entries) => self.workspace_entries = entries,
                AppEvent::PreviewLoaded { path, content } => self.preview = Some((path, content)),
                AppEvent::ChatUser(message) => self.messages.push(message),
                AppEvent::ChatStarted { id } => {
                    self.messages.push(ChatMessage {
                        id,
                        role: MessageRole::Assistant,
                        content: String::new(),
                    });
                    self.streaming_id = Some(id);
                    self.run_pending = true;
                }
                AppEvent::ChatDelta { id, text } => {
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
                    if let Some(id) = self.conversation_id {
                        self.command(AppCommand::SelectConversation {
                            workspace: self.settings.workspace_root.clone(),
                            id,
                        });
                    }
                }
                AppEvent::ToolActivity(activity) => {
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
                    self.conversation_id = Some(state.current);
                    self.conversations = state.conversations;
                    self.messages = state.messages;
                    self.history = PromptHistory::from_messages(&self.messages);
                    self.summary = state.summary;
                    self.compacted_through = state.compacted_through;
                    self.memory = state.memory;
                    self.activity = state.activity;
                    if self.conversation_pending {
                        self.notice = None;
                        self.show_full_history = false;
                        self.composer.clear();
                    }
                    self.conversation_pending = false;
                }
                AppEvent::ApprovalRequested {
                    request_id,
                    summary,
                } => {
                    self.preview = None;
                    self.approval = Some((request_id, summary));
                }
                AppEvent::TaskAdded(task) => self.tasks.push(task),
                AppEvent::TaskChanged { id, complete } => {
                    if let Some(task) = self.tasks.iter_mut().find(|task| task.id == id) {
                        task.complete = complete;
                    }
                }
                AppEvent::Notice(message) => self.notice = Some(message),
                AppEvent::Error(message) => {
                    self.conversation_pending = false;
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
        if self.run_pending
            || self.conversation_pending
            || self.workspace_pending
            || self.composer.trim().is_empty()
        {
            return;
        }
        if self.settings.workspace_root.is_empty() || self.conversation_id.is_none() {
            self.notice = Some("Open a workspace before starting a conversation.".to_owned());
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
        self.run_pending = true;
        self.cancelled = Arc::new(AtomicBool::new(false));
        self.command(AppCommand::SendChat {
            settings: self.settings.clone(),
            content,
            agent_mode: self.agent_mode,
            cancelled: self.cancelled.clone(),
        });
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

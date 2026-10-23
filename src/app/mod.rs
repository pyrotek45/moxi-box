use std::sync::mpsc::{self, Receiver, Sender};

use eframe::egui;

use crate::{
    domain::{
        AppCommand, AppEvent, ChatMessage, MessageRole, Settings, Task, WorkspaceEntry,
        APPLICATION_NAME, DEFAULT_WINDOW_HEIGHT, DEFAULT_WINDOW_WIDTH,
    },
    runtime,
    ui::{theme, widgets::ProviderStatus},
};

mod view;

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
    provider_online: bool,
    approval: Option<(String, String)>,
    activity: Vec<String>,
    workspace_entries: Vec<WorkspaceEntry>,
    preview: Option<(String, String)>,
    run_pending: bool,
    show_workspace: bool,
    connection_settings: bool,
    provider_checking: bool,
}

impl MoxiBoxApp {
    fn new(context: &egui::Context) -> Self {
        theme::install_theme(context);
        let (command_sender, command_receiver) = mpsc::channel();
        let (event_sender, event_receiver) = mpsc::channel();
        runtime::start(command_receiver, event_sender);
        let app = Self {
            commands: command_sender,
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
            provider_online: false,
            approval: None,
            activity: Vec::new(),
            workspace_entries: Vec::new(),
            preview: None,
            run_pending: false,
            show_workspace: true,
            connection_settings: false,
            provider_checking: true,
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
                    self.command(AppCommand::RefreshModels {
                        base_url: self.settings.lm_studio_url.clone(),
                    });
                    self.command(AppCommand::BrowseWorkspace {
                        root: self.settings.workspace_root.clone(),
                    });
                }
                AppEvent::ModelsLoaded(models) => {
                    self.models = models;
                    let selected_is_available = self
                        .settings
                        .selected_model
                        .as_ref()
                        .is_some_and(|selected| self.models.contains(selected));
                    if !selected_is_available {
                        self.settings.selected_model = self.models.first().cloned();
                        self.command(AppCommand::SaveSettings(self.settings.clone()));
                    }
                    self.notice = Some(if self.settings.selected_model.is_some() {
                        "Provider ready.".to_owned()
                    } else {
                        "Server is online, but it has no loaded models.".to_owned()
                    });
                }
                AppEvent::ConnectionChanged(connected) => {
                    self.provider_online = connected;
                    self.provider_checking = false;
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
                }
                AppEvent::ToolActivity(activity) => {
                    self.activity.push(activity);
                }
                AppEvent::ApprovalRequested {
                    request_id,
                    summary,
                } => self.approval = Some((request_id, summary)),
                AppEvent::TaskAdded(task) => self.tasks.push(task),
                AppEvent::TaskChanged { id, complete } => {
                    if let Some(task) = self.tasks.iter_mut().find(|task| task.id == id) {
                        task.complete = complete;
                    }
                }
                AppEvent::Notice(message) => self.notice = Some(message),
                AppEvent::Error(message) => self.notice = Some(message),
                AppEvent::RunFailed(message) => {
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
        if self.run_pending || self.composer.trim().is_empty() {
            return;
        }
        if !self.provider_online {
            self.notice =
                Some("LM Studio is offline. Start its local server and refresh models.".to_owned());
            return;
        }
        if self.settings.selected_model.is_none() {
            self.notice = Some("Load and select a model before sending a message.".to_owned());
            return;
        }
        let content = std::mem::take(&mut self.composer);
        self.run_pending = true;
        self.command(AppCommand::SendChat {
            settings: self.settings.clone(),
            content,
            agent_mode: self.agent_mode,
        });
    }

    fn provider_status(&self) -> ProviderStatus {
        if self.provider_checking {
            ProviderStatus::Checking
        } else if self.approval.is_some() {
            ProviderStatus::AwaitingApproval
        } else if self.run_pending {
            ProviderStatus::Working
        } else if !self.provider_online {
            ProviderStatus::Offline
        } else if self
            .settings
            .selected_model
            .as_ref()
            .is_some_and(|model| self.models.contains(model))
        {
            ProviderStatus::Ready
        } else {
            ProviderStatus::OnlineNoModel
        }
    }
}

#[cfg(test)]
mod tests;

use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use eframe::egui::{self, Align, Color32, Layout, RichText, ScrollArea};

use crate::{
    backend,
    core::{
        AppCommand, AppEvent, ChatMessage, MessageRole, Settings, Task, WorkspaceEntry,
        APPLICATION_NAME, DEFAULT_WINDOW_HEIGHT, DEFAULT_WINDOW_WIDTH,
    },
    widgets::{self, ProviderStatus, ACCENT, SURFACE, TEXT_MUTED},
};

const LEFT_PANEL_WIDTH: f32 = 300.0;
const COMPOSER_HEIGHT: f32 = 122.0;

pub fn run() -> eframe::Result<()> {
    eframe::run_native(
        APPLICATION_NAME,
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([DEFAULT_WINDOW_WIDTH, DEFAULT_WINDOW_HEIGHT]),
            ..Default::default()
        },
        Box::new(|context| Ok(Box::new(MoxiBoxApp::new(&context.egui_ctx)))),
    )
}

struct MoxiBoxApp {
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
}

impl MoxiBoxApp {
    fn new(context: &egui::Context) -> Self {
        widgets::install_theme(context);
        let (command_sender, command_receiver) = mpsc::channel();
        let (event_sender, event_receiver) = mpsc::channel();
        backend::start(command_receiver, event_sender);
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
                AppEvent::ConnectionChanged(connected) => self.provider_online = connected,
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
                AppEvent::Error(message) => {
                    self.streaming_id = None;
                    self.run_pending = false;
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
        if self.run_pending {
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

impl eframe::App for MoxiBoxApp {
    fn update(&mut self, context: &egui::Context, _: &mut eframe::Frame) {
        self.handle_events(context);
        if self.run_pending {
            context.request_repaint_after(Duration::from_millis(50));
        }
        egui::TopBottomPanel::top("header")
            .frame(
                egui::Frame::new()
                    .fill(SURFACE)
                    .inner_margin(egui::Margin::symmetric(12, 8)),
            )
            .show(context, |ui| {
                ui.horizontal(|ui| {
                    widgets::brand(ui, APPLICATION_NAME);
                    widgets::status_badge(ui, self.provider_status());
                    ui.separator();
                    ui.label("LM Studio URL");
                    ui.scope(|ui| {
                        ui.set_width(220.0);
                        widgets::text_field(ui, &mut self.settings.lm_studio_url, "LM Studio URL");
                    });
                    if ui.add(widgets::secondary_button("Refresh")).clicked() {
                        self.provider_online = false;
                        self.notice = Some("Checking LM Studio…".to_owned());
                        self.command(AppCommand::RefreshModels {
                            base_url: self.settings.lm_studio_url.clone(),
                        });
                    }
                    if widgets::model_picker(ui, &mut self.settings.selected_model, &self.models) {
                        self.command(AppCommand::SaveSettings(self.settings.clone()));
                    }
                    if ui.add(widgets::primary_button("Save")).clicked() {
                        self.command(AppCommand::SaveSettings(self.settings.clone()));
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if let Some(notice) = &self.notice {
                            ui.label(RichText::new(notice).color(Color32::LIGHT_YELLOW));
                        }
                    });
                });
            });
        egui::SidePanel::left("workspace")
            .resizable(true)
            .default_width(LEFT_PANEL_WIDTH)
            .show(context, |ui| {
                ui.heading("Workspace");
                widgets::text_field(ui, &mut self.settings.workspace_root, "Workspace path");
                if ui.add(widgets::primary_button("Open workspace")).clicked() {
                    self.command(AppCommand::SaveSettings(self.settings.clone()));
                    self.command(AppCommand::BrowseWorkspace {
                        root: self.settings.workspace_root.clone(),
                    });
                }
                ui.small("Agent tools stay constrained to this directory.");
                ui.separator();
                ScrollArea::vertical()
                    .id_salt("workspace_tree_scroll")
                    .max_height(300.0)
                    .show(ui, |ui| {
                        for entry in self.workspace_entries.clone() {
                            if widgets::file_row(
                                ui,
                                &entry,
                                self.preview
                                    .as_ref()
                                    .is_some_and(|(path, _)| path == &entry.path),
                            ) {
                                self.command(AppCommand::ReadPreview {
                                    root: self.settings.workspace_root.clone(),
                                    path: entry.path,
                                });
                            }
                        }
                    });
                ui.separator();
                ui.heading("Tasks");
                ScrollArea::vertical()
                    .id_salt("task_list_scroll")
                    .max_height(260.0)
                    .show(ui, |ui| {
                        for task in self.tasks.clone() {
                            if let Some(complete) = widgets::task_row(ui, &task) {
                                self.command(AppCommand::SetTaskComplete {
                                    id: task.id,
                                    complete,
                                });
                            }
                        }
                    });
                ui.horizontal(|ui| {
                    widgets::text_field(ui, &mut self.new_task, "New task");
                    if ui.add(widgets::secondary_button("Add")).clicked() {
                        let title = std::mem::take(&mut self.new_task);
                        self.command(AppCommand::AddTask(title));
                    }
                });
            });
        let mut send_from_composer = false;
        egui::TopBottomPanel::bottom("chat_composer")
            .resizable(false)
            .exact_height(COMPOSER_HEIGHT)
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(18, 22, 31))
                    .inner_margin(egui::Margin::symmetric(12, 8)),
            )
            .show(context, |ui| {
                send_from_composer =
                    widgets::composer(ui, &mut self.composer, self.agent_mode, self.run_pending)
                        .send;
            });
        if send_from_composer {
            self.send_message();
        }
        egui::CentralPanel::default().show(context, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new(if self.preview.is_some() { "FILE PREVIEW" } else { "CONVERSATION" }).small().strong().color(TEXT_MUTED));
                    ui.heading(if self.preview.is_some() { "Workspace file" } else { "Your local assistant" });
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    widgets::mode_pill(ui, &mut self.agent_mode, true, "Agent");
                    widgets::mode_pill(ui, &mut self.agent_mode, false, "Chat");
                });
            });
            if self.agent_mode { ui.label(RichText::new("Read and search are automatic · Writes and commands require approval").small().color(TEXT_MUTED)); }
            ui.add_space(4.0);
            if let Some((request_id, summary)) = self.approval.clone() {
                widgets::card().fill(Color32::from_rgb(62, 50, 28)).show(ui, |ui| { ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("⚠  {summary}")).strong().color(Color32::from_rgb(248, 205, 112)));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.add(widgets::primary_button("Approve")).clicked() { self.command(AppCommand::ResolveApproval { request_id: request_id.clone(), approved: true }); self.approval = None; }
                        if ui.add(widgets::secondary_button("Deny")).clicked() { self.command(AppCommand::ResolveApproval { request_id: request_id.clone(), approved: false }); self.approval = None; }
                    });
                });
                });
            }
            if !self.activity.is_empty() {
                egui::CollapsingHeader::new("Agent activity").show(ui, |ui| {
                    for activity in self.activity.iter().rev().take(8) {
                        ui.monospace(activity);
                    }
                });
            }
            let conversation_height = ui.available_height().max(100.0);
            ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), conversation_height), Layout::top_down(Align::Min), |ui| {
            if let Some((path, content)) = self.preview.clone() {
                widgets::card().show(ui, |ui| { ui.horizontal(|ui| { ui.label(RichText::new(&path).strong()); ui.with_layout(Layout::right_to_left(Align::Center), |ui| { if ui.add(widgets::secondary_button("← Return to chat")).clicked() { self.preview = None; } }); }); });
                ui.add_space(8.0);
                ScrollArea::vertical().id_salt("file_preview_scroll").auto_shrink([false, false]).show(ui, |ui| {
                    widgets::card().fill(Color32::from_rgb(15, 19, 27)).show(ui, |ui| { ui.monospace(&content); });
                });
            } else {
                ScrollArea::vertical().id_salt("chat_messages_scroll").auto_shrink([false, false]).stick_to_bottom(true).show(ui, |ui| {
                    if self.messages.is_empty() {
                        ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), (conversation_height - 24.0).max(80.0)), Layout::centered_and_justified(egui::Direction::TopDown), |ui| {
                            ui.vertical_centered(|ui| { ui.label(RichText::new("◈").size(38.0).color(ACCENT)); ui.heading("What are we building?"); ui.label(RichText::new("Ask a question or switch to Agent mode to work on your code.").color(TEXT_MUTED)); });
                        });
                    }
                    for message in &self.messages {
                        widgets::message_bubble(ui, message, self.streaming_id == Some(message.id));
                        ui.add_space(12.0);
                    }
                });
            }
            });
        });
    }
}

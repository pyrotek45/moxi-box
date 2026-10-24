use super::*;

impl MoxiBoxApp {
    pub(super) fn sidebar(&mut self, context: &egui::Context) {
        egui::SidePanel::left("workspace")
            .frame(egui::Frame::new().fill(SURFACE).inner_margin(16))
            .resizable(true)
            .default_width(260.0)
            .width_range(200.0..=320.0)
            .show(context, |ui| {
                ScrollArea::vertical()
                    .id_salt("sidebar")
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                    .show(ui, |ui| {
                        ui.label(RichText::new("WORKSPACE").small().color(TEXT_MUTED));
                        ui.add_enabled_ui(
                            !self.run_pending
                                && !self.workspace_pending
                                && self.folder_picker.is_none(),
                            |ui| {
                                let mut selected = self.settings.workspace_root.clone();
                                let name = std::path::Path::new(&selected)
                                    .file_name()
                                    .and_then(|name| name.to_str())
                                    .unwrap_or("Choose workspace");
                                egui::ComboBox::from_id_salt("workspace_picker")
                                    .width(ui.available_width())
                                    .selected_text(name)
                                    .truncate()
                                    .show_ui(ui, |ui| {
                                        for root in &self.settings.workspaces {
                                            let name = std::path::Path::new(root)
                                                .file_name()
                                                .and_then(|name| name.to_str())
                                                .unwrap_or(root);
                                            ui.selectable_value(&mut selected, root.clone(), name)
                                                .on_hover_text(root);
                                        }
                                    });
                                if selected != self.settings.workspace_root {
                                    self.open_workspace(selected, false);
                                }
                                ui.spacing_mut().button_padding = egui::vec2(8.0, 6.0);
                                ui.spacing_mut().item_spacing.x = 6.0;
                                ui.horizontal_wrapped(|ui| {
                                    for (label, dialog) in [
                                        ("New", crate::app::workspaces::WorkspaceDialog::New),
                                        ("Open", crate::app::workspaces::WorkspaceDialog::Open),
                                        ("Reset", crate::app::workspaces::WorkspaceDialog::Reset),
                                        ("Delete", crate::app::workspaces::WorkspaceDialog::Remove),
                                    ] {
                                        if ui
                                            .add_enabled(
                                                !matches!(label, "Delete" | "Reset")
                                                    || !self.settings.workspace_root.is_empty(),
                                                widgets::secondary_button(label),
                                            )
                                            .clicked()
                                        {
                                            self.show_workspace_dialog(dialog, context);
                                        }
                                    }
                                });
                            },
                        );
                        if !self.settings.workspace_root.is_empty() {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&self.settings.workspace_root)
                                        .small()
                                        .color(TEXT_MUTED),
                                )
                                .truncate(),
                            )
                            .on_hover_text(&self.settings.workspace_root);
                        }
                        ui.add_space(12.0);
                        ui.label(RichText::new("CONVERSATIONS").small().color(TEXT_MUTED));
                        ui.add_enabled_ui(
                            !self.run_pending
                                && !self.workspace_pending
                                && !self.conversation_pending
                                && !self.settings.workspace_root.is_empty(),
                            |ui| {
                                let mut selected = self.conversation_id;
                                let title = self
                                    .conversations
                                    .iter()
                                    .find(|c| Some(c.id) == selected)
                                    .map(|c| c.title.as_str())
                                    .unwrap_or("New conversation");
                                egui::ComboBox::from_id_salt("conversation_picker")
                                    .width(ui.available_width())
                                    .selected_text(title)
                                    .truncate()
                                    .show_ui(ui, |ui| {
                                        for conversation in &self.conversations {
                                            ui.selectable_value(
                                                &mut selected,
                                                Some(conversation.id),
                                                &conversation.title,
                                            );
                                        }
                                    });
                                if selected != self.conversation_id {
                                    if let Some(id) = selected {
                                        self.conversation_pending = true;
                                        self.command(AppCommand::SelectConversation {
                                            workspace: self.settings.workspace_root.clone(),
                                            id,
                                        });
                                    }
                                }
                                ui.horizontal_wrapped(|ui| {
                                    if ui.add(widgets::secondary_button("New chat")).clicked() {
                                        self.conversation_pending = true;
                                        self.command(AppCommand::NewConversation {
                                            workspace: self.settings.workspace_root.clone(),
                                        });
                                    }
                                    if ui.add(widgets::secondary_button("Memory")).clicked() {
                                        self.memory_draft = Some(self.memory.clone());
                                    }
                                });
                            },
                        );
                        ui.add_space(8.0);
                        ScrollArea::vertical()
                            .id_salt("files")
                            .scroll_bar_visibility(
                                egui::scroll_area::ScrollBarVisibility::AlwaysHidden,
                            )
                            .max_height(160.0)
                            .show(ui, |ui| {
                                for entry in &self.workspace_entries {
                                    if widgets::file_row(
                                        ui,
                                        entry,
                                        self.preview
                                            .as_ref()
                                            .is_some_and(|(path, _)| path == &entry.path),
                                    ) {
                                        self.command(AppCommand::ReadPreview {
                                            root: self.settings.workspace_root.clone(),
                                            path: entry.path.clone(),
                                        });
                                    }
                                }
                            });
                        ui.add_space(12.0);
                        ui.label(RichText::new("TASKS").small().color(TEXT_MUTED));
                        for task in &self.tasks {
                            if let Some(complete) = widgets::task_row(ui, task) {
                                self.command(AppCommand::SetTaskComplete {
                                    id: task.id,
                                    complete,
                                });
                            }
                        }
                        ui.horizontal(|ui| {
                            let input_width = (ui.available_width() - 68.0).max(40.0);
                            ui.allocate_ui(egui::vec2(input_width, 34.0), |ui| {
                                widgets::text_field(ui, &mut self.new_task, "New task");
                            });
                            if ui
                                .add_enabled(
                                    !self.new_task.trim().is_empty(),
                                    widgets::secondary_button("Add"),
                                )
                                .clicked()
                            {
                                let title = std::mem::take(&mut self.new_task);
                                self.command(AppCommand::AddTask(title));
                            }
                        });
                    });
            });
    }
}

use super::*;
use crate::app::workspaces::WorkspaceDialog;

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
                        let idle = self.idle();
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("WORKSPACES").small().color(TEXT_MUTED));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui
                                    .add_enabled_ui(idle, widgets::plus_button)
                                    .inner
                                    .on_hover_text("New workspace")
                                    .clicked()
                                {
                                    self.name_workspace(None);
                                }
                            });
                        });
                        ScrollArea::vertical()
                            .id_salt("workspaces")
                            .scroll_bar_visibility(
                                egui::scroll_area::ScrollBarVisibility::AlwaysHidden,
                            )
                            .max_height(200.0)
                            .show(ui, |ui| {
                                ui.spacing_mut().item_spacing.y = 0.0;
                                for workspace in self.workspaces.clone() {
                                    let selected = self.workspace_id == Some(workspace.id);
                                    match widgets::workspace_row(ui, &workspace, selected, idle) {
                                        Some(widgets::RowAction::Select) => {
                                            self.select_workspace(workspace.id)
                                        }
                                        Some(widgets::RowAction::Rename) => {
                                            self.name_workspace(Some(workspace.id))
                                        }
                                        Some(widgets::RowAction::Delete) => {
                                            self.workspace_name.clear();
                                            self.workspace_dialog =
                                                Some(WorkspaceDialog::Delete(workspace.id));
                                        }
                                        None => {}
                                    }
                                }
                            });
                        let folder = self.settings.workspace_root.clone();
                        if !folder.is_empty() {
                            ui.add_space(12.0);
                            ui.label(RichText::new("FILES").small().color(TEXT_MUTED));
                            ScrollArea::vertical()
                                .id_salt("files")
                                .scroll_bar_visibility(
                                    egui::scroll_area::ScrollBarVisibility::AlwaysHidden,
                                )
                                .max_height(240.0)
                                .show(ui, |ui| {
                                    ui.spacing_mut().item_spacing.y = 0.0;
                                    // Entries arrive depth-first; skip descendants of collapsed folders.
                                    let mut hidden_below = usize::MAX;
                                    for entry in &self.workspace_entries {
                                        if entry.depth > hidden_below {
                                            continue;
                                        }
                                        hidden_below = usize::MAX;
                                        let expanded = self.expanded_folders.contains(&entry.path);
                                        let selected = self
                                            .preview
                                            .as_ref()
                                            .is_some_and(|(path, _)| path == &entry.path);
                                        let clicked =
                                            widgets::file_row(ui, entry, selected, expanded);
                                        if entry.is_directory {
                                            if clicked && !self.expanded_folders.remove(&entry.path)
                                            {
                                                self.expanded_folders.insert(entry.path.clone());
                                            }
                                            if !self.expanded_folders.contains(&entry.path) {
                                                hidden_below = entry.depth;
                                            }
                                        } else if clicked {
                                            self.command(AppCommand::ReadPreview {
                                                root: self.settings.workspace_root.clone(),
                                                path: entry.path.clone(),
                                            });
                                        }
                                    }
                                    if self.workspace_entries.is_empty() {
                                        ui.label(
                                            RichText::new("Empty folder").small().color(TEXT_MUTED),
                                        );
                                    }
                                });
                        }
                        ui.add_space(12.0);
                        let completed = self
                            .tasks
                            .iter()
                            .filter(|task| task.status == crate::domain::TaskStatus::Completed)
                            .count();
                        ui.label(
                            RichText::new(format!("TASKS  {completed}/{}", self.tasks.len()))
                                .small()
                                .color(TEXT_MUTED),
                        );
                        ui.add_enabled_ui(idle, |ui| {
                            ScrollArea::vertical()
                                .id_salt("tasks")
                                .scroll_bar_visibility(
                                    egui::scroll_area::ScrollBarVisibility::AlwaysHidden,
                                )
                                .max_height(176.0)
                                .show(ui, |ui| {
                                    ui.spacing_mut().item_spacing.y = 0.0;
                                    for task in &self.tasks {
                                        if let Some(complete) = widgets::task_row(ui, task) {
                                            self.command(AppCommand::SetTaskComplete {
                                                id: task.id,
                                                complete,
                                            });
                                        }
                                    }
                                });
                            ui.add_space(4.0);
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
            });
    }
}

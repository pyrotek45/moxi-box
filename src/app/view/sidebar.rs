use super::*;
use crate::app::workspaces::{FolderPick, WorkspaceDialog};

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
                        self.workspace_section(ui, context);
                        if self.workspace_id.is_some() {
                            ui.add_space(12.0);
                            self.files_section(ui, context);
                            ui.add_space(12.0);
                            self.tasks_section(ui);
                            ui.add_space(12.0);
                            self.shortcuts_section(ui);
                        }
                    });
            });
    }

    fn workspace_section(&mut self, ui: &mut egui::Ui, context: &egui::Context) {
        let idle = self.idle();
        let has_folder = !self.settings.workspace_root.is_empty();
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
                if ui
                    .add_enabled_ui(idle && self.workspace_id.is_some(), widgets::folder_button)
                    .inner
                    .on_hover_text(if has_folder {
                        "Change this workspace's folder"
                    } else {
                        "Add a folder to this workspace"
                    })
                    .clicked()
                {
                    self.pick_folder(FolderPick::Attach, context);
                }
            });
        });
        ScrollArea::vertical()
            .id_salt("workspaces")
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .max_height(200.0)
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for workspace in self.workspaces.clone() {
                    let selected = self.workspace_id == Some(workspace.id);
                    match widgets::workspace_row(ui, &workspace, selected, idle) {
                        Some(widgets::RowAction::Select) => self.select_workspace(workspace.id),
                        Some(widgets::RowAction::Rename) => self.name_workspace(Some(workspace.id)),
                        Some(widgets::RowAction::Delete) => {
                            self.workspace_name.clear();
                            self.workspace_dialog = Some(WorkspaceDialog::Delete(workspace.id));
                        }
                        None => {}
                    }
                }
            });
    }

    fn files_section(&mut self, ui: &mut egui::Ui, context: &egui::Context) {
        let folder = self.settings.workspace_root.clone();
        if folder.is_empty() {
            ui.label(RichText::new("FILES").small().color(TEXT_MUTED));
            ui.label(
                RichText::new("No folder attached. Add one so the agent can read and edit files.")
                    .small()
                    .color(TEXT_MUTED),
            );
            if ui
                .add_enabled(self.idle(), widgets::secondary_button("Add folder"))
                .clicked()
            {
                self.pick_folder(FolderPick::Attach, context);
            }
            return;
        }
        let name = std::path::Path::new(&folder)
            .file_name()
            .map(|name| name.to_string_lossy().to_uppercase())
            .unwrap_or_else(|| "FILES".to_owned());
        ui.add(egui::Label::new(RichText::new(name).small().color(TEXT_MUTED)).truncate())
            .on_hover_text(&folder);
        ScrollArea::vertical()
            .id_salt("files")
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .max_height(240.0)
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                // Entries arrive depth-first; skip descendants of collapsed folders.
                let mut hidden_below = usize::MAX;
                let mut rescan = false;
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
                    let clicked = widgets::file_row(ui, entry, selected, expanded);
                    if entry.is_directory {
                        if clicked {
                            if !self.expanded_folders.remove(&entry.path) {
                                self.expanded_folders.insert(entry.path.clone());
                            }
                            rescan = true;
                        }
                        if !self.expanded_folders.contains(&entry.path) {
                            hidden_below = entry.depth;
                        }
                    } else if clicked {
                        self.preview_requested = Some(entry.path.clone());
                        self.command(AppCommand::ReadPreview {
                            root: self.settings.workspace_root.clone(),
                            path: entry.path.clone(),
                        });
                    }
                }
                // Opening a folder lists its contents right away instead of on the next tick.
                if rescan {
                    self.rescan_tree_now();
                }
                if self.workspace_entries.is_empty() {
                    ui.label(RichText::new("Empty folder").small().color(TEXT_MUTED));
                }
            });
    }

    /// Memory and goal stay one compact row each; their contents open in dialogs.
    fn shortcuts_section(&mut self, ui: &mut egui::Ui) {
        ui.spacing_mut().item_spacing.y = 2.0;
        let notes = memory_notes(&self.memory).len();
        let detail = match notes {
            0 => "empty".to_owned(),
            1 => "1 note".to_owned(),
            n => format!("{n} notes"),
        };
        if widgets::nav_row(ui, "Memory", &detail)
            .on_hover_text("Facts and lessons the agent keeps for this workspace")
            .clicked()
        {
            self.open_memory();
        }
        let goal = if self.goal.is_empty() {
            "not set"
        } else {
            "set"
        };
        if widgets::nav_row(ui, "Goal", goal)
            .on_hover_text(if self.goal.is_empty() {
                "Set a long-running goal for the agent"
            } else {
                self.goal.as_str()
            })
            .clicked()
        {
            self.goal_draft = Some(self.goal.clone());
        }
        ui.spacing_mut().item_spacing.y = 8.0;
    }

    fn tasks_section(&mut self, ui: &mut egui::Ui) {
        let completed = self
            .tasks
            .iter()
            .filter(|task| task.status == TaskStatus::Completed)
            .count();
        // Task edits stay available during a run; the agent picks them up before its next step.
        let editable = !self.switching && !self.worker_lost;
        let actions: &[&str] = match (completed > 0, self.tasks.is_empty()) {
            (_, true) => &[],
            (true, _) => &["Clear done", "Clear all"],
            (false, _) => &["Clear all"],
        };
        let title = format!("TASKS  {completed}/{}", self.tasks.len());
        let clicked = ui
            .add_enabled_ui(editable, |ui| widgets::section_header(ui, &title, actions))
            .inner;
        if let Some(index) = clicked {
            self.command(AppCommand::ClearTasks {
                completed_only: actions[index] == "Clear done",
            });
        }
        ui.add_enabled_ui(editable, |ui| {
            ScrollArea::vertical()
                .id_salt("tasks")
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                .max_height(176.0)
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for task in &self.tasks {
                        match widgets::task_row(ui, task) {
                            Some(widgets::TaskAction::Toggle(complete)) => {
                                self.command(AppCommand::SetTaskComplete {
                                    id: task.id,
                                    complete,
                                })
                            }
                            Some(widgets::TaskAction::Delete) => {
                                self.command(AppCommand::DeleteTask(task.id))
                            }
                            None => {}
                        }
                    }
                });
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let input_width = (ui.available_width() - 68.0).max(40.0);
                let field = ui
                    .allocate_ui(egui::vec2(input_width, 34.0), |ui| {
                        widgets::text_field(ui, &mut self.new_task, "New task")
                    })
                    .inner;
                let submit =
                    field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
                let add = ui
                    .add_enabled(
                        !self.new_task.trim().is_empty(),
                        widgets::secondary_button("Add"),
                    )
                    .clicked();
                if (add || submit) && !self.new_task.trim().is_empty() {
                    let title = std::mem::take(&mut self.new_task);
                    self.command(AppCommand::AddTask(title));
                }
            });
        });
    }
}

/// Memory is stored as text; each nonempty line is shown as one note, without list markers.
pub(super) fn memory_notes(memory: &str) -> Vec<&str> {
    memory
        .lines()
        .map(|line| line.trim().trim_start_matches(['-', '*']).trim())
        .filter(|line| !line.is_empty())
        .collect()
}

use super::*;

impl MoxiBoxApp {
    pub(super) fn workspace_dialog(&mut self, context: &egui::Context, width: f32, height: f32) {
        let Some(dialog) = self.workspace_dialog else {
            return;
        };
        let mut accept = false;
        let mut cancel = false;
        let folder_name = std::path::Path::new(&self.settings.workspace_root)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("")
            .to_owned();
        let workspace_name = match dialog {
            WorkspaceDialog::Delete(id) => self
                .workspaces
                .iter()
                .find(|workspace| workspace.id == id)
                .map(|workspace| workspace.name.clone())
                .unwrap_or_default(),
            _ => String::new(),
        };
        let response = egui::Modal::new(egui::Id::new("workspace_dialog")).frame(widgets::card().inner_margin(24)).show(context, |ui| {
            ui.set_width(width);
            ui.heading(match dialog {
                WorkspaceDialog::NewFolder => "New folder",
                WorkspaceDialog::Name(None) => "New workspace",
                WorkspaceDialog::Name(Some(_)) => "Rename workspace",
                WorkspaceDialog::Delete(_) => "Delete workspace?",
                WorkspaceDialog::Reset => "Reset folder?",
            });
            ScrollArea::vertical().id_salt("workspace_form").max_height(height - 75.0).show(ui, |ui| {
                match dialog {
                    WorkspaceDialog::Delete(_) => {
                        ui.add(egui::Label::new(RichText::new(&workspace_name).strong()).wrap());
                        ui.label("Its conversation, memory, and tasks are deleted. Files in its folder stay on disk.");
                    }
                    WorkspaceDialog::Reset => {
                        ui.add(egui::Label::new(&self.settings.workspace_root).wrap());
                        ui.label("Move this folder to a sibling backup and recreate it empty. The conversation is kept.");
                        ui.label(format!("Type {folder_name} to confirm:"));
                        let field = widgets::text_field(ui, &mut self.workspace_name, "Folder name");
                        widgets::focus_if_idle(ui, &field);
                    }
                    WorkspaceDialog::Name(id) => {
                        ui.label(if id.is_none() { "Name it now, or leave blank to name it from your first message." } else { "Workspace name" });
                        let field = widgets::text_field(ui, &mut self.workspace_name, "Workspace name");
                        widgets::focus_if_idle(ui, &field);
                    }
                    WorkspaceDialog::NewFolder => {
                        ui.label("Create in:");
                        ui.add(egui::Label::new(&self.workspace_path).wrap());
                        ui.add_space(12.0);
                        ui.label("Folder name");
                        let field = widgets::text_field(ui, &mut self.workspace_name, "my-project");
                        widgets::focus_if_idle(ui, &field);
                    }
                }
                if let Some(error) = &self.workspace_error { ui.add(egui::Label::new(RichText::new(error).color(Color32::LIGHT_RED)).wrap()); }
            });
            ui.add_space(10.0);
            let allowed = !self.workspace_pending
                && match dialog {
                    WorkspaceDialog::Reset => !folder_name.is_empty() && self.workspace_name == folder_name,
                    WorkspaceDialog::NewFolder => !self.workspace_name.trim().is_empty(),
                    WorkspaceDialog::Delete(_) | WorkspaceDialog::Name(None) => true,
                    WorkspaceDialog::Name(Some(_)) => !self.workspace_name.trim().is_empty(),
                };
            ui.add_enabled_ui(!self.workspace_pending, |ui| {
                ui.horizontal(|ui| {
                    let label = match dialog {
                        WorkspaceDialog::NewFolder => "Create and attach",
                        WorkspaceDialog::Name(None) => "Create",
                        WorkspaceDialog::Name(Some(_)) => "Rename",
                        WorkspaceDialog::Delete(_) => "Delete workspace",
                        WorkspaceDialog::Reset => "Archive and reset",
                    };
                    let button = if matches!(dialog, WorkspaceDialog::NewFolder | WorkspaceDialog::Name(_)) { widgets::primary_button(label) } else { widgets::danger_button(label) };
                    accept = ui.add_enabled(allowed, button).clicked();
                    cancel = ui.add(widgets::secondary_button("Cancel")).clicked();
                });
            });
            accept |= allowed && widgets::enter_pressed(ui);
            if self.workspace_pending { ui.label("Working…"); }
        });
        if accept {
            match dialog {
                WorkspaceDialog::Name(id) => {
                    let name = self.workspace_name.trim().to_owned();
                    self.workspace_dialog = None;
                    self.switching = true;
                    self.command(match id {
                        Some(id) => AppCommand::RenameWorkspace { id, name },
                        None => AppCommand::NewWorkspace(name),
                    });
                }
                WorkspaceDialog::Delete(id) => {
                    self.switching = true;
                    self.workspace_dialog = None;
                    self.command(AppCommand::DeleteWorkspace(id));
                }
                WorkspaceDialog::Reset => {
                    self.workspace_pending = true;
                    self.command(AppCommand::ResetWorkspace {
                        path: self.settings.workspace_root.clone(),
                    });
                }
                WorkspaceDialog::NewFolder => {
                    let name = self.workspace_name.trim();
                    if name == "." || name == ".." || name.contains(['/', '\\']) {
                        self.workspace_error = Some("Enter a single new folder name.".to_owned());
                    } else {
                        let path = std::path::Path::new(&self.workspace_path)
                            .join(name)
                            .to_string_lossy()
                            .into_owned();
                        self.attach_folder(path, true);
                    }
                }
            }
        } else if !self.workspace_pending && (cancel || response.should_close()) {
            self.workspace_dialog = None;
        }
    }
}

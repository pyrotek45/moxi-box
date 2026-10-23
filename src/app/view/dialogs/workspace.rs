use super::*;

impl MoxiBoxApp {
    pub(super) fn workspace_dialog(&mut self, context: &egui::Context, width: f32, height: f32) {
        let Some(dialog) = self.workspace_dialog else {
            return;
        };
        let mut accept = false;
        let mut cancel = false;
        let mut navigate = None;
        let expected = std::path::Path::new(&self.settings.workspace_root)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("")
            .to_owned();
        let response = egui::Modal::new(egui::Id::new("workspace_dialog")).frame(widgets::card().inner_margin(24)).show(context, |ui| {
            ui.set_width(width);
            ui.heading(match dialog { WorkspaceDialog::New => "New workspace", WorkspaceDialog::Open => "Open workspace", WorkspaceDialog::Remove => "Delete workspace?", WorkspaceDialog::Reset => "Reset workspace?" });
            ScrollArea::vertical().id_salt("workspace_form").max_height(height - 75.0).show(ui, |ui| {
                match dialog {
                    WorkspaceDialog::Remove => {
                        ui.add(egui::Label::new(&self.settings.workspace_root).wrap());
                        ui.label("Remove this workspace from your saved list. Its files will stay on disk.");
                    }
                    WorkspaceDialog::Reset => {
                        ui.add(egui::Label::new(&self.settings.workspace_root).wrap());
                        ui.label("Archive this folder to a sibling backup, recreate it empty, and start a new chat. Old conversations and memory remain saved.");
                        ui.label(format!("Type {expected} to confirm:"));
                        widgets::text_field(ui,&mut self.workspace_name,"Workspace folder name");
                    }
                    WorkspaceDialog::Open | WorkspaceDialog::New => {
                        ui.label(if dialog == WorkspaceDialog::New { "Choose the parent folder" } else { "Choose a folder" });
                        widgets::text_field(ui,&mut self.workspace_path,"Folder path");
                        ui.horizontal(|ui| {
                            if ui.add_enabled(!self.folder_loading,widgets::secondary_button("Go")).clicked() { navigate = Some(self.workspace_path.clone()); }
                            if ui.add_enabled(!self.folder_loading,widgets::secondary_button("Up")).clicked() {
                                navigate = std::path::Path::new(&self.workspace_path).parent().map(|path| path.to_string_lossy().into_owned());
                            }
                        });
                        if self.folder_loading { ui.label("Loading folders…"); }
                        ScrollArea::vertical().id_salt("folder_list").max_height(140.0).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            if self.folder_entries.is_empty() && !self.folder_loading { ui.label(RichText::new("No subfolders").small().color(TEXT_MUTED)); }
                            for path in &self.folder_entries {
                                let name = std::path::Path::new(path).file_name().and_then(|name| name.to_str()).unwrap_or(path);
                                if ui.add(egui::Button::new(name).frame(false).truncate()).on_hover_text(path).clicked() { navigate = Some(path.clone()); }
                            }
                        });
                        if dialog == WorkspaceDialog::New {
                            ui.label("New folder name");
                            widgets::text_field(ui,&mut self.workspace_name,"my-project");
                        }
                    }
                }
                if let Some(error) = &self.workspace_error { ui.add(egui::Label::new(RichText::new(error).color(Color32::LIGHT_RED)).wrap()); }
            });
            ui.add_space(10.0);
            ui.add_enabled_ui(!self.workspace_pending && !self.folder_loading, |ui| {
                ui.horizontal(|ui| {
                    let allowed = match dialog { WorkspaceDialog::Reset => !expected.is_empty() && self.workspace_name == expected, WorkspaceDialog::New => !self.workspace_name.trim().is_empty(), _ => true };
                    accept = ui.add_enabled(allowed, widgets::primary_button(match dialog { WorkspaceDialog::New => "Create", WorkspaceDialog::Open => "Open folder", WorkspaceDialog::Remove => "Remove from list", WorkspaceDialog::Reset => "Archive and reset" })).clicked();
                    cancel = ui.add(widgets::secondary_button("Cancel")).clicked();
                });
            });
            if self.workspace_pending { ui.label("Opening workspace…"); }
        });
        if let Some(path) = navigate {
            self.browse_folders(path);
        }
        if accept {
            match dialog {
                WorkspaceDialog::Remove => self.remove_workspace(),
                WorkspaceDialog::Reset => {
                    self.workspace_pending = true;
                    self.conversation_pending = true;
                    self.command(AppCommand::ResetWorkspace {
                        path: self.settings.workspace_root.clone(),
                    });
                }
                WorkspaceDialog::Open => self.open_workspace(self.workspace_path.clone(), false),
                WorkspaceDialog::New => {
                    let name = self.workspace_name.trim();
                    if name == "." || name == ".." || name.contains(['/', '\\']) {
                        self.workspace_error = Some("Enter a single new folder name.".to_owned());
                    } else {
                        self.open_workspace(
                            std::path::Path::new(&self.workspace_path)
                                .join(name)
                                .to_string_lossy()
                                .into_owned(),
                            true,
                        );
                    }
                }
            }
        } else if !self.workspace_pending && (cancel || response.should_close()) {
            self.workspace_dialog = None;
        }
    }
}

use super::*;

impl MoxiBoxApp {
    pub(super) fn settings_dialog(&mut self, context: &egui::Context, width: f32, height: f32) {
        if !self.show_settings {
            return;
        }
        let mut close = false;
        let mut open_provider = false;
        let response = egui::Modal::new(egui::Id::new("settings"))
            .frame(widgets::card().inner_margin(24))
            .show(context, |ui| {
                ui.set_width(width);
                ui.heading("Settings");
                ui.add_space(8.0);
                ScrollArea::vertical().id_salt("settings_body").max_height(height - 60.0).show(ui, |ui| {
                    egui::CollapsingHeader::new("Workspace").default_open(true).show(ui, |ui| {
                    ui.add(egui::Label::new(if self.settings.workspace_root.is_empty() { "No folder attached" } else { &self.settings.workspace_root }).wrap());
                    ui.add_enabled_ui(self.idle(), |ui| {
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 2.0;
                            if settings_action(ui, "Attach / change").clicked() { self.pick_folder(crate::app::workspaces::FolderPick::Attach, context); close = true; }
                            if settings_action(ui, "New folder").clicked() { self.pick_folder(crate::app::workspaces::FolderPick::NewFolderParent, context); close = true; }
                            if !self.settings.workspace_root.is_empty() {
                                if settings_action(ui, "Detach").clicked() { self.attach_folder(String::new(), false); }
                                if settings_action(ui, "Reset folder").clicked() { self.workspace_name.clear(); self.workspace_error = None; self.workspace_dialog = Some(crate::app::workspaces::WorkspaceDialog::Reset); close = true; }
                            }
                            if settings_action(ui, "Workspace memory").clicked() { self.memory_draft = Some(self.memory.clone()); close = true; }
                        });
                    });
                    ui.add_space(12.0);
                    });
                    egui::CollapsingHeader::new("Agent behavior").default_open(false).show(ui, |ui| {
                    let mut auto = self.auto_approve.load(std::sync::atomic::Ordering::Acquire);
                    if ui.checkbox(&mut auto, "Auto-approve edits, writes, deletions, commands, and memory notes").changed() {
                        self.set_auto_approve(auto);
                    }
                    ui.label(RichText::new("Off by default and reset at launch. Commands run with your permissions; the workspace folder is not a sandbox.").small().color(TEXT_MUTED));
                    ui.add_space(8.0);
                    if ui.checkbox(&mut self.settings.web_access, "Web access: search and read pages when unsure").changed() {
                        self.command(AppCommand::SaveSettings(self.settings.clone()));
                    }
                    ui.label(RichText::new("Searches go to DuckDuckGo. Pages are fetched from this computer.").small().color(TEXT_MUTED));
                    ui.add_space(12.0);
                    });
                    egui::CollapsingHeader::new("Model connection").default_open(false).show(ui, |ui| {
                    ui.add(egui::Label::new(format!("{} · {}", self.settings.provider_name, self.settings.lm_studio_url)).truncate());
                    open_provider = ui.add_enabled_ui(!self.run_pending, |ui| settings_action(ui, "Provider and context budget")).inner.clicked();
                    ui.add_space(12.0);
                    });
                    egui::CollapsingHeader::new("Tool reference").default_open(false).show(ui, |ui| {
                    for (name, help) in [
                        ("list_files / read_file / search_content / grep / glob", "Explore the workspace. Automatic."),
                        ("edit_file / write_file", "Edit existing text or create/replace a file. edit_file can replace one match or every match. Needs approval."),
                        ("delete_path", "Permanently delete a file or folder. Nonempty folders require recursive=true. Needs approval."),
                        ("run_command", "Build, test, git, or run Python/scripts for complex edits. Needs approval."),
                        ("list_tasks / update_tasks", "Plan multi-step work in the sidebar task list. Automatic."),
                        ("remember", "Save a project note to workspace memory. Needs approval."),
                        ("web_search / fetch_url", "Research online when Web access is on. Automatic."),
                    ] {
                        ui.label(RichText::new(name).monospace().size(12.0));
                        ui.label(RichText::new(help).small().color(TEXT_MUTED));
                    }
                    ui.label(RichText::new("Send while Moxi works to queue a message; the agent reads it at its next step. Stop cancels the run and the queue.").small().color(TEXT_MUTED));
                    ui.add_space(12.0);
                    });
                    egui::CollapsingHeader::new("Local data").default_open(false).show(ui, |ui| {
                    let location = crate::storage::data_location();
                    ui.add(egui::Label::new(RichText::new(&location).small()).truncate()).on_hover_text(&location);
                    ui.label(RichText::new("Workspaces, memory, tasks, and crash.log live here.").small().color(TEXT_MUTED));
                    });
                });
                ui.add_space(12.0);
                close |= ui.add(widgets::primary_button("Done")).clicked() || widgets::enter_pressed(ui);
            });
        if open_provider {
            self.provider_draft = Some(self.settings.clone());
            self.provider_form_error = None;
        }
        if close || open_provider || response.should_close() {
            self.show_settings = false;
        }
    }
}

fn settings_action(ui: &mut egui::Ui, label: &str) -> egui::Response {
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), 24.0),
        Layout::left_to_right(Align::Center),
        |ui| ui.selectable_label(false, RichText::new(label).size(14.0)),
    )
    .inner
}

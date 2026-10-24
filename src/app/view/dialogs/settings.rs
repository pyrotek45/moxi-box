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
                    ui.label(RichText::new("AGENT").small().color(TEXT_MUTED));
                    let mut auto = self.auto_approve.load(std::sync::atomic::Ordering::Acquire);
                    if ui.checkbox(&mut auto, "Auto-approve edits, writes, commands, and memory notes").changed() {
                        self.set_auto_approve(auto);
                    }
                    ui.label(RichText::new("Off by default and reset at launch. Commands run with your permissions; the workspace folder is not a sandbox.").small().color(TEXT_MUTED));
                    ui.add_space(12.0);
                    ui.label(RichText::new("MODEL PROVIDER").small().color(TEXT_MUTED));
                    ui.add(egui::Label::new(format!("{} · {}", self.settings.provider_name, self.settings.lm_studio_url)).truncate());
                    open_provider = ui.add_enabled(!self.run_pending, widgets::secondary_button("Provider and context budget…")).clicked();
                    ui.add_space(12.0);
                    ui.label(RichText::new("AGENT TOOLS").small().color(TEXT_MUTED));
                    for (name, help) in [
                        ("list_files / read_file / search_content", "Explore the workspace. Automatic."),
                        ("edit_file / write_file", "Change or create files. Shows a diff; needs approval."),
                        ("run_command", "Build, test, git, move or delete files. Needs approval."),
                        ("list_tasks / update_tasks", "Plan multi-step work in the sidebar task list. Automatic."),
                        ("remember", "Save a project note to workspace memory. Needs approval."),
                    ] {
                        ui.label(RichText::new(name).monospace().size(12.0));
                        ui.label(RichText::new(help).small().color(TEXT_MUTED));
                    }
                    ui.label(RichText::new("Send while Moxi works to queue a message; the agent reads it at its next step. Stop cancels the run and the queue.").small().color(TEXT_MUTED));
                    ui.add_space(12.0);
                    ui.label(RichText::new("DATA").small().color(TEXT_MUTED));
                    let location = crate::storage::data_location();
                    ui.add(egui::Label::new(RichText::new(&location).small()).truncate()).on_hover_text(&location);
                    ui.label(RichText::new("Conversations, memory, tasks, and crash.log live here.").small().color(TEXT_MUTED));
                });
                ui.add_space(12.0);
                close = ui.add(widgets::primary_button("Done")).clicked();
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

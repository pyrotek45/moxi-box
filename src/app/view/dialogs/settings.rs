use super::*;
use crate::app::workspaces::{FolderPick, WorkspaceDialog};

const TABS: [&str; 4] = ["Workspace", "Agent", "General", "Help"];

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
                ui.add_space(4.0);
                egui::Frame::new()
                    .fill(Color32::from_gray(38))
                    .corner_radius(10.0)
                    .inner_margin(3)
                    .show(ui, |ui| widgets::tab_bar(ui, &mut self.settings_tab, &TABS));
                ui.add_space(8.0);
                ScrollArea::vertical()
                    .id_salt(("settings_body", self.settings_tab))
                    .max_height((height - 120.0).max(80.0))
                    .show(ui, |ui| match self.settings_tab {
                        0 => close |= self.workspace_settings(ui, context),
                        1 => self.agent_settings(ui),
                        2 => open_provider = self.model_settings(ui),
                        _ => help_settings(ui),
                    });
                ui.add_space(12.0);
                close |=
                    ui.add(widgets::primary_button("Done")).clicked() || widgets::enter_pressed(ui);
            });
        if open_provider {
            self.provider_draft = Some(self.settings.clone());
            self.provider_form_error = None;
        }
        if close || open_provider || response.should_close() {
            self.show_settings = false;
        }
    }

    /// Returns true when an action opened another dialog, so Settings should close.
    fn workspace_settings(&mut self, ui: &mut egui::Ui, context: &egui::Context) -> bool {
        let mut close = false;
        let attached = !self.settings.workspace_root.is_empty();
        heading(ui, "Folder");
        let folder = if attached {
            self.settings.workspace_root.as_str()
        } else {
            "No folder attached"
        };
        ui.add(egui::Label::new(RichText::new(folder).monospace().size(12.5)).wrap());
        hint(ui, "Attach selects one local folder for this workspace. Files are not uploaded or copied; file tools can only reach inside that folder.");
        ui.add_enabled_ui(self.idle(), |ui| {
            ui.horizontal_wrapped(|ui| {
                let label = if attached {
                    "Change folder"
                } else {
                    "Attach folder"
                };
                if ui.add(widgets::secondary_button(label)).clicked() {
                    self.pick_folder(FolderPick::Attach, context);
                    close = true;
                }
                if ui.add(widgets::secondary_button("New folder")).clicked() {
                    self.pick_folder(FolderPick::NewFolderParent, context);
                    close = true;
                }
                if attached
                    && ui
                        .add(widgets::secondary_button("Detach"))
                        .on_hover_text("Unlink the folder. Every file stays in place.")
                        .clicked()
                {
                    self.attach_folder(String::new(), false);
                }
            });
        });
        ui.add_space(12.0);
        heading(ui, "Memory");
        let notes = sidebar::memory_notes(&self.memory).len();
        hint(
            ui,
            &match notes {
                0 => "No saved notes for this workspace.".to_owned(),
                1 => "1 saved note for this workspace.".to_owned(),
                n => format!("{n} saved notes for this workspace."),
            },
        );
        if ui.add(widgets::secondary_button("View memory")).clicked() {
            self.open_memory();
            close = true;
        }
        ui.add_space(12.0);
        heading(ui, "What each action does");
        hint(ui, "• Detach removes Moxi's link to the folder and leaves every file in place.\n• Deleting a workspace (the x on its row) removes its chat, memory, and tasks; files stay on disk.\n• Reset folder archives the whole folder beside the original, then recreates it empty. The chat is kept.");
        if attached {
            ui.add_space(4.0);
            ui.add_enabled_ui(self.idle(), |ui| {
                if ui.add(widgets::danger_button("Reset folder…")).clicked() {
                    self.workspace_name.clear();
                    self.workspace_error = None;
                    self.workspace_dialog = Some(WorkspaceDialog::Reset);
                    close = true;
                }
            });
        }
        close
    }

    fn agent_settings(&mut self, ui: &mut egui::Ui) {
        heading(ui, "Approvals");
        let mut auto = self.auto_approve.load(std::sync::atomic::Ordering::Acquire);
        if ui
            .checkbox(&mut auto, "Auto-approve edits, commands, and memory notes")
            .changed()
        {
            self.set_auto_approve(auto);
        }
        hint(ui, "Off by default and reset at every launch. Commands run with your permissions; the workspace folder is not a sandbox.");
        ui.add_space(12.0);
        heading(ui, "Web access");
        if ui
            .checkbox(
                &mut self.settings.web_access,
                "Let the agent search and read web pages",
            )
            .changed()
        {
            self.command(AppCommand::SaveSettings(self.settings.clone()));
        }
        hint(
            ui,
            "Searches go to DuckDuckGo. Pages are fetched from this computer.",
        );
        ui.add_space(12.0);
        heading(ui, "Long tasks");
        hint(ui, "For multi-step work the agent writes a task list and keeps going until every task is done or it explains a blocker. Edit or clear tasks in the sidebar at any time; Stop ends the run.");
    }

    /// Returns true when the provider dialog should open.
    fn model_settings(&mut self, ui: &mut egui::Ui) -> bool {
        heading(ui, "Provider");
        ui.add(
            egui::Label::new(format!(
                "{} · {}",
                self.settings.provider_name, self.settings.lm_studio_url
            ))
            .truncate(),
        );
        ui.label(
            RichText::new(format!(
                "Model: {}",
                self.settings
                    .selected_model
                    .as_deref()
                    .unwrap_or("none selected")
            ))
            .small()
            .color(TEXT_MUTED),
        );
        ui.label(
            RichText::new(if self.settings.auto_context {
                "Context: automatic".to_owned()
            } else {
                format!("Context: manual, {} tokens", self.settings.context_budget)
            })
            .small()
            .color(TEXT_MUTED),
        );
        ui.add_space(4.0);
        let open = ui
            .add_enabled(
                !self.run_pending,
                widgets::secondary_button("Edit provider and context…"),
            )
            .clicked();
        ui.add_space(12.0);
        heading(ui, "Backup");
        hint(ui, "Export or import preferences, the shared playbook, and skills as a JSON file. Folders, chats, and workspace memory are not included.");
        ui.add_enabled_ui(self.file_dialog.is_none(), |ui| {
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add(widgets::secondary_button("Export settings…"))
                    .clicked()
                {
                    self.choose_settings_file(true, ui.ctx());
                }
                if ui
                    .add_enabled(
                        !self.run_pending,
                        widgets::secondary_button("Import settings…"),
                    )
                    .clicked()
                {
                    self.choose_settings_file(false, ui.ctx());
                }
            });
        });
        ui.add_space(12.0);
        heading(ui, "Local data");
        let location = crate::storage::data_location();
        ui.add(egui::Label::new(RichText::new(&location).monospace().size(12.0)).truncate())
            .on_hover_text(&location);
        hint(ui, "Workspaces, memory, tasks, and crash.log live here.");
        open
    }
}

fn help_settings(ui: &mut egui::Ui) {
    heading(ui, "Tools the agent can use");
    for (name, help) in [
        (
            "list_files · read_file · search_content · grep · glob",
            "Explore the folder. Automatic.",
        ),
        (
            "edit_file · write_file",
            "Change or create files. Needs approval.",
        ),
        ("delete_path", "Delete a file or folder. Needs approval."),
        (
            "run_command",
            "Build, test, git, or scripts. Needs approval.",
        ),
        (
            "list_tasks · update_tasks",
            "Plan and track work in the sidebar task list. Automatic.",
        ),
        (
            "save_skill · run_skill",
            "Save a working Python script as a reusable skill, and run saved skills. Needs approval.",
        ),
        (
            "ask_user",
            "Ask you a multiple-choice question when a decision is needed. Automatic.",
        ),
        (
            "remember",
            "Save a note to workspace memory. Needs approval.",
        ),
        (
            "web_search · fetch_url",
            "Research online when Web access is on. Automatic.",
        ),
    ] {
        ui.label(RichText::new(name).monospace().size(12.0));
        hint(ui, help);
    }
    ui.add_space(8.0);
    heading(ui, "Commands");
    for (command, help) in crate::app::commands::HELP {
        ui.label(RichText::new(*command).monospace().size(12.0));
        hint(ui, help);
    }
    ui.add_space(8.0);
    heading(ui, "Tips");
    hint(ui, "Send while Moxi works to queue a message; the agent reads it after its current step. Stop cancels the run and the queue. Double-click a workspace to rename it.");
}

fn heading(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).strong().size(14.0));
}

fn hint(ui: &mut egui::Ui, text: &str) {
    ui.add(egui::Label::new(RichText::new(text).small().color(TEXT_MUTED)).wrap());
}

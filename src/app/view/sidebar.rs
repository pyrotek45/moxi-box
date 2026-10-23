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
                        widgets::text_field(
                            ui,
                            &mut self.settings.workspace_root,
                            "Workspace path",
                        );
                        if ui
                            .add_enabled(
                                !self.run_pending,
                                widgets::secondary_button("Open workspace"),
                            )
                            .clicked()
                        {
                            self.preview = None;
                            self.command(AppCommand::SaveSettings(self.settings.clone()));
                            self.command(AppCommand::BrowseWorkspace {
                                root: self.settings.workspace_root.clone(),
                            });
                        }
                        ui.label(
                            RichText::new("Files and search use this directory.")
                                .small()
                                .color(TEXT_MUTED),
                        );
                        ui.add_space(12.0);
                        ScrollArea::vertical()
                            .id_salt("files")
                            .scroll_bar_visibility(
                                egui::scroll_area::ScrollBarVisibility::AlwaysHidden,
                            )
                            .max_height(230.0)
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

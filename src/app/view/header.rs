use super::*;

impl MoxiBoxApp {
    pub(super) fn header(&mut self, context: &egui::Context) {
        egui::TopBottomPanel::top("header")
            .frame(
                egui::Frame::new()
                    .fill(crate::ui::theme::BACKGROUND)
                    .inner_margin(egui::Margin::symmetric(16, 10)),
            )
            .show(context, |ui| {
                ui.horizontal_wrapped(|ui| {
                    widgets::brand(ui, APPLICATION_NAME);
                    ui.add_space(8.0);
                    widgets::status_badge(ui, self.provider_status());
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.toggle_value(&mut self.connection_settings, "Connection");
                        ui.toggle_value(&mut self.show_workspace, "Workspace");
                    });
                });
                if self.connection_settings {
                    ui.add_space(8.0);
                    ui.add_enabled_ui(!self.run_pending && !self.provider_checking, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.allocate_ui(egui::vec2(220.0, 34.0), |ui| {
                                widgets::text_field(
                                    ui,
                                    &mut self.settings.lm_studio_url,
                                    "LM Studio URL",
                                );
                            });
                            if ui.add(widgets::secondary_button("Connect")).clicked() {
                                self.provider_checking = true;
                                self.notice = Some("Checking LM Studio…".to_owned());
                                self.command(AppCommand::RefreshModels {
                                    base_url: self.settings.lm_studio_url.clone(),
                                });
                            }
                            if widgets::model_picker(
                                ui,
                                &mut self.settings.selected_model,
                                &self.models,
                            ) {
                                self.command(AppCommand::SaveSettings(self.settings.clone()));
                            }
                            if ui.add(widgets::secondary_button("Save")).clicked() {
                                self.command(AppCommand::SaveSettings(self.settings.clone()));
                            }
                        });
                    });
                }
                if let Some(notice) = &self.notice {
                    // Connection details stay in Connection; other feedback remains visible.
                    if self.connection_settings || !notice.starts_with("Provider request failed") {
                        ui.add(
                            egui::Label::new(RichText::new(notice).small().color(TEXT_MUTED))
                                .truncate(),
                        )
                        .on_hover_text(notice);
                    }
                }
            });
    }
}

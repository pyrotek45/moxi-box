use super::*;

impl MoxiBoxApp {
    pub(super) fn header(&mut self, context: &egui::Context) {
        egui::TopBottomPanel::top("header")
            .frame(egui::Frame::new().fill(crate::ui::theme::BACKGROUND).inner_margin(egui::Margin::symmetric(16, 10)))
            .show(context, |ui| {
                ui.horizontal(|ui| {
                    widgets::brand(ui, APPLICATION_NAME);
                    ui.add_space(8.0);
                    widgets::status_badge(ui, self.provider_status());
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.toggle_value(&mut self.show_workspace, "Workspace");
                    });
                });
                ui.add_space(4.0);
                ui.add_enabled_ui(!self.run_pending, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        if ui.add_enabled(!self.connection.checking,widgets::secondary_button(&self.settings.provider_name)).on_hover_text("Configure the model provider").clicked() {
                            self.provider_draft = Some(self.settings.clone());
                            self.provider_form_error = None;
                        }
                        if ui.add_enabled_ui(!self.connection.checking,|ui| widgets::model_picker(ui, &mut self.settings.selected_model, &self.models)).inner {
                            self.command(AppCommand::SaveSettings(self.settings.clone()));
                        }
                        if ui.add_enabled(!self.connection.checking,widgets::secondary_button("Refresh")).on_hover_text("Discover model IDs without connecting or loading a model").clicked() {
                            self.refresh_models(false);
                        }
                        if ui.add(widgets::primary_button(if self.connection.checking { "Cancel" } else if self.connection.connected { "Disconnect" } else { "Connect" })).clicked() {
                            if self.connection.connected || self.connection.checking { self.cancelled.store(true,std::sync::atomic::Ordering::Release); self.connection.disconnect(); self.connection.error = None; }
                            else { self.refresh_models(true); }
                        }
                    });
                });
                if self.connection.checking { ui.label(RichText::new("Contacting model server…").small().color(TEXT_MUTED)); }
                if let Some(error) = &self.connection.error {
                    ui.add(egui::Label::new(RichText::new("Cannot reach the model server. Check the URL and that its server is running.").small().color(Color32::from_rgb(224, 155, 143))).wrap());
                    egui::CollapsingHeader::new("Connection details").show(ui, |ui| {
                        ScrollArea::vertical().max_height(64.0).show(ui, |ui| { ui.add(egui::Label::new(error).wrap()); });
                    });
                } else if self.connection.available && self.models.is_empty() {
                    ui.label(RichText::new("Server reachable. Load a model or enter its exact ID in provider settings.").small().color(TEXT_MUTED));
                }
                if let Some(notice) = &self.notice {
                    ui.add(egui::Label::new(RichText::new(notice).small().color(TEXT_MUTED)).truncate()).on_hover_text(notice);
                }
            });
    }
}

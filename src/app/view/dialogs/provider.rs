use super::*;

impl MoxiBoxApp {
    pub(super) fn provider_dialog(&mut self, context: &egui::Context, width: f32, height: f32) {
        if let Some(mut draft) = self.provider_draft.clone() {
            let mut save = false;
            let mut cancel = false;
            let response = egui::Modal::new(egui::Id::new("provider_settings"))
                .frame(widgets::card().inner_margin(24))
                .show(context, |ui| {
                    ui.set_width(width);
                    ui.heading("Model provider");
                    ui.label(RichText::new("Connect to a local OpenAI-compatible server.").color(TEXT_MUTED));
                    ui.add_space(8.0);
                    ScrollArea::vertical().id_salt("provider_form").max_height(height - 100.0).show(ui, |ui| {
                        ui.label("Provider");
                        let before = draft.provider_name.clone();
                        egui::ComboBox::from_id_salt("provider_preset").width(ui.available_width()).selected_text(&draft.provider_name).show_ui(ui, |ui| {
                            for preset in ["LM Studio", "Ollama", "Custom OpenAI-compatible"] { ui.selectable_value(&mut draft.provider_name, preset.to_owned(), preset); }
                        });
                        if before != draft.provider_name {
                            draft.lm_studio_url = match draft.provider_name.as_str() {
                                "LM Studio" => "http://127.0.0.1:1234/v1".to_owned(),
                                "Ollama" => "http://127.0.0.1:11434/v1".to_owned(),
                                _ => draft.lm_studio_url.clone(),
                            };
                        }
                        ui.label("Base URL");
                        widgets::text_field(ui, &mut draft.lm_studio_url, "http://127.0.0.1:1234/v1");
                        ui.label("Default model ID");
                        let mut model = draft.selected_model.clone().unwrap_or_default();
                        if widgets::text_field(ui, &mut model, "Optional · discovered models appear in the header").changed() {
                            draft.selected_model = (!model.trim().is_empty()).then_some(model);
                        }
                        ui.label(RichText::new("Save updates the provider and discovers models. Click Connect in the header when you're ready.").small().color(TEXT_MUTED));
                        egui::CollapsingHeader::new("Context recovery").show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.radio_value(&mut draft.auto_context, true, "Auto");
                                ui.radio_value(&mut draft.auto_context, false, "Manual");
                            });
                            if draft.auto_context {
                                ui.label(RichText::new("Sends full history until the provider reports that context is full, then learns a safe limit for this workspace and retries.").small().color(TEXT_MUTED));
                            } else {
                                ui.horizontal(|ui| {
                                    ui.add(egui::DragValue::new(&mut draft.context_budget).range(8192..=131072).speed(1024));
                                    ui.label("tokens");
                                });
                                ui.label(RichText::new("Compacts before requests cross this conservative estimate. More predictable for models whose context errors are inconsistent.").small().color(TEXT_MUTED));
                            }
                            ui.label(RichText::new("Complete conversation logs always stay saved.").small().color(TEXT_MUTED));
                        });
                        if let Some(error) = &self.provider_form_error { ui.colored_label(Color32::LIGHT_RED, error); }
                    });
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        save = ui.add(widgets::primary_button("Save")).clicked();
                        cancel = ui.add(widgets::secondary_button("Cancel")).clicked();
                    });
                    save |= widgets::enter_pressed(ui);
                });
            self.provider_draft = Some(draft.clone());
            if save {
                match self.save_provider(draft) {
                    Ok(()) => {
                        self.provider_draft = None;
                        self.provider_form_error = None;
                    }
                    Err(error) => self.provider_form_error = Some(error),
                }
            } else if cancel || response.should_close() {
                self.provider_draft = None;
                self.provider_form_error = None;
            }
        }
    }
}

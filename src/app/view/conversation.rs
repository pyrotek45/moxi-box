use super::*;

impl MoxiBoxApp {
    pub(super) fn conversation(&mut self, context: &egui::Context) {
        egui::CentralPanel::default()
            .frame(egui::Frame::central_panel(&context.style()).inner_margin(16))
            .show(context, |ui| {
                egui::TopBottomPanel::bottom("composer").frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(0, 8))).show_inside(ui, |ui| {
                    if widgets::composer(ui, &mut self.composer, self.agent_mode, self.run_pending).send {
                        self.send_message();
                    }
                    ui.vertical_centered(|ui| { ui.label(RichText::new("Local models. Your workspace. You're in control.").small().color(TEXT_MUTED)); });
                });
                ui.horizontal_wrapped(|ui| {
                    ui.heading(if self.preview.is_some() { "File preview" } else { "Conversation" });
                    ui.add_enabled_ui(!self.run_pending, |ui| {
                        widgets::mode_pill(ui, &mut self.agent_mode, false, "Chat");
                        widgets::mode_pill(ui, &mut self.agent_mode, true, "Agent");
                    });
                });
                ui.add_space(12.0);
                ScrollArea::vertical().id_salt("conversation").auto_shrink([false, false])
                    .stick_to_bottom(self.preview.is_none() && self.approval.is_none())
                    .show(ui, |ui| {
                        if self.agent_mode {
                            ui.label(RichText::new("Writes and commands ask for your approval.").small().color(TEXT_MUTED));
                        }
                        if let Some((request_id, summary)) = self.approval.clone() {
                            widgets::card().fill(Color32::from_rgb(54, 45, 30)).show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.label(RichText::new("Approval needed").strong().color(Color32::LIGHT_YELLOW));
                                ui.add(egui::Label::new(&summary).wrap());
                                ui.horizontal_wrapped(|ui| {
                                    for (label, approved) in [("Approve", true), ("Deny", false)] {
                                        if ui.add(widgets::secondary_button(label)).clicked() {
                                            self.command(AppCommand::ResolveApproval { request_id: request_id.clone(), approved });
                                            self.approval = None;
                                        }
                                    }
                                });
                            });
                        }
                        if !self.activity.is_empty() {
                            egui::CollapsingHeader::new("Agent activity").show(ui, |ui| {
                                for activity in self.activity.iter().rev().take(8) {
                                    ui.add(egui::Label::new(RichText::new(activity).monospace()).wrap());
                                }
                            });
                        }
                        if let Some((path, content)) = &self.preview {
                            let path = path.clone();
                            let content = content.clone();
                            if ui.add(widgets::secondary_button("Return to chat")).clicked() {
                                self.preview = None;
                            }
                            ui.add(egui::Label::new(RichText::new(path).strong()).wrap());
                            ScrollArea::horizontal().id_salt("file_content").show(ui, |ui| {
                                ui.monospace(content);
                            });
                        } else {
                            if self.messages.is_empty() {
                                ui.add_space((ui.available_height() * 0.25).max(16.0));
                                ui.vertical_centered(|ui| {
                                    ui.label(RichText::new("Your ideas, built locally.").size(28.0));

                                    ui.label(RichText::new("Chat with your local model, or use Agent mode to work on code.").color(TEXT_MUTED));
                                });
                            }
                            for message in &self.messages {
                                widgets::message_bubble(ui, message, self.streaming_id == Some(message.id));
                                ui.add_space(10.0);
                            }
                        }
                    });
            });
    }
}

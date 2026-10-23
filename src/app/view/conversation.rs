use super::*;

impl MoxiBoxApp {
    pub(super) fn conversation(&mut self, context: &egui::Context) {
        egui::CentralPanel::default()
            .frame(egui::Frame::central_panel(&context.style()).inner_margin(16))
            .show(context, |ui| {
                egui::TopBottomPanel::bottom("composer")
                    .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(0, 8)))
                    .show_inside(ui, |ui| {
                        let response = widgets::composer(ui, &mut self.composer, self.agent_mode, self.run_pending, self.history.browsing());
                        if let Some(older) = response.history { self.history.navigate(&mut self.composer, older); }
                        else if response.edited { self.history.edited(); }
                        if response.send { self.send_message(); }
                        if response.stop { self.cancelled.store(true,std::sync::atomic::Ordering::Release); self.notice = Some("Stopping…".to_owned()); }
                        ui.vertical_centered(|ui| { ui.label(RichText::new("Local models. Your workspace. You're in control.").small().color(TEXT_MUTED)); });
                    });
                ui.horizontal_wrapped(|ui| {
                    ui.heading(if self.preview.is_some() { "File preview" } else { "Conversation" });
                    ui.add_enabled_ui(!self.run_pending, |ui| {
                        widgets::mode_pill(ui, &mut self.agent_mode, false, "Chat");
                        widgets::mode_pill(ui, &mut self.agent_mode, true, "Agent");
                    });
                    if ui.add(widgets::secondary_button("Toolbelt")).clicked() { self.show_toolbelt = true; }
                });
                if self.approval.is_some() { ui.label(RichText::new("Waiting for your approval").small().color(Color32::LIGHT_YELLOW)); }
                else if self.run_pending { ui.label(RichText::new("Working…").small().color(TEXT_MUTED)); }
                ui.add_space(8.0);
                ScrollArea::vertical().id_salt(("conversation",self.conversation_id))
                    .auto_shrink([false, false]).stick_to_bottom(self.preview.is_none())
                    .show(ui, |ui| {
                        if self.agent_mode { ui.label(RichText::new("Writes and commands ask for your approval.").small().color(TEXT_MUTED)); }
                        if !self.summary.is_empty() {
                            egui::CollapsingHeader::new("Saved conversation summary").show(ui, |ui| {
                                ui.add(egui::Label::new(&self.summary).wrap());
                                ui.label(RichText::new("Earlier turns are summarized for the model. The full conversation is saved.").small().color(TEXT_MUTED));
                                ui.checkbox(&mut self.show_full_history,"Show earlier messages");
                            });
                        }
                        if let Some((path, content)) = self.preview.clone() {
                            if ui.add(widgets::secondary_button("Return to chat")).clicked() { self.preview = None; }
                            ui.add(egui::Label::new(RichText::new(path).strong()).wrap());
                            ScrollArea::horizontal().id_salt("file_content").show(ui, |ui| { ui.monospace(content); });
                        } else {
                            if self.messages.is_empty() {
                                ui.add_space((ui.available_height() * 0.20).max(12.0));
                                ui.vertical_centered(|ui| {
                                    ui.label(RichText::new("Your ideas, built locally.").size(28.0));
                                    ui.label(RichText::new("Chat with your local model, or use Agent mode to work on code.").color(TEXT_MUTED));
                                });
                            }
                            let mut approval_response = None;
                            for message in self.messages.iter().enumerate().filter(|(index,message)| self.show_full_history || message.id > self.compacted_through || *index + 2 >= self.messages.len()).map(|(_,message)| message) {
                                let steps = self.activity.iter().filter(|step| step.message_id == message.id).collect::<Vec<_>>();
                                let deferred_bubble = message.role == MessageRole::Assistant && !steps.is_empty();
                                if !deferred_bubble && (!message.content.is_empty() || steps.is_empty()) {
                                    widgets::message_bubble(ui, message, self.streaming_id == Some(message.id));
                                }
                                for step in steps {
                                    ui.push_id((&step.call_id,step.message_id), |ui| {
                                        ui.scope(|ui| {
                                            ui.spacing_mut().interact_size.y = 22.0;
                                            ui.spacing_mut().item_spacing.y = 4.0;
                                            let color = if step.title.starts_with("Failed") || step.title.starts_with("Interrupted") { Color32::LIGHT_RED } else if step.title.starts_with("Awaiting") { Color32::LIGHT_YELLOW } else { TEXT_MUTED };
                                            egui::CollapsingHeader::new(RichText::new(&step.title).size(12.0).color(color)).show(ui, |ui| {
                                                ScrollArea::vertical().max_height(180.0).show(ui, |ui| { ui.add(egui::Label::new(RichText::new(&step.detail).monospace().size(12.0)).wrap()); });
                                            }).header_response.on_hover_text("Expand to inspect arguments or output");
                                        });
                                    });
                                }
                                if deferred_bubble && !message.content.is_empty() { widgets::message_bubble(ui,message,self.streaming_id == Some(message.id)); }
                                if self.streaming_id == Some(message.id) {
                                    if let Some((request_id, summary)) = self.approval.clone() {
                                        widgets::card().show(ui, |ui| {
                                            ui.label(RichText::new("Approval needed").strong().color(Color32::LIGHT_YELLOW));
                                            ui.add(egui::Label::new(summary).wrap());
                                            ui.horizontal_wrapped(|ui| {
                                                for (label, approved) in [("Approve", true), ("Deny", false)] {
                                                    if ui.add(widgets::secondary_button(label)).clicked() {
                                                        approval_response = Some((request_id.clone(),approved));
                                                    }
                                                }
                                            });
                                        });
                                    }
                                }
                                ui.add_space(10.0);
                            }
                            if let Some((request_id, approved)) = approval_response { self.command(AppCommand::ResolveApproval {request_id,approved}); self.approval = None; }
                        }
                    });
            });
    }
}

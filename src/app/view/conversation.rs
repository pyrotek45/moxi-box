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
                        if response.stop { self.stop(); }
                        ui.vertical_centered(|ui| { ui.label(RichText::new("Local models. Your workspace. You're in control.").small().color(TEXT_MUTED)); });
                    });
                let mut rename = false;
                ui.horizontal_wrapped(|ui| {
                    let title = self.workspaces.iter().find(|workspace| Some(workspace.id) == self.workspace_id).map_or("Moxi".to_owned(), |workspace| workspace.name.clone());
                    // Leave room for the mode buttons beside a long workspace name.
                    let room = (ui.available_width() - 190.0).max(80.0);
                    ui.scope(|ui| {
                        ui.set_max_width(room);
                        if ui.add(egui::Label::new(RichText::new(if self.preview.is_some() { "File preview" } else { title.as_str() }).heading()).truncate().sense(egui::Sense::click())).on_hover_text(format!("{title}\nClick to rename")).clicked() && self.preview.is_none() { rename = true; }
                    });
                    ui.add_enabled_ui(!self.run_pending, |ui| {
                        widgets::mode_pill(ui, &mut self.agent_mode, false, "Chat");
                        widgets::mode_pill(ui, &mut self.agent_mode, true, "Agent");
                    });
                    if self.agent_mode {
                        let mut auto = self.auto_approve.load(std::sync::atomic::Ordering::Acquire);
                        if ui.checkbox(&mut auto, "Auto-approve").on_hover_text("Run edits, writes, commands, and memory notes without asking").changed() { self.set_auto_approve(auto); }
                    }
                });
                if rename { self.name_workspace(self.workspace_id); }
                // A fixed slot: status text appearing or vanishing must not shift the conversation.
                let (slot, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 18.0), egui::Sense::hover());
                ui.scope_builder(egui::UiBuilder::new().max_rect(slot), |ui| {
                    if self.approval.is_some() { ui.label(RichText::new("Waiting for your approval").small().color(Color32::LIGHT_YELLOW)); }
                    else if self.run_pending && self.activity_at.elapsed().as_secs() >= 5 { widgets::working_indicator(ui, &self.waiting_label("Working")); }
                });
                ui.add_space(8.0);
                // The preview header stays outside the scroll area so Return to chat is always reachable.
                if let Some((path, _)) = &self.preview {
                    let mut close = false;
                    ui.horizontal(|ui| {
                        close = ui.add(widgets::secondary_button("Return to chat")).clicked();
                        ui.add(egui::Label::new(RichText::new(path).strong()).truncate()).on_hover_text(path);
                    });
                    ui.separator();
                    if close { self.preview = None; }
                }
                ScrollArea::vertical().id_salt(("conversation",self.workspace_id, self.preview.as_ref().map(|(path, _)| path.clone())))
                    .auto_shrink([false, false]).min_scrolled_height(0.0)
                    .max_height(ui.available_height().max(0.0)).stick_to_bottom(self.preview.is_none())
                    .show(ui, |ui| {
                        if self.agent_mode {
                            if self.auto_approve.load(std::sync::atomic::Ordering::Acquire) {
                                ui.label(RichText::new("Auto-approve is on: edits, writes, and commands run without asking.").small().color(Color32::LIGHT_YELLOW));
                            } else {
                                ui.label(RichText::new("Edits, writes, and commands ask for your approval.").small().color(TEXT_MUTED));
                            }
                        }
                        if !self.summary.is_empty() {
                            egui::CollapsingHeader::new("Saved conversation summary").show(ui, |ui| {
                                ui.add(egui::Label::new(&self.summary).wrap());
                                ui.label(RichText::new("Earlier turns are summarized for the model. The full conversation is saved.").small().color(TEXT_MUTED));
                                ui.checkbox(&mut self.show_full_history,"Show earlier messages");
                            });
                        }
                        if let Some((_, content)) = self.preview.clone() {
                            ScrollArea::horizontal().id_salt("file_content").show(ui, |ui| { ui.monospace(content); });
                        } else {
                            if self.messages.is_empty() {
                                let roomy = ui.available_height() >= 120.0;
                                if roomy { ui.add_space(ui.available_height() * 0.20); }
                                ui.vertical_centered(|ui| {
                                    if roomy { ui.label(RichText::new("Your ideas, built locally.").size(28.0)); }
                                    ui.label(RichText::new("Chat with your local model, or use Agent mode to work on code.").color(TEXT_MUTED));
                                });
                            }
                            let mut approval_response = None;
                            for message in self.messages.iter().enumerate().filter(|(index,message)| self.show_full_history || message.id > self.compacted_through || *index + 2 >= self.messages.len()).map(|(_,message)| message) {
                                let streaming = self.streaming_id == Some(message.id);
                                let steps = self.activity.iter().filter(|step| step.message_id == message.id).collect::<Vec<_>>();
                                if steps.is_empty() && streaming && message.content.is_empty() {
                                    let label = self.waiting_label("Thinking");
                                    widgets::bubble(ui, message.id, message.role, |ui| widgets::working_indicator(ui, &label));
                                } else if steps.is_empty() {
                                    widgets::message_bubble(ui, message, streaming);
                                } else {
                                    widgets::bubble(ui, message.id, message.role, |ui| {
                                        // Text and tool steps in the order they happened.
                                        let content = &message.content;
                                        let mut start = 0;
                                        for (index, step) in steps.iter().enumerate() {
                                            let mut end = step.offset.clamp(start, content.len());
                                            while !content.is_char_boundary(end) { end -= 1; }
                                            if !content[start..end].trim().is_empty() {
                                                ui.push_id(("text", index), |ui| widgets::message_text(ui, &content[start..end]));
                                            }
                                            start = end;
                                            tool_step(ui, step);
                                        }
                                        if !content[start..].trim().is_empty() {
                                            ui.add_space(4.0);
                                            ui.push_id("text_end", |ui| widgets::message_text(ui, &content[start..]));
                                        } else if streaming && self.approval.is_none() {
                                            ui.add_space(4.0);
                                            let running = steps.last().is_some_and(|step| step.running);
                                            widgets::working_indicator(ui, &self.waiting_label(if running { "Running tool" } else { "Thinking" }));
                                        }
                                    });
                                }
                                if self.streaming_id == Some(message.id) {
                                    if let Some((_, summary, preview)) = self.approval.clone() {
                                        widgets::card().show(ui, |ui| {
                                            ui.label(RichText::new(summary).strong().color(Color32::LIGHT_YELLOW));
                                            if !preview.is_empty() {
                                                widgets::code_view(ui, "approval_preview", &preview);
                                            }
                                            ui.add_space(6.0);
                                            ui.horizontal_wrapped(|ui| {
                                                if ui.add(widgets::primary_button("Approve")).clicked() { approval_response = Some(true); }
                                                ui.add_space(8.0);
                                                if ui.add(widgets::secondary_button("Deny")).clicked() { approval_response = Some(false); }
                                            });
                                        });
                                    }
                                }
                                ui.add_space(10.0);
                            }
                            for text in &self.queued {
                                widgets::card().show(ui, |ui| {
                                    ui.label(RichText::new("Queued · Moxi reads this at its next step").small().color(TEXT_MUTED));
                                    ui.add(egui::Label::new(text).wrap());
                                });
                                ui.add_space(6.0);
                            }
                            if let Some(approved) = approval_response { self.resolve_approval(approved); }
                        }
                    });
            });
    }
}

fn tool_step(ui: &mut egui::Ui, step: &AgentActivity) {
    let title = &step.title;
    let status = if step.running && !title.starts_with("Awaiting") {
        widgets::StepStatus::Running
    } else if title.starts_with("Awaiting") {
        widgets::StepStatus::Waiting
    } else if ["Failed", "Interrupted", "Denied"]
        .iter()
        .any(|prefix| title.starts_with(prefix))
    {
        widgets::StepStatus::Failed
    } else {
        widgets::StepStatus::Done
    };
    // Edits open by default so their diff is visible, like a code review.
    let open_by_default = title.starts_with("Edited") || title.starts_with("Wrote");
    ui.push_id(("step", step.message_id, &step.call_id), |ui| {
        widgets::tool_step(ui, title, &step.detail, status, open_by_default);
    });
}

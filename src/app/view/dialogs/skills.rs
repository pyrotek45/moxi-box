use super::*;

impl MoxiBoxApp {
    /// Saved skills: what each does, its code, and Delete (asks once more before deleting).
    pub(super) fn skills_dialog(&mut self, context: &egui::Context, width: f32, height: f32) {
        if !self.show_skills {
            return;
        }
        let mut close = false;
        let mut delete = None;
        let response = egui::Modal::new(egui::Id::new("skills"))
            .frame(widgets::card().inner_margin(24))
            .show(context, |ui| {
                ui.set_width(width);
                ui.heading("Skills");
                ui.add(
                    egui::Label::new(
                        RichText::new("Python scripts the agent saved after they worked, to reuse in any workspace. Saving and running a skill always asks for your approval.")
                            .small()
                            .color(TEXT_MUTED),
                    )
                    .wrap(),
                );
                ui.add_space(8.0);
                ScrollArea::vertical()
                    .id_salt("skills_body")
                    .max_height((height - 150.0).max(80.0))
                    .show(ui, |ui| {
                        if self.skills.is_empty() {
                            ui.label(RichText::new("No skills yet. In Agent mode, the agent can save a script that worked with save_skill.").color(TEXT_MUTED));
                        }
                        for skill in &self.skills {
                            ui.push_id(&skill.name, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(&skill.name).monospace().strong());
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        let confirming = self.skill_to_delete.as_deref() == Some(skill.name.as_str());
                                        if confirming {
                                            if ui.add(widgets::danger_button("Delete")).clicked() {
                                                delete = Some(skill.name.clone());
                                            }
                                            if widgets::link_button(ui, "Keep").clicked() {
                                                self.skill_to_delete = None;
                                            }
                                        } else if widgets::link_button(ui, "Delete").clicked() {
                                            self.skill_to_delete = Some(skill.name.clone());
                                        }
                                    });
                                });
                                if !skill.description.is_empty() {
                                    ui.add(egui::Label::new(&skill.description).wrap());
                                }
                                if !skill.usage.is_empty() {
                                    ui.label(RichText::new(format!("Usage: {}", skill.usage)).small().color(TEXT_MUTED));
                                }
                                egui::CollapsingHeader::new("Code").id_salt("code").show(ui, |ui| {
                                    widgets::code_view(ui, &format!("skill_{}", skill.name), &skill.code);
                                });
                                ui.add_space(8.0);
                            });
                        }
                    });
                if let Ok(folder) = crate::skills::directory() {
                    let folder = folder.display().to_string();
                    ui.add(egui::Label::new(RichText::new(format!("Folder: {folder}")).small().color(TEXT_MUTED)).truncate())
                        .on_hover_text(folder);
                }
                ui.add_space(8.0);
                close = ui.add(widgets::primary_button("Done")).clicked() || widgets::enter_pressed(ui);
            });
        if let Some(name) = delete {
            self.skill_to_delete = None;
            self.command(AppCommand::DeleteSkill(name));
        }
        if close || response.should_close() {
            self.show_skills = false;
            self.skill_to_delete = None;
        }
    }
}

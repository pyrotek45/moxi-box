use super::*;

impl MoxiBoxApp {
    pub(super) fn goal_dialog(&mut self, context: &egui::Context, width: f32) {
        let Some(mut goal) = self.goal_draft.clone() else {
            return;
        };
        let mut save = false;
        let mut clear = false;
        let mut cancel = false;
        let response = egui::Modal::new(egui::Id::new("workspace_goal"))
            .frame(widgets::card().inner_margin(24))
            .show(context, |ui| {
                ui.set_width(width);
                ui.heading("Goal");
                ui.label(
                    RichText::new("A long-running objective for this workspace. The agent sees it on every request, plans it as tasks, and keeps working until they are done. You can also type /goal <text>.")
                        .small()
                        .color(TEXT_MUTED),
                );
                ui.add_space(8.0);
                let field = ui.add_sized(
                    [ui.available_width(), 90.0],
                    egui::TextEdit::multiline(&mut goal)
                        .hint_text("For example: a Python CLI that converts CSV to JSON, with passing tests"),
                );
                widgets::focus_if_idle(ui, &field);
                ui.label(RichText::new("Ctrl+Enter saves").small().color(TEXT_MUTED));
                ui.horizontal(|ui| {
                    save = ui.add(widgets::primary_button("Save")).clicked();
                    if !self.goal.is_empty() {
                        clear = ui.add(widgets::secondary_button("Clear goal")).clicked();
                    }
                    cancel = ui.add(widgets::secondary_button("Cancel")).clicked();
                });
                save |= ui.input_mut(|input| {
                    input.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter)
                });
            });
        self.goal_draft = Some(goal.clone());
        if save || clear {
            let goal = if clear {
                String::new()
            } else {
                goal.trim().to_owned()
            };
            self.goal = goal.clone();
            self.command(AppCommand::SetGoal(goal));
            self.goal_draft = None;
        } else if cancel || response.should_close() {
            self.goal_draft = None;
        }
    }
}

use super::*;

impl MoxiBoxApp {
    pub(super) fn memory_dialog(&mut self, context: &egui::Context, width: f32, height: f32) {
        if let Some(mut memory) = self.memory_draft.clone() {
            let mut save = false;
            let mut cancel = false;
            let response = egui::Modal::new(egui::Id::new("workspace_memory"))
                .frame(widgets::card().inner_margin(24))
                .show(context, |ui| {
                    ui.set_width(width);
                    ui.heading("Workspace memory");
                    ui.label(
                        RichText::new("Notes and preferences the model keeps for this workspace.")
                            .color(TEXT_MUTED),
                    );
                    ScrollArea::vertical()
                        .max_height(height - 100.0)
                        .show(ui, |ui| {
                            ui.add_sized(
                                [ui.available_width(), 130.0],
                                egui::TextEdit::multiline(&mut memory).hint_text(
                                    "Project conventions, important decisions, build commands…",
                                ),
                            );
                        });
                    ui.label(
                        RichText::new(format!("{} / 6,000 bytes · Ctrl+Enter saves", memory.len()))
                            .small()
                            .color(TEXT_MUTED),
                    );
                    ui.horizontal(|ui| {
                        save = ui
                            .add_enabled(
                                memory.len() <= 6000,
                                widgets::primary_button("Save memory"),
                            )
                            .clicked();
                        cancel = ui.add(widgets::secondary_button("Cancel")).clicked();
                    });
                    // Plain Enter adds a line in the notes; Ctrl+Enter saves.
                    save |= memory.len() <= 6000
                        && ui.input_mut(|input| {
                            input.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter)
                        });
                });
            self.memory_draft = Some(memory.clone());
            if save {
                self.command(AppCommand::SaveMemory(memory));
                self.memory_draft = None;
            } else if cancel || response.should_close() {
                self.memory_draft = None;
            }
        }
    }
}

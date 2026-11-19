use super::*;

const LIMIT: usize = 6000;

impl MoxiBoxApp {
    /// Workspace memory and the shared playbook, edited together and saved with one button.
    pub(super) fn memory_dialog(&mut self, context: &egui::Context, width: f32, height: f32) {
        let Some(mut memory) = self.memory_draft.clone() else {
            return;
        };
        let mut playbook = self.playbook_draft.clone();
        let mut save = false;
        let mut cancel = false;
        let response = egui::Modal::new(egui::Id::new("workspace_memory"))
            .frame(widgets::card().inner_margin(24))
            .show(context, |ui| {
                ui.set_width(width);
                ui.heading("Memory");
                egui::Frame::new()
                    .fill(Color32::from_gray(38))
                    .corner_radius(10.0)
                    .inner_margin(3)
                    .show(ui, |ui| {
                        widgets::tab_bar(ui, &mut self.memory_tab, &["This workspace", "All workspaces"])
                    });
                let shared = self.memory_tab == 1;
                ui.label(
                    RichText::new(if shared {
                        "The shared playbook: notes used in every workspace. The agent can add lessons (with your approval) after solving a problem. Unshare moves a note back to this workspace only."
                    } else {
                        "Facts and lessons for this workspace only. Share moves a note to every workspace. The agent asks before saving a new one."
                    })
                    .small()
                    .color(TEXT_MUTED),
                );
                ui.add_space(4.0);
                let moved = ScrollArea::vertical()
                    .id_salt(("memory_body", shared))
                    .max_height((height - 230.0).max(70.0))
                    .show(ui, |ui| {
                        notes_editor(ui, if shared { &mut playbook } else { &mut memory }, shared)
                    })
                    .inner;
                // Sharing moves a note between this workspace and the playbook; nothing is copied.
                if let Some(note) = moved {
                    let target = if shared { &mut memory } else { &mut playbook };
                    *target = format!("{}\n- {note}", target.trim_end()).trim().to_owned();
                }
                let text = if shared { &mut playbook } else { &mut memory };
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    let input_width = (ui.available_width() - 68.0).max(60.0);
                    let field = ui
                        .allocate_ui(egui::vec2(input_width, 34.0), |ui| {
                            widgets::text_field(ui, &mut self.memory_note, "Add a note")
                        })
                        .inner;
                    let submit =
                        field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
                    let add = ui
                        .add_enabled(
                            !self.memory_note.trim().is_empty(),
                            widgets::secondary_button("Add"),
                        )
                        .clicked();
                    if (add || submit) && !self.memory_note.trim().is_empty() {
                        let note = std::mem::take(&mut self.memory_note);
                        *text = format!("{}\n- {}", text.trim_end(), note.trim())
                            .trim()
                            .to_owned();
                    }
                });
                let too_long = memory.len() > LIMIT || playbook.len() > LIMIT;
                let current = if shared { playbook.len() } else { memory.len() };
                ui.label(
                    RichText::new(format!("{current} / 6,000 bytes · Ctrl+Enter saves"))
                        .small()
                        .color(if current > LIMIT { Color32::LIGHT_RED } else { TEXT_MUTED }),
                );
                ui.horizontal(|ui| {
                    save = ui
                        .add_enabled(!too_long, widgets::primary_button("Save"))
                        .clicked();
                    cancel = ui.add(widgets::secondary_button("Cancel")).clicked();
                });
                // Plain Enter adds a line or note; Ctrl+Enter saves.
                save |= !too_long
                    && ui.input_mut(|input| {
                        input.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter)
                    });
            });
        self.memory_draft = Some(memory.clone());
        self.playbook_draft = playbook.clone();
        if save {
            if memory != self.memory {
                self.command(AppCommand::SaveMemory(memory));
            }
            if playbook != self.playbook {
                self.playbook = playbook.clone();
                self.command(AppCommand::SavePlaybook(playbook));
            }
            self.memory_draft = None;
            self.memory_note.clear();
        } else if cancel || response.should_close() {
            self.memory_draft = None;
            self.memory_note.clear();
        }
    }
}

/// Notes as a readable list with Remove and Share/Unshare buttons, plus a raw text editor for
/// bulk edits. Returns a note the user moved to the other list.
fn notes_editor(ui: &mut egui::Ui, text: &mut String, shared: bool) -> Option<String> {
    let notes = sidebar::memory_notes(text)
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if notes.is_empty() {
        ui.label(RichText::new("No notes yet.").color(TEXT_MUTED));
    }
    let mut remove = None;
    let mut moved = None;
    ui.spacing_mut().item_spacing.y = 4.0;
    for (index, note) in notes.iter().enumerate() {
        ui.horizontal(|ui| {
            ui.with_layout(Layout::right_to_left(Align::TOP), |ui| {
                if widgets::link_button(ui, "Remove").clicked() {
                    remove = Some(index);
                }
                let (label, hint) = if shared {
                    ("Unshare", "Use this note only in the current workspace")
                } else {
                    ("Share", "Use this note in every workspace")
                };
                if widgets::link_button(ui, label)
                    .on_hover_text(hint)
                    .clicked()
                {
                    remove = Some(index);
                    moved = Some(note.clone());
                }
                ui.with_layout(Layout::left_to_right(Align::TOP), |ui| {
                    ui.add(egui::Label::new(note.as_str()).wrap());
                });
            });
        });
    }
    if let Some(index) = remove {
        *text = notes
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != index)
            .map(|(_, note)| format!("- {note}"))
            .collect::<Vec<_>>()
            .join("\n");
    }
    ui.add_space(6.0);
    egui::CollapsingHeader::new("Edit as text")
        .id_salt(("memory_text", shared))
        .show(ui, |ui| {
            ui.add_sized(
                [ui.available_width(), 110.0],
                egui::TextEdit::multiline(text)
                    .hint_text("Project conventions, important decisions, build commands…"),
            );
        });
    moved
}

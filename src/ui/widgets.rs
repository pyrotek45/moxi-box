use eframe::egui::{self, Align, Color32, Layout, RichText, TextEdit};

use crate::domain::{ChatMessage, MessageRole, Task, WorkspaceEntry};

#[derive(Clone, Copy)]
pub enum ProviderStatus {
    Checking,
    Offline,
    OnlineNoModel,
    Ready,
    Working,
    AwaitingApproval,
}

pub use super::theme::{ACCENT, SURFACE, SURFACE_RAISED, TEXT_MUTED};

pub fn card() -> egui::Frame {
    egui::Frame::new()
        .fill(SURFACE_RAISED)
        .corner_radius(12.0)
        .inner_margin(egui::Margin::same(12))
        .stroke(egui::Stroke::NONE)
}

pub fn primary_button(label: &str) -> egui::Button<'_> {
    egui::Button::new(RichText::new(label).strong().color(Color32::WHITE))
        .fill(ACCENT)
        .corner_radius(8.0)
        .min_size(egui::vec2(72.0, 34.0))
}

pub fn secondary_button(label: &str) -> egui::Button<'_> {
    egui::Button::new(RichText::new(label).strong())
        .fill(SURFACE_RAISED)
        .corner_radius(8.0)
        .stroke(egui::Stroke::NONE)
}

pub fn text_field(ui: &mut egui::Ui, value: &mut String, hint: &str) -> egui::Response {
    egui::Frame::new()
        .fill(SURFACE_RAISED)
        .corner_radius(8.0)
        .inner_margin(egui::Margin::symmetric(9, 3))
        .stroke(egui::Stroke::NONE)
        .show(ui, |ui| {
            ui.add_sized(
                [ui.available_width(), 26.0],
                TextEdit::singleline(value).frame(false).hint_text(hint),
            )
        })
        .inner
}

pub fn model_picker(ui: &mut egui::Ui, selected: &mut Option<String>, models: &[String]) -> bool {
    let before = selected.clone();
    egui::ComboBox::from_id_salt("provider_model_picker")
        .selected_text(selected.as_deref().unwrap_or("Select model"))
        .truncate()
        .width(150.0)
        .show_ui(ui, |ui| {
            for model in models {
                ui.selectable_value(selected, Some(model.clone()), model);
            }
        });
    *selected != before
}

pub fn file_row(ui: &mut egui::Ui, entry: &WorkspaceEntry, selected: bool) -> bool {
    let mut clicked = false;
    ui.scope(|ui| {
        ui.spacing_mut().interact_size.y = 22.0;
        ui.spacing_mut().button_padding = egui::vec2(6.0, 3.0);
        ui.horizontal(|ui| {
            ui.add_space((entry.depth as f32 * 12.0).min(ui.available_width() * 0.3));
            if entry.is_directory {
                ui.add(
                    egui::Label::new(RichText::new(&entry.name).strong().color(TEXT_MUTED))
                        .truncate(),
                )
                .on_hover_text(&entry.path);
            } else {
                clicked = ui
                    .add(
                        egui::Button::new(&entry.name)
                            .selected(selected)
                            .frame(selected)
                            .truncate(),
                    )
                    .clicked();
            }
        });
    });
    clicked
}

pub fn task_row(ui: &mut egui::Ui, task: &Task) -> Option<bool> {
    let row = egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(4, 6))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let (rect, response) =
                    ui.allocate_exact_size(egui::vec2(18.0, 20.0), egui::Sense::click());
                ui.painter().circle_stroke(
                    rect.center(),
                    6.0,
                    egui::Stroke::new(1.3, if task.complete { ACCENT } else { TEXT_MUTED }),
                );
                if task.complete {
                    ui.painter().circle_filled(rect.center(), 3.0, ACCENT);
                }
                let label = ui.add(
                    egui::Label::new(RichText::new(&task.title).color(if task.complete {
                        TEXT_MUTED
                    } else {
                        Color32::from_gray(236)
                    }))
                    .wrap()
                    .sense(egui::Sense::click()),
                );
                response.clicked() || label.clicked()
            })
            .inner
        });
    row.inner.then_some(!task.complete)
}

pub fn status_badge(ui: &mut egui::Ui, status: ProviderStatus) {
    let (color, label) = match status {
        ProviderStatus::Checking => (TEXT_MUTED, "Checking…"),
        ProviderStatus::Offline => (Color32::from_rgb(215, 139, 128), "Disconnected"),
        ProviderStatus::OnlineNoModel => (Color32::from_rgb(222, 185, 105), "Connected · no model"),
        ProviderStatus::Ready => (ACCENT, "Connected"),
        ProviderStatus::Working => (ACCENT, "Working…"),
        ProviderStatus::AwaitingApproval => (Color32::from_rgb(222, 185, 105), "Needs approval"),
    };
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(8.0, 16.0), egui::Sense::hover());
        ui.painter().circle_filled(rect.center(), 3.0, color);
        ui.label(RichText::new(label).size(12.0).color(TEXT_MUTED));
    });
}

pub fn brand(ui: &mut egui::Ui, product: &str) {
    ui.label(RichText::new(product).size(18.0).strong());
}

pub fn mode_pill(ui: &mut egui::Ui, selected: &mut bool, value: bool, label: &str) {
    let active = *selected == value;
    let button = egui::Button::new(RichText::new(label).strong())
        .fill(if active { ACCENT } else { Color32::TRANSPARENT })
        .corner_radius(16.0)
        .stroke(egui::Stroke::new(
            1.0,
            if active {
                ACCENT
            } else {
                Color32::from_rgb(66, 76, 96)
            },
        ));
    if ui.add(button).clicked() {
        *selected = value;
    }
}

pub fn message_bubble(ui: &mut egui::Ui, message: &ChatMessage, streaming: bool) {
    let is_user = message.role == MessageRole::User;
    let width = ui.available_width();
    ui.with_layout(
        if is_user {
            Layout::right_to_left(Align::TOP)
        } else {
            Layout::left_to_right(Align::TOP)
        },
        |ui| {
            egui::Frame::new()
                .fill(if is_user {
                    SURFACE_RAISED
                } else {
                    Color32::TRANSPARENT
                })
                .corner_radius(18.0)
                .inner_margin(egui::Margin::symmetric(14, 11))
                .show(ui, |ui| {
                    ui.with_layout(Layout::top_down(Align::Min), |ui| {
                        ui.set_max_width((width * 0.88 - 28.0).max(1.0));
                        ui.label(
                            RichText::new(message.role.label())
                                .small()
                                .color(TEXT_MUTED),
                        );
                        if message.content.is_empty() && streaming {
                            ui.label(RichText::new("Thinking…").italics().color(TEXT_MUTED));
                        } else {
                            ui.add(egui::Label::new(&message.content).wrap());
                        }
                    });
                });
        },
    );
}

pub struct ComposerResponse {
    pub send: bool,
}

pub fn composer(
    ui: &mut egui::Ui,
    text: &mut String,
    agent_mode: bool,
    working: bool,
) -> ComposerResponse {
    let mut send = false;
    card()
        .corner_radius(24.0)
        .fill(SURFACE_RAISED)
        .show(ui, |ui| {
            let response = egui::ScrollArea::vertical()
                .id_salt("composer_input")
                .auto_shrink([false, false])
                .max_height(52.0)
                .show(ui, |ui| {
                    ui.add_sized(
                        [ui.available_width(), 44.0],
                        TextEdit::multiline(text)
                            .desired_rows(2)
                            .frame(false)
                            .hint_text(if agent_mode {
                                "Describe a coding task…"
                            } else {
                                "Message your local model…"
                            }),
                    )
                })
                .inner;
            let shortcut = response.has_focus()
                && ui.input(|input| {
                    input.key_pressed(egui::Key::Enter)
                        && (input.modifiers.ctrl || input.modifiers.command)
                });
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(if agent_mode {
                        "Agent mode"
                    } else {
                        "Chat mode"
                    })
                    .small()
                    .color(TEXT_MUTED),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    send = ui
                        .add_enabled(
                            !working && !text.trim().is_empty(),
                            primary_button(if working { "Working…" } else { "Send" }),
                        )
                        .clicked()
                        || shortcut;
                    ui.label(
                        RichText::new("Ctrl / Cmd + Enter")
                            .small()
                            .color(TEXT_MUTED),
                    );
                });
            });
        });
    ComposerResponse { send }
}

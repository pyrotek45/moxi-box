use eframe::egui::{self, Align, Color32, Layout, RichText, TextEdit};

use crate::core::{ChatMessage, MessageRole, Task, WorkspaceEntry};

pub const SURFACE: Color32 = Color32::from_rgb(24, 29, 40);
pub const SURFACE_RAISED: Color32 = Color32::from_rgb(34, 41, 55);
pub const ACCENT: Color32 = Color32::from_rgb(78, 139, 230);
pub const TEXT_MUTED: Color32 = Color32::from_rgb(150, 160, 180);

#[derive(Clone, Copy)]
pub enum ProviderStatus {
    Offline,
    OnlineNoModel,
    Ready,
    Working,
}

pub fn install_theme(context: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = Color32::from_rgb(18, 22, 31);
    visuals.window_fill = SURFACE;
    visuals.extreme_bg_color = Color32::from_rgb(13, 16, 23);
    visuals.faint_bg_color = Color32::from_rgb(31, 37, 50);
    visuals.selection.bg_fill = ACCENT;
    visuals.widgets.noninteractive.bg_fill = Color32::from_rgb(28, 34, 47);
    visuals.widgets.inactive.bg_fill = Color32::from_rgb(39, 47, 64);
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(55, 80, 112);
    visuals.widgets.active.bg_fill = ACCENT;
    visuals.window_corner_radius = egui::CornerRadius::same(12);
    visuals.menu_corner_radius = egui::CornerRadius::same(10);
    let mut style = (*context.style()).clone();
    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.spacing.button_padding = egui::vec2(12.0, 7.0);
    style.spacing.interact_size.y = 34.0;
    context.set_style(style);
    context.set_visuals(visuals);
}

pub fn card() -> egui::Frame {
    egui::Frame::new()
        .fill(SURFACE_RAISED)
        .corner_radius(12.0)
        .inner_margin(egui::Margin::same(12))
        .stroke(egui::Stroke::new(1.0, Color32::from_rgb(49, 58, 76)))
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
        .stroke(egui::Stroke::new(1.0, Color32::from_rgb(62, 73, 94)))
}

pub fn text_field(ui: &mut egui::Ui, value: &mut String, hint: &str) -> egui::Response {
    egui::Frame::new()
        .fill(Color32::from_rgb(17, 21, 29))
        .corner_radius(8.0)
        .inner_margin(egui::Margin::symmetric(9, 3))
        .stroke(egui::Stroke::new(1.0, Color32::from_rgb(55, 65, 84)))
        .show(ui, |ui| {
            ui.add(TextEdit::singleline(value).frame(false).hint_text(hint))
        })
        .inner
}

pub fn model_picker(ui: &mut egui::Ui, selected: &mut Option<String>, models: &[String]) -> bool {
    let before = selected.clone();
    egui::ComboBox::from_id_salt("provider_model_picker")
        .selected_text(selected.as_deref().unwrap_or("Select model"))
        .width(180.0)
        .show_ui(ui, |ui| {
            for model in models {
                ui.selectable_value(selected, Some(model.clone()), model);
            }
        });
    *selected != before
}

pub fn file_row(ui: &mut egui::Ui, entry: &WorkspaceEntry, selected: bool) -> bool {
    let mut clicked = false;
    ui.horizontal(|ui| {
        ui.add_space(entry.depth as f32 * 14.0);
        if entry.is_directory {
            ui.label(
                RichText::new(format!("⌄  {}", entry.name))
                    .strong()
                    .color(TEXT_MUTED),
            );
        } else {
            clicked = ui
                .selectable_label(selected, format!("◇  {}", entry.name))
                .clicked();
        }
    });
    clicked
}

pub fn task_row(ui: &mut egui::Ui, task: &Task) -> Option<bool> {
    let mut complete = task.complete;
    let changed = card()
        .inner_margin(egui::Margin::symmetric(9, 6))
        .show(ui, |ui| ui.checkbox(&mut complete, &task.title))
        .inner
        .changed();
    changed.then_some(complete)
}

pub fn status_badge(ui: &mut egui::Ui, status: ProviderStatus) {
    let (color, label) = match status {
        ProviderStatus::Offline => (Color32::from_rgb(230, 99, 92), "Offline"),
        ProviderStatus::OnlineNoModel => (Color32::from_rgb(238, 184, 86), "Select a model"),
        ProviderStatus::Ready => (Color32::from_rgb(74, 205, 128), "Ready"),
        ProviderStatus::Working => (Color32::from_rgb(103, 164, 245), "Working"),
    };
    egui::Frame::new()
        .fill(color.gamma_multiply(0.16))
        .corner_radius(16.0)
        .inner_margin(egui::Margin::symmetric(10, 5))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.colored_label(color, "●");
                ui.label(RichText::new(label).strong().color(color));
            });
        });
}

pub fn brand(ui: &mut egui::Ui, product: &str) {
    egui::Frame::new()
        .fill(ACCENT.gamma_multiply(0.14))
        .corner_radius(10.0)
        .inner_margin(egui::Margin::symmetric(11, 6))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("◈").size(19.0).color(ACCENT));
                ui.label(
                    RichText::new(product)
                        .size(18.0)
                        .strong()
                        .color(Color32::WHITE),
                );
            });
        });
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
    ui.with_layout(
        if is_user {
            Layout::right_to_left(Align::TOP)
        } else {
            Layout::left_to_right(Align::TOP)
        },
        |ui| {
            ui.set_max_width((ui.available_width() * 0.78).max(240.0));
            egui::Frame::new()
                .fill(if is_user {
                    Color32::from_rgb(43, 91, 151)
                } else {
                    SURFACE_RAISED
                })
                .corner_radius(14.0)
                .inner_margin(egui::Margin::symmetric(14, 11))
                .stroke(egui::Stroke::new(
                    1.0,
                    if is_user {
                        Color32::from_rgb(61, 114, 180)
                    } else {
                        Color32::from_rgb(50, 59, 77)
                    },
                ))
                .show(ui, |ui| {
                    ui.label(RichText::new(message.role.label()).small().strong().color(
                        if is_user {
                            Color32::from_rgb(190, 220, 255)
                        } else {
                            Color32::from_rgb(134, 217, 174)
                        },
                    ));
                    ui.add_space(3.0);
                    if message.content.is_empty() && streaming {
                        ui.label(RichText::new("Thinking…").italics().color(TEXT_MUTED));
                    } else {
                        ui.label(&message.content);
                    }
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
    card().fill(SURFACE).show(ui, |ui| {
        let response = ui.add_sized(
            [ui.available_width(), 58.0],
            TextEdit::multiline(text)
                .desired_rows(4)
                .frame(false)
                .hint_text(if agent_mode {
                    "Describe a coding task…"
                } else {
                    "Message your local model…"
                }),
        );
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
                        !working,
                        primary_button(if working { "Working…" } else { "Send  ↑" }),
                    )
                    .clicked()
                    || shortcut;
                ui.label(RichText::new("⌘/Ctrl+Enter").small().color(TEXT_MUTED));
            });
        });
    });
    ComposerResponse { send }
}

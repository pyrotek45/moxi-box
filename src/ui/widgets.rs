use eframe::egui::{self, Align, Color32, Layout, RichText, ScrollArea, TextEdit};

use crate::domain::{ChatMessage, MessageRole, Task, WorkspaceEntry};

#[derive(Clone, Copy)]
pub enum ProviderStatus {
    Checking,
    Offline,
    OnlineNoModel,
    Ready,
    Available,
}

pub use super::theme::{ACCENT, SURFACE, SURFACE_RAISED, TEXT, TEXT_MUTED};

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
        .stroke(egui::Stroke::new(1.0_f32, Color32::from_gray(68)))
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
            if let Some(model) = before.as_ref().filter(|model| !models.contains(model)) {
                ui.selectable_value(selected, Some(model.clone()), model);
            }
            for model in models {
                ui.selectable_value(selected, Some(model.clone()), model);
            }
        });
    *selected != before
}

/// One VS Code-style tree row: chevron for folders, indent guides, flat selection.
/// Returns true when clicked.
pub fn file_row(ui: &mut egui::Ui, entry: &WorkspaceEntry, selected: bool, expanded: bool) -> bool {
    const INDENT: f32 = 12.0;
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 22.0), egui::Sense::click());
    if !ui.is_rect_visible(rect) {
        return response.clicked();
    }
    let painter = ui.painter_at(rect);
    if selected {
        painter.rect_filled(rect, 4.0, SURFACE_RAISED);
    } else if response.hovered() {
        painter.rect_filled(rect, 4.0, Color32::from_white_alpha(6));
    }
    let depth = entry.depth.min(((rect.width() * 0.4) / INDENT) as usize);
    for level in 0..depth {
        let x = rect.left() + 9.5 + level as f32 * INDENT;
        painter.vline(
            x,
            rect.y_range(),
            egui::Stroke::new(1.0_f32, Color32::from_white_alpha(18)),
        );
    }
    let center = egui::pos2(rect.left() + 9.5 + depth as f32 * INDENT, rect.center().y);
    if entry.is_directory {
        let stroke = egui::Stroke::new(1.4_f32, TEXT_MUTED);
        let points = if expanded {
            [
                center + egui::vec2(-3.5, -1.5),
                center + egui::vec2(0.0, 2.0),
                center + egui::vec2(3.5, -1.5),
            ]
        } else {
            [
                center + egui::vec2(-1.5, -3.5),
                center + egui::vec2(2.0, 0.0),
                center + egui::vec2(-1.5, 3.5),
            ]
        };
        painter.line(points.to_vec(), stroke);
    }
    let text_left = center.x + 9.0;
    let galley = ui.painter().layout_no_wrap(
        entry.name.clone(),
        egui::FontId::proportional(13.0),
        if selected { Color32::WHITE } else { TEXT },
    );
    let text_rect = egui::Rect::from_x_y_ranges(
        text_left..=(rect.right() - 4.0).max(text_left),
        rect.y_range(),
    );
    painter.with_clip_rect(text_rect).galley(
        egui::pos2(text_left, rect.center().y - galley.size().y / 2.0),
        galley,
        TEXT,
    );
    response.on_hover_text(&entry.path).clicked()
}

/// A compact checklist row; returns the new completion state when clicked.
pub fn task_row(ui: &mut egui::Ui, task: &Task) -> Option<bool> {
    use crate::domain::TaskStatus;
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 22.0), egui::Sense::click());
    let painter = ui.painter_at(rect);
    if response.hovered() {
        painter.rect_filled(rect, 4.0, Color32::from_white_alpha(6));
    }
    let icon = egui::pos2(rect.left() + 10.0, rect.center().y);
    match task.status {
        TaskStatus::Completed => {
            painter.circle_filled(icon, 6.0, ACCENT);
            painter.line(
                vec![
                    icon + egui::vec2(-2.8, 0.0),
                    icon + egui::vec2(-0.8, 2.2),
                    icon + egui::vec2(2.8, -2.2),
                ],
                egui::Stroke::new(1.5_f32, Color32::WHITE),
            );
        }
        TaskStatus::InProgress => {
            painter.circle_stroke(icon, 5.5, egui::Stroke::new(1.3_f32, Color32::LIGHT_YELLOW));
            painter.circle_filled(icon, 2.5, Color32::LIGHT_YELLOW);
        }
        TaskStatus::Pending => {
            painter.circle_stroke(icon, 5.5, egui::Stroke::new(1.3_f32, TEXT_MUTED));
        }
    }
    let done = task.status == TaskStatus::Completed;
    let galley = ui.painter().layout_no_wrap(
        task.title.clone(),
        egui::FontId::proportional(13.0),
        if done { TEXT_MUTED } else { TEXT },
    );
    let text_rect = egui::Rect::from_x_y_ranges(
        (icon.x + 12.0)..=(rect.right() - 4.0).max(icon.x + 12.0),
        rect.y_range(),
    );
    let text_pos = egui::pos2(text_rect.left(), rect.center().y - galley.size().y / 2.0);
    let width = galley.size().x.min(text_rect.width());
    painter
        .with_clip_rect(text_rect)
        .galley(text_pos, galley, TEXT);
    if done {
        painter.hline(
            text_rect.left()..=text_rect.left() + width,
            rect.center().y + 0.5,
            egui::Stroke::new(1.0_f32, TEXT_MUTED),
        );
    }
    let response = response.on_hover_text(format!(
        "{} · {}",
        task.title,
        task.status.as_str().replace('_', " ")
    ));
    response.clicked().then_some(!done)
}

pub fn status_badge(ui: &mut egui::Ui, status: ProviderStatus) {
    let (color, label) = match status {
        ProviderStatus::Checking => (TEXT_MUTED, "Checking…"),
        ProviderStatus::Offline => (Color32::from_rgb(215, 139, 128), "Disconnected"),
        ProviderStatus::OnlineNoModel => (Color32::from_rgb(222, 185, 105), "Connected · no model"),
        ProviderStatus::Ready => (ACCENT, "Connected"),
        ProviderStatus::Available => (TEXT_MUTED, "Available"),
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
            1.0_f32,
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
    bubble(ui, message.id, message.role, |ui| {
        if message.content.is_empty() && streaming {
            working_indicator(ui, "Thinking");
        } else {
            message_text(ui, &message.content);
        }
    });
}

/// A painted gear icon button.
pub fn cog_button(ui: &mut egui::Ui) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(32.0, 32.0), egui::Sense::click());
    let painter = ui.painter_at(rect);
    if response.hovered() {
        painter.rect_filled(rect, 8.0, SURFACE_RAISED);
    }
    let center = rect.center();
    let color = if response.hovered() { TEXT } else { TEXT_MUTED };
    for tooth in 0..8 {
        let angle = tooth as f32 * std::f32::consts::TAU / 8.0;
        let direction = egui::vec2(angle.cos(), angle.sin());
        painter.line_segment(
            [center + direction * 5.5, center + direction * 8.5],
            egui::Stroke::new(3.0_f32, color),
        );
    }
    painter.circle_stroke(center, 5.5, egui::Stroke::new(2.0_f32, color));
    painter.circle_filled(center, 2.0, color);
    response
}

#[derive(Clone, Copy, PartialEq)]
pub enum StepStatus {
    Running,
    Waiting,
    Done,
    Failed,
}

/// A rounded tool-step row with a painted status icon; click to show its detail.
pub fn tool_step(
    ui: &mut egui::Ui,
    title: &str,
    detail: &str,
    status: StepStatus,
    open_by_default: bool,
) {
    let id = ui.id().with("open");
    // Only an explicit click is remembered, so a step that becomes an edit opens to show its diff.
    let mut open = ui.data(|data| data.get_temp(id)).unwrap_or(open_by_default);
    let expandable = !detail.trim().is_empty();
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), 30.0),
        if expandable {
            egui::Sense::click()
        } else {
            egui::Sense::hover()
        },
    );
    let painter = ui.painter_at(rect);
    let fill = if response.hovered() && expandable {
        Color32::from_gray(52)
    } else {
        Color32::from_gray(36)
    };
    painter.rect_filled(rect, 8.0, fill);
    let icon = egui::pos2(rect.left() + 16.0, rect.center().y);
    match status {
        StepStatus::Running => {
            let time = ui.input(|input| input.time) as f32;
            let points = (0..=16)
                .map(|index| {
                    let angle = time * 5.0 + index as f32 / 16.0 * std::f32::consts::PI * 1.4;
                    icon + 5.5 * egui::vec2(angle.cos(), angle.sin())
                })
                .collect::<Vec<_>>();
            painter.add(egui::Shape::line(
                points,
                egui::Stroke::new(1.8_f32, ACCENT),
            ));
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(30));
        }
        StepStatus::Waiting => {
            painter.circle_filled(icon, 4.5, Color32::from_rgb(234, 179, 8));
        }
        StepStatus::Done => {
            painter.circle_filled(icon, 7.0, ACCENT.gamma_multiply(0.25));
            painter.line(
                vec![
                    icon + egui::vec2(-3.0, 0.0),
                    icon + egui::vec2(-0.8, 2.4),
                    icon + egui::vec2(3.2, -2.4),
                ],
                egui::Stroke::new(1.6_f32, ACCENT),
            );
        }
        StepStatus::Failed => {
            painter.circle_filled(
                icon,
                7.0,
                Color32::from_rgb(220, 38, 38).gamma_multiply(0.3),
            );
            let stroke = egui::Stroke::new(1.6_f32, Color32::from_rgb(248, 113, 113));
            painter.line_segment(
                [icon + egui::vec2(-2.5, -2.5), icon + egui::vec2(2.5, 2.5)],
                stroke,
            );
            painter.line_segment(
                [icon + egui::vec2(-2.5, 2.5), icon + egui::vec2(2.5, -2.5)],
                stroke,
            );
        }
    }
    let right = if expandable {
        rect.right() - 28.0
    } else {
        rect.right() - 10.0
    };
    let text_rect = egui::Rect::from_x_y_ranges(
        (rect.left() + 32.0)..=right.max(rect.left() + 32.0),
        rect.y_range(),
    );
    let color = match status {
        StepStatus::Failed => Color32::from_rgb(248, 113, 113),
        StepStatus::Waiting => Color32::from_rgb(250, 204, 21),
        _ => TEXT,
    };
    let galley =
        ui.painter()
            .layout_no_wrap(title.to_owned(), egui::FontId::proportional(13.0), color);
    painter.with_clip_rect(text_rect).galley(
        egui::pos2(text_rect.left(), rect.center().y - galley.size().y / 2.0),
        galley,
        color,
    );
    if expandable {
        let chevron = egui::pos2(rect.right() - 14.0, rect.center().y);
        let points = if open {
            vec![
                chevron + egui::vec2(-3.5, -1.5),
                chevron + egui::vec2(0.0, 2.0),
                chevron + egui::vec2(3.5, -1.5),
            ]
        } else {
            vec![
                chevron + egui::vec2(-1.5, -3.5),
                chevron + egui::vec2(2.0, 0.0),
                chevron + egui::vec2(-1.5, 3.5),
            ]
        };
        painter.line(points, egui::Stroke::new(1.4_f32, TEXT_MUTED));
    }
    if response.clicked() {
        open = !open;
        ui.data_mut(|data| data.insert_temp(id, open));
    }
    response.on_hover_text(title);
    if open && expandable {
        ui.add_space(2.0);
        code_view(ui, "detail", detail);
    }
}

/// Dark monospace panel; lines starting with "+ " or "- " are tinted as a diff.
pub fn code_view(ui: &mut egui::Ui, id: &str, text: &str) {
    egui::Frame::new()
        .fill(Color32::from_gray(22))
        .corner_radius(8.0)
        .inner_margin(egui::Margin::symmetric(0, 6))
        .show(ui, |ui| {
            ScrollArea::vertical()
                .id_salt(id)
                .max_height(240.0)
                .min_scrolled_height(240.0)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for line in text.lines().take(2000) {
                        let (fill, color) = if line.starts_with("+ ") {
                            (
                                Color32::from_rgb(24, 52, 34),
                                Color32::from_rgb(134, 239, 172),
                            )
                        } else if line.starts_with("- ") {
                            (
                                Color32::from_rgb(64, 28, 30),
                                Color32::from_rgb(252, 165, 165),
                            )
                        } else if line.starts_with("$ ") {
                            (Color32::TRANSPARENT, TEXT)
                        } else {
                            (Color32::TRANSPARENT, Color32::from_gray(190))
                        };
                        egui::Frame::new()
                            .fill(fill)
                            .inner_margin(egui::Margin::symmetric(10, 1))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(line).monospace().size(12.0).color(color),
                                    )
                                    .wrap(),
                                );
                            });
                    }
                });
        });
}

/// Three pulsing painted dots and a label, shown while a run is active.
pub fn working_indicator(ui: &mut egui::Ui, label: &str) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(26.0, 16.0), egui::Sense::hover());
        let time = ui.input(|input| input.time) as f32;
        for index in 0..3 {
            let phase = (time * 4.0 - index as f32 * 0.7).sin() * 0.5 + 0.5;
            ui.painter().circle_filled(
                egui::pos2(rect.left() + 4.0 + index as f32 * 9.0, rect.center().y),
                2.0 + phase * 1.2,
                ACCENT.gamma_multiply(0.35 + phase * 0.65),
            );
        }
        ui.label(RichText::new(label).color(TEXT_MUTED));
    });
    ui.ctx()
        .request_repaint_after(std::time::Duration::from_millis(50));
}

/// A chat bubble whose body is drawn by the caller, e.g. text interleaved with tool steps.
pub fn bubble(ui: &mut egui::Ui, id: i64, role: MessageRole, body: impl FnOnce(&mut egui::Ui)) {
    let is_user = role == MessageRole::User;
    let width = (ui.available_width() * if is_user { 0.86 } else { 0.96 }).min(780.0);
    ui.push_id(id, |ui| {
        ui.with_layout(
            if is_user {
                Layout::right_to_left(Align::TOP)
            } else {
                Layout::left_to_right(Align::TOP)
            },
            |ui| {
                ui.set_max_width(width);
                egui::Frame::new()
                    .fill(if is_user {
                        Color32::from_rgb(44, 57, 53)
                    } else {
                        Color32::from_gray(42)
                    })
                    .corner_radius(18.0)
                    .inner_margin(egui::Margin::symmetric(16, 14))
                    .show(ui, |ui| {
                        ui.with_layout(Layout::top_down(Align::Min), |ui| {
                            ui.set_max_width((width - 32.0).max(1.0));
                            ui.label(RichText::new(role.label()).size(12.0).strong().color(
                                if is_user {
                                    Color32::from_rgb(160, 199, 183)
                                } else {
                                    TEXT_MUTED
                                },
                            ));
                            ui.add_space(4.0);
                            body(ui);
                        });
                    });
            },
        );
    });
}

pub fn message_text(ui: &mut egui::Ui, content: &str) {
    for (index, part) in content.split("```").enumerate() {
        if index % 2 == 1 {
            let (language, code) = part.split_once('\n').unwrap_or(("", part));
            egui::Frame::new()
                .fill(Color32::from_gray(25))
                .corner_radius(8)
                .inner_margin(10)
                .show(ui, |ui| {
                    if !language.trim().is_empty() {
                        ui.label(RichText::new(language).small().color(TEXT_MUTED));
                    }
                    ScrollArea::horizontal()
                        .id_salt(("code", index))
                        .show(ui, |ui| {
                            ui.monospace(code.trim_end());
                        });
                });
        } else {
            for paragraph in part.trim().split("\n\n").filter(|text| !text.is_empty()) {
                ui.add(egui::Label::new(RichText::new(paragraph).size(16.0)).wrap());
                ui.add_space(4.0);
            }
        }
    }
}

pub struct ComposerResponse {
    pub send: bool,
    pub stop: bool,
    pub history: Option<bool>,
    pub edited: bool,
}

pub fn composer(
    ui: &mut egui::Ui,
    text: &mut String,
    agent_mode: bool,
    working: bool,
    browsing_history: bool,
) -> ComposerResponse {
    let mut send = false;
    let mut stop = false;
    let mut history = None;
    let mut edited = false;
    card()
        .corner_radius(24.0)
        .fill(SURFACE_RAISED)
        .show(ui, |ui| {
            let input_id = egui::Id::new("composer_text");
            if ui.memory(|memory| memory.has_focus(input_id))
                && (!text.contains('\n') || browsing_history)
            {
                history = ui.input_mut(|input| {
                    if input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp) {
                        Some(true)
                    } else if input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown) {
                        Some(false)
                    } else {
                        None
                    }
                });
            }
            // Ctrl+Enter also works when focus has moved off the input to nothing else, e.g. after clicking Send.
            let shortcut = ui
                .memory(|memory| memory.has_focus(input_id) || memory.focused().is_none())
                && ui.input_mut(|input| {
                    input.consume_key(egui::Modifiers::CTRL, egui::Key::Enter)
                        || input.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter)
                });
            let response = egui::ScrollArea::vertical()
                .id_salt("composer_input")
                .auto_shrink([false, false])
                .max_height(52.0)
                .show(ui, |ui| {
                    ui.add_sized(
                        [ui.available_width(), 44.0],
                        TextEdit::multiline(text)
                            .id(input_id)
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
            edited = response.changed();
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
                            !text.trim().is_empty(),
                            primary_button(if working { "Queue" } else { "Send" }),
                        )
                        .clicked()
                        || (shortcut && !text.trim().is_empty());
                    if working {
                        stop = ui.add(secondary_button("Stop")).clicked();
                    }
                    if ui.available_width() > 190.0 && !working {
                        ui.label(
                            RichText::new("Up/Down history · Ctrl + Enter")
                                .small()
                                .color(TEXT_MUTED),
                        );
                    }
                });
            });
        });
    if send {
        ui.memory_mut(|memory| memory.request_focus(egui::Id::new("composer_text")));
    }
    ComposerResponse {
        send,
        stop,
        history,
        edited,
    }
}

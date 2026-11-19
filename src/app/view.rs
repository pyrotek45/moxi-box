use eframe::egui::{self, Align, Color32, Layout, RichText, ScrollArea};

use super::MoxiBoxApp;
use crate::{
    domain::*,
    ui::widgets::{self, SURFACE, TEXT_MUTED},
};

mod conversation;
mod dialogs;
mod header;
mod sidebar;

impl MoxiBoxApp {
    pub(super) fn render(&mut self, context: &egui::Context) {
        self.handle_events(context);
        self.poll_folder_picker();
        self.poll_file_dialog();
        self.tick_connection(context);
        self.finish_connect_and_send();
        self.refresh_tree(context);
        // Worker events wake egui directly; idle windows do not need polling redraws. During a
        // run, redraw every frame so stick-to-bottom scrolling settles at once instead of
        // showing a cut-off frame until the next streamed token arrives.
        if self.run_pending {
            context.request_repaint_after(std::time::Duration::from_millis(50));
        }
        self.header(context);
        if self.show_workspace {
            self.sidebar(context);
        }
        self.conversation(context);
        self.dialogs(context);
    }
}

impl MoxiBoxApp {
    /// A clear warning above the composer when Send was pressed without a usable model.
    fn send_blocked_bar(&mut self, ui: &mut egui::Ui) {
        use crate::app::SendBlocked;
        // Clear the warning once the cause is gone.
        let ready = self.connection.connected && self.settings.selected_model.is_some();
        if ready
            || (self.send_blocked == Some(SendBlocked::Reconnecting)
                && self.connection.reconnect.is_none()
                && !self.connection.connected)
        {
            self.send_blocked = if ready {
                None
            } else {
                Some(SendBlocked::NotConnected)
            };
            if ready {
                return;
            }
        }
        let Some(blocked) = self.send_blocked else {
            return;
        };
        let red = Color32::from_rgb(239, 120, 110);
        egui::Frame::new()
            .fill(Color32::from_rgb(58, 34, 32))
            .stroke(egui::Stroke::new(1.0_f32, red))
            .corner_radius(10.0)
            .inner_margin(12)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                let (title, detail) = match blocked {
                    SendBlocked::NotConnected => ("Not connected to a model", "Your message was not sent. Start your model server, then click Connect."),
                    SendBlocked::Reconnecting => ("Connection lost — reconnecting", "Your message was not sent. It stays here; press Send again once Moxi reconnects."),
                    SendBlocked::NoModel => ("No model selected", "Your message was not sent. Choose a model in the header, then press Send again."),
                };
                ui.horizontal(|ui| {
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 18.0), egui::Sense::hover());
                    ui.painter().circle_filled(rect.center(), 4.5, red);
                    ui.label(RichText::new(title).strong().color(red));
                });
                ui.add(egui::Label::new(RichText::new(detail).small().color(widgets::TEXT)).wrap());
                ui.horizontal_wrapped(|ui| {
                    match blocked {
                        SendBlocked::NotConnected => {
                            let checking = self.connection.checking;
                            if ui.add_enabled(!checking, widgets::primary_button(if checking { "Connecting…" } else { "Connect" })).clicked() {
                                self.refresh_models(true);
                            }
                        }
                        SendBlocked::Reconnecting => {
                            if ui.add_enabled(!self.connection.checking, widgets::primary_button("Retry now")).clicked() {
                                self.retry_connection();
                            }
                        }
                        SendBlocked::NoModel => {}
                    }
                    if ui.add(widgets::secondary_button("Provider settings")).clicked() {
                        self.provider_draft = Some(self.settings.clone());
                        self.provider_form_error = None;
                    }
                    if widgets::link_button(ui, "Dismiss").clicked() {
                        self.send_blocked = None;
                    }
                });
            });
        ui.add_space(6.0);
    }

    /// Shown above the composer when a Chat-mode message looks like it needs files or commands.
    fn mode_hint_bar(&mut self, ui: &mut egui::Ui) {
        widgets::card().show(ui, |ui| {
            ui.add(
                egui::Label::new(
                    RichText::new("This looks like a coding task. Chat mode can't read or change files or run commands; Agent mode can.")
                        .small()
                        .color(Color32::from_rgb(222, 185, 105)),
                )
                .wrap(),
            );
            ui.horizontal_wrapped(|ui| {
                if ui.add(widgets::primary_button("Switch to Agent and send")).clicked() {
                    self.mode_hint = false;
                    self.agent_mode = true;
                    self.send_message();
                }
                if ui.add(widgets::secondary_button("Send in Chat anyway")).clicked() {
                    self.mode_hint = false;
                    self.chat_confirmed = true;
                    self.send_message();
                }
                if widgets::link_button(ui, "Dismiss").clicked() {
                    self.mode_hint = false;
                }
            });
        });
        ui.add_space(6.0);
    }
}

impl eframe::App for MoxiBoxApp {
    fn update(&mut self, context: &egui::Context, _: &mut eframe::Frame) {
        self.render(context);
    }
}

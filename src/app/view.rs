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

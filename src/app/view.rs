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
        self.refresh_tree(context);
        // Worker events wake egui directly; idle windows do not need polling redraws. During a
        // run, redraw every frame so stick-to-bottom scrolling settles at once instead of
        // showing a cut-off frame until the next streamed token arrives.
        if self.run_pending {
            context.request_repaint();
        }
        self.header(context);
        if self.show_workspace {
            self.sidebar(context);
        }
        self.conversation(context);
        self.dialogs(context);
    }
}

impl eframe::App for MoxiBoxApp {
    fn update(&mut self, context: &egui::Context, _: &mut eframe::Frame) {
        self.render(context);
    }
}

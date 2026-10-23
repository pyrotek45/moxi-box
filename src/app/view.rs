use std::time::Duration;

use eframe::egui::{self, Align, Color32, Layout, RichText, ScrollArea};

use super::MoxiBoxApp;
use crate::{
    domain::*,
    ui::widgets::{self, SURFACE, TEXT_MUTED},
};

mod conversation;
mod header;
mod sidebar;

impl MoxiBoxApp {
    pub(super) fn render(&mut self, context: &egui::Context) {
        self.handle_events(context);
        // The backend uses channels; repaint even while idle to receive task and connection updates.
        context.request_repaint_after(Duration::from_millis(100));
        self.header(context);
        if self.show_workspace {
            self.sidebar(context);
        }
        self.conversation(context);
    }
}

impl eframe::App for MoxiBoxApp {
    fn update(&mut self, context: &egui::Context, _: &mut eframe::Frame) {
        self.render(context);
    }
}

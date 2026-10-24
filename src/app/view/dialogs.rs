use super::*;
use crate::app::workspaces::WorkspaceDialog;

mod memory;
mod provider;
mod settings;
mod workspace;

impl MoxiBoxApp {
    pub(super) fn dialogs(&mut self, context: &egui::Context) {
        let width = (context.screen_rect().width() - 80.0).clamp(240.0, 440.0);
        let height = (context.screen_rect().height() - 150.0).max(160.0);
        self.provider_dialog(context, width, height);
        self.workspace_dialog(context, width, height);
        self.memory_dialog(context, width, height);
        self.settings_dialog(context, width, height);
    }
}

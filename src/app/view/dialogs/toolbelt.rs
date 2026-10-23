use super::*;

impl MoxiBoxApp {
    pub(super) fn toolbelt_dialog(&mut self, context: &egui::Context, width: f32, height: f32) {
        if !self.show_toolbelt {
            return;
        }
        let mut close = false;
        let response = egui::Modal::new(egui::Id::new("toolbelt_help")).frame(widgets::card().inner_margin(24)).show(context, |ui| {
            ui.set_width(width); ui.heading("Using the toolbelt");
            ScrollArea::vertical().max_height(height - 45.0).show(ui, |ui| {
                ui.label("1. Open a workspace and connect to a model that supports tool calling.");
                ui.label("2. Select Agent and describe the outcome you want. The agent chooses tools and shows each step in the chat.");
                ui.label("3. Review each requested write, command, or memory note. Approve runs it once; Deny stops the run. Use Stop to interrupt a slow request or command.");
                ui.add_space(8.0);
                for (name,help) in [
                    ("List files", "Inspect workspace filenames. Automatic."),
                    ("Read file", "Read a UTF-8 workspace file, up to 256 KiB. Automatic."),
                    ("Search content", "Find matching lines in workspace text files. Automatic."),
                    ("Write file", "Create or replace a workspace file. Requires approval."),
                    ("Run command", "Run an executable and arguments from the workspace. Requires approval. Commands have your machine permissions; the working directory is not an OS sandbox."),
                    ("Remember", "Save a concise project fact for later conversations. Requires approval. Review or edit saved notes using Memory."),
                ] { ui.label(RichText::new(name).strong()); ui.label(help); }
                ui.add_space(8.0);
                ui.label(RichText::new("Try asking").strong());
                ui.label("Find where settings are loaded, explain it, then add a missing validation check and run the tests.");
                ui.label("Expand a tool step to inspect its arguments and result. Large outputs are capped; narrower searches and smaller file reads keep context useful.");
                ui.label("Chat answers without tools. New chat starts a separate conversation; workspace memory carries over. Long histories are summarized automatically, while full logs remain saved.");
            });
            ui.add_space(8.0); close = ui.add(widgets::primary_button("Got it")).clicked();
        });
        if close || response.should_close() {
            self.show_toolbelt = false;
        }
    }
}

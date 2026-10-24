use super::*;

#[derive(Clone, Copy, PartialEq)]
pub(super) enum WorkspaceDialog {
    New,
    Open,
    Remove,
    Reset,
}

impl MoxiBoxApp {
    pub(super) fn show_workspace_dialog(
        &mut self,
        dialog: WorkspaceDialog,
        context: &egui::Context,
    ) {
        if self.run_pending || self.workspace_pending || self.folder_picker.is_some() {
            return;
        }
        self.workspace_error = None;
        self.workspace_name.clear();
        if matches!(dialog, WorkspaceDialog::Remove | WorkspaceDialog::Reset) {
            self.workspace_dialog = Some(dialog);
            return;
        }
        let root = if self.settings.workspace_root.is_empty() {
            std::env::current_dir().unwrap_or_default()
        } else {
            std::path::PathBuf::from(&self.settings.workspace_root)
        };
        let (sender, receiver) = mpsc::channel();
        self.folder_picker = Some((dialog, receiver));
        let context = context.clone();
        std::thread::spawn(move || {
            let selection = rfd::FileDialog::new()
                .set_title(if dialog == WorkspaceDialog::New {
                    "Choose parent folder for new workspace"
                } else {
                    "Open workspace"
                })
                .set_directory(root)
                .pick_folder();
            let _ = sender.send(selection);
            context.request_repaint();
        });
    }

    pub(super) fn poll_folder_picker(&mut self) {
        let Some((dialog, receiver)) = &self.folder_picker else {
            return;
        };
        let dialog = *dialog;
        let selection = match receiver.try_recv() {
            Ok(selection) => selection,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => {
                self.folder_picker = None;
                self.notice = Some("The system folder chooser could not open. Check your desktop file-dialog service.".to_owned());
                return;
            }
        };
        self.folder_picker = None;
        if let Some(path) = selection {
            let path = path.to_string_lossy().into_owned();
            if dialog == WorkspaceDialog::New {
                self.workspace_path = path;
                self.workspace_dialog = Some(WorkspaceDialog::New);
            } else {
                self.open_workspace(path, false);
            }
        }
    }
    pub(super) fn remember_workspace(&mut self) {
        let root = &self.settings.workspace_root;
        if !root.is_empty() && !self.settings.workspaces.contains(root) {
            self.settings.workspaces.push(root.clone());
        }
    }
    pub(super) fn workspace_opened(
        &mut self,
        result: Result<(String, Vec<WorkspaceEntry>), String>,
    ) {
        self.workspace_pending = false;
        match result {
            Ok((root, entries)) => {
                self.settings.workspace_root = root;
                self.workspace_entries = entries;
                self.preview = None;
                self.remember_workspace();
                self.command(AppCommand::SaveSettings(self.settings.clone()));
                self.workspace_dialog = None;
                self.workspace_error = None;
            }
            Err(error) => {
                self.workspace_error = Some(error.clone());
                self.notice = Some(error);
            }
        }
    }
    pub(super) fn open_workspace(&mut self, path: String, create: bool) {
        if self.run_pending || self.workspace_pending {
            return;
        }
        self.workspace_pending = true;
        self.workspace_error = None;
        self.command(AppCommand::OpenWorkspace { path, create });
    }
    pub(super) fn remove_workspace(&mut self) {
        if self.run_pending || self.workspace_pending {
            return;
        }
        let root = self.settings.workspace_root.clone();
        self.settings.workspaces.retain(|path| path != &root);
        self.settings.workspace_root = String::new();
        self.workspace_entries.clear();
        self.messages.clear();
        self.tasks.clear();
        self.activity.clear();
        self.conversations.clear();
        self.conversation_id = None;
        self.summary.clear();
        self.memory.clear();
        self.preview = None;
        self.workspace_dialog = None;
        self.command(AppCommand::SaveSettings(self.settings.clone()));
        self.notice = Some("Workspace removed from the list. Files were kept on disk.".to_owned());
    }
}

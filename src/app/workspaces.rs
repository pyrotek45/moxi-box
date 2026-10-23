use super::*;

#[derive(Clone, Copy, PartialEq)]
pub(super) enum WorkspaceDialog {
    New,
    Open,
    Remove,
    Reset,
}

impl MoxiBoxApp {
    pub(super) fn show_workspace_dialog(&mut self, dialog: WorkspaceDialog) {
        self.workspace_dialog = Some(dialog);
        self.workspace_error = None;
        self.workspace_name.clear();
        let root = if self.settings.workspace_root.is_empty() {
            std::env::current_dir()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        } else {
            self.settings.workspace_root.clone()
        };
        if matches!(dialog, WorkspaceDialog::New | WorkspaceDialog::Open) {
            self.browse_folders(root);
        }
    }
    pub(super) fn browse_folders(&mut self, path: String) {
        self.folder_request += 1;
        self.folder_loading = true;
        self.workspace_error = None;
        self.command(AppCommand::ListFolders {
            path,
            request_id: self.folder_request,
        });
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

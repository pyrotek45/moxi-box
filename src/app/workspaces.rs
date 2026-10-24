use super::*;

#[derive(Clone, Copy, PartialEq)]
pub(super) enum WorkspaceDialog {
    /// Name a new folder inside the chosen parent.
    NewFolder,
    /// Name a new workspace (None) or rename an existing one.
    Name(Option<i64>),
    Delete(i64),
    Reset,
}

/// What the native folder chooser was opened for.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum FolderPick {
    Attach,
    NewFolderParent,
}

impl MoxiBoxApp {
    pub(super) fn idle(&self) -> bool {
        !self.run_pending
            && !self.workspace_pending
            && !self.switching
            && self.folder_picker.is_none()
    }

    /// Open the name dialog for a new workspace (None) or a rename.
    pub(super) fn name_workspace(&mut self, id: Option<i64>) {
        if !self.idle() {
            return;
        }
        self.workspace_error = None;
        self.workspace_name = id
            .and_then(|id| self.workspaces.iter().find(|workspace| workspace.id == id))
            .map(|workspace| workspace.name.clone())
            .unwrap_or_default();
        self.workspace_dialog = Some(WorkspaceDialog::Name(id));
    }

    pub(super) fn select_workspace(&mut self, id: i64) {
        if self.idle() && self.workspace_id != Some(id) {
            self.switching = true;
            self.command(AppCommand::SelectWorkspace(id));
        }
    }

    /// The native chooser runs off the render thread; cancelling keeps the current folder.
    pub(super) fn pick_folder(&mut self, pick: FolderPick, context: &egui::Context) {
        if !self.idle() {
            return;
        }
        self.workspace_error = None;
        self.workspace_name.clear();
        let start = if self.settings.workspace_root.is_empty() {
            std::env::var_os("HOME")
                .map(std::path::PathBuf::from)
                .unwrap_or_default()
        } else {
            std::path::PathBuf::from(&self.settings.workspace_root)
        };
        let (sender, receiver) = mpsc::channel();
        self.folder_picker = Some((pick, receiver));
        let context = context.clone();
        std::thread::spawn(move || {
            let selection = rfd::FileDialog::new()
                .set_title(match pick {
                    FolderPick::Attach => "Attach a folder",
                    FolderPick::NewFolderParent => "Choose where to create the folder",
                })
                .set_directory(start)
                .pick_folder();
            let _ = sender.send(selection);
            context.request_repaint();
        });
    }

    pub(super) fn poll_folder_picker(&mut self) {
        let Some((pick, receiver)) = &self.folder_picker else {
            return;
        };
        let pick = *pick;
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
            match pick {
                FolderPick::NewFolderParent => {
                    self.workspace_path = path;
                    self.workspace_dialog = Some(WorkspaceDialog::NewFolder);
                }
                FolderPick::Attach => self.attach_folder(path, false),
            }
        }
    }

    /// Attach a folder to the current workspace; an empty path detaches it.
    pub(super) fn attach_folder(&mut self, path: String, create: bool) {
        if self.run_pending || self.workspace_pending {
            return;
        }
        self.workspace_pending = true;
        self.workspace_error = None;
        self.command(AppCommand::AttachFolder { path, create });
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
                self.workspace_dialog = None;
                self.workspace_error = None;
            }
            Err(error) => {
                self.workspace_error = Some(error.clone());
                self.notice = Some(error);
            }
        }
    }
}

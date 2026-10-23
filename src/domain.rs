use serde::{Deserialize, Serialize};

pub const APPLICATION_NAME: &str = "Moxi Box";
pub const DEFAULT_LM_STUDIO_URL: &str = "http://127.0.0.1:1234";
pub const DEFAULT_WINDOW_WIDTH: f32 = 1_280.0;
pub const DEFAULT_WINDOW_HEIGHT: f32 = 800.0;
pub const MAX_STORED_MESSAGES: usize = 500;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub lm_studio_url: String,
    pub selected_model: Option<String>,
    #[serde(default = "default_workspace_root")]
    pub workspace_root: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            lm_studio_url: DEFAULT_LM_STUDIO_URL.to_owned(),
            selected_model: None,
            workspace_root: default_workspace_root(),
        }
    }
}

fn default_workspace_root() -> String {
    std::env::current_dir()
        .unwrap_or_default()
        .display()
        .to_string()
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum MessageRole {
    User,
    Assistant,
    System,
}

impl MessageRole {
    pub fn label(self) -> &'static str {
        match self {
            Self::User => "You",
            Self::Assistant => "Moxi",
            Self::System => "System",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: i64,
    pub role: MessageRole,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: i64,
    pub title: String,
    pub complete: bool,
}

#[derive(Debug, Clone)]
pub struct WorkspaceEntry {
    pub path: String,
    pub name: String,
    pub depth: usize,
    pub is_directory: bool,
}

#[derive(Debug, Clone)]
pub enum AppCommand {
    Load,
    SaveSettings(Settings),
    RefreshModels {
        base_url: String,
    },
    BrowseWorkspace {
        root: String,
    },
    ReadPreview {
        root: String,
        path: String,
    },
    AddTask(String),
    SetTaskComplete {
        id: i64,
        complete: bool,
    },
    SendChat {
        settings: Settings,
        content: String,
        agent_mode: bool,
    },
    ResolveApproval {
        request_id: String,
        approved: bool,
    },
}

#[derive(Debug, Clone)]
pub enum AppEvent {
    Loaded {
        settings: Settings,
        tasks: Vec<Task>,
        messages: Vec<ChatMessage>,
    },
    ModelsLoaded(Vec<String>),
    ConnectionChanged(bool),
    WorkspaceLoaded(Vec<WorkspaceEntry>),
    PreviewLoaded {
        path: String,
        content: String,
    },
    ChatUser(ChatMessage),
    ChatStarted {
        id: i64,
    },
    ChatDelta {
        id: i64,
        text: String,
    },
    ChatFinished {
        id: i64,
    },
    ToolActivity(String),
    ApprovalRequested {
        request_id: String,
        summary: String,
    },
    TaskAdded(Task),
    TaskChanged {
        id: i64,
        complete: bool,
    },
    Notice(String),
    Error(String),
    RunFailed(String),
}

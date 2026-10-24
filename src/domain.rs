use serde::{Deserialize, Serialize};

pub const APPLICATION_NAME: &str = "Moxi Box";
pub const DEFAULT_LM_STUDIO_URL: &str = "http://127.0.0.1:1234";
pub const DEFAULT_WINDOW_WIDTH: f32 = 1_280.0;
pub const DEFAULT_WINDOW_HEIGHT: f32 = 800.0;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default = "default_provider_name")]
    pub provider_name: String,
    #[serde(default = "default_context_budget")]
    pub context_budget: usize,
    pub lm_studio_url: String,
    pub selected_model: Option<String>,
    #[serde(default = "default_workspace_root")]
    pub workspace_root: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            provider_name: default_provider_name(),
            context_budget: default_context_budget(),
            lm_studio_url: DEFAULT_LM_STUDIO_URL.to_owned(),
            selected_model: None,
            workspace_root: default_workspace_root(),
        }
    }
}

fn default_context_budget() -> usize {
    8192
}

fn default_provider_name() -> String {
    "LM Studio".to_owned()
}

fn default_workspace_root() -> String {
    String::new()
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
    pub status: TaskStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    InProgress,
    Completed,
}

impl TaskStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::InProgress => "in_progress",
            Self::Completed => "completed",
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskDraft {
    pub title: String,
    pub status: TaskStatus,
}

#[derive(Debug, Clone)]
pub struct WorkspaceEntry {
    pub path: String,
    pub name: String,
    pub depth: usize,
    pub is_directory: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentActivity {
    pub message_id: i64,
    /// Byte length of the reply text when the step started, so steps render in order.
    #[serde(default)]
    pub offset: usize,
    pub call_id: String,
    pub title: String,
    pub detail: String,
    pub running: bool,
}

/// One conversation with its own memory and tasks; the folder is optional.
#[derive(Debug, Clone)]
pub struct Workspace {
    pub id: i64,
    pub name: String,
    pub folder: String,
}

#[derive(Debug, Clone)]
pub struct ConversationState {
    pub current: i64,
    /// The current workspace's folder, or empty when none is attached.
    pub folder: String,
    pub workspaces: Vec<Workspace>,
    pub messages: Vec<ChatMessage>,
    pub summary: String,
    pub compacted_through: i64,
    pub memory: String,
    pub activity: Vec<AgentActivity>,
}

#[derive(Debug, Clone)]
pub enum AppCommand {
    Load,
    NewWorkspace,
    SelectWorkspace(i64),
    DeleteWorkspace(i64),
    SaveMemory(String),
    SaveSettings(Settings),
    RefreshModels {
        request_id: u64,
        base_url: String,
        cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
    },
    ResetWorkspace {
        path: String,
    },
    /// Attach a folder to the current workspace; create makes a new folder first. Empty path detaches.
    AttachFolder {
        path: String,
        create: bool,
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
        /// Read before each approval so toggling applies to the running agent.
        auto_approve: std::sync::Arc<std::sync::atomic::AtomicBool>,
        cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
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
    ModelsLoaded {
        request_id: u64,
        result: Result<Vec<String>, String>,
    },
    WorkspaceOpened(Result<(String, Vec<WorkspaceEntry>), String>),
    ProviderUnreachable,
    ConversationLoaded(ConversationState),
    ContextCompacted {
        summary: String,
        through: i64,
    },
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
    ToolActivity(AgentActivity),
    ApprovalRequested {
        request_id: String,
        summary: String,
        preview: String,
    },
    TasksUpdated(Vec<Task>),
    TaskAdded(Task),
    TaskChanged {
        id: i64,
        complete: bool,
    },
    Notice(String),
    Error(String),
    RunFailed(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn existing_settings_load_without_new_fields() {
        let settings: Settings = serde_json::from_str(r#"{"lm_studio_url":"http://localhost:1234","selected_model":null,"workspace_root":"/tmp/project"}"#).unwrap();
        assert_eq!(settings.provider_name, "LM Studio");
        assert_eq!(settings.workspace_root, "/tmp/project");
    }
}

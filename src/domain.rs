use serde::{Deserialize, Serialize};

pub const APPLICATION_NAME: &str = "Moxi Box";
pub const DEFAULT_LM_STUDIO_URL: &str = "http://127.0.0.1:1234";
pub const DEFAULT_WINDOW_WIDTH: f32 = 1_280.0;
pub const DEFAULT_WINDOW_HEIGHT: f32 = 800.0;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default = "default_provider_name")]
    pub provider_name: String,
    #[serde(default)]
    pub workspaces: Vec<String>,
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
            workspaces: Vec::new(),
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentActivity {
    pub message_id: i64,
    pub call_id: String,
    pub title: String,
    pub detail: String,
    pub running: bool,
}

#[derive(Debug, Clone)]
pub struct Conversation {
    pub id: i64,
    pub title: String,
}

#[derive(Debug, Clone)]
pub struct ConversationState {
    pub current: i64,
    pub conversations: Vec<Conversation>,
    pub messages: Vec<ChatMessage>,
    pub summary: String,
    pub compacted_through: i64,
    pub memory: String,
    pub activity: Vec<AgentActivity>,
}

#[derive(Debug, Clone)]
pub enum AppCommand {
    Load,
    NewConversation {
        workspace: String,
    },
    SelectConversation {
        workspace: String,
        id: i64,
    },
    SaveMemory {
        workspace: String,
        content: String,
    },
    SaveSettings(Settings),
    RefreshModels {
        request_id: u64,
        base_url: String,
        cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
    },
    ResetWorkspace {
        path: String,
    },
    OpenWorkspace {
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
    ConnectionChanged(bool),
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn existing_settings_load_without_new_fields() {
        let settings: Settings = serde_json::from_str(r#"{"lm_studio_url":"http://localhost:1234","selected_model":null,"workspace_root":"/tmp/project"}"#).unwrap();
        assert_eq!(settings.provider_name, "LM Studio");
        assert!(settings.workspaces.is_empty());
        assert_eq!(settings.workspace_root, "/tmp/project");
    }
}

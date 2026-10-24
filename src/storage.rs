mod conversations;
mod tasks;

use std::{
    cell::{Cell, RefCell},
    fs,
    path::PathBuf,
};

use rusqlite::{params, Connection};

use crate::domain::{
    AgentActivity, ChatMessage, Conversation, ConversationState, MessageRole, Settings, Task,
};

const DATABASE_DIRECTORY: &str = "moxi-box";
const DATABASE_FILENAME: &str = "moxi-box.sqlite";
const SCHEMA_VERSION: i64 = 3;

pub struct Store {
    connection: Connection,
    current: Cell<i64>,
    workspace: RefCell<String>,
}

impl Store {
    pub fn open() -> Result<Self, String> {
        let directory = data_directory()?;
        fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
        Self::at_path(&directory.join(DATABASE_FILENAME))
    }

    fn at_path(path: &std::path::Path) -> Result<Self, String> {
        let connection = Connection::open(path).map_err(|error| error.to_string())?;
        connection
            .busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|e| e.to_string())?;
        connection
            .execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;")
            .map_err(|e| e.to_string())?;
        let store = Self {
            connection,
            current: Cell::new(1),
            workspace: RefCell::new(String::new()),
        };
        store.migrate()?;
        let settings = store.settings()?;
        store.select_workspace(&settings.workspace_root)?;
        Ok(store)
    }

    #[cfg(test)]
    pub(crate) fn in_memory() -> Self {
        Self::at_path(std::path::Path::new(":memory:")).unwrap()
    }

    fn migrate(&self) -> Result<(), String> {
        self.connection
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS schema_version (version INTEGER NOT NULL);
                 CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
                 CREATE TABLE IF NOT EXISTS tasks (id INTEGER PRIMARY KEY, title TEXT NOT NULL, complete INTEGER NOT NULL DEFAULT 0);
                 CREATE TABLE IF NOT EXISTS messages (id INTEGER PRIMARY KEY, role TEXT NOT NULL, content TEXT NOT NULL);",
            )
            .map_err(|error| error.to_string())?;
        let count: i64 = self
            .connection
            .query_row("SELECT COUNT(*) FROM schema_version", [], |row| row.get(0))
            .map_err(|error| error.to_string())?;
        if count == 0 {
            self.connection
                .execute(
                    "INSERT INTO schema_version (version) VALUES (?1)",
                    [SCHEMA_VERSION],
                )
                .map_err(|error| error.to_string())?;
        }
        self.connection
            .execute_batch("BEGIN IMMEDIATE;")
            .map_err(|e| e.to_string())?;
        let migration = (|| -> Result<(), String> {
            self.connection.execute_batch("CREATE TABLE IF NOT EXISTS conversations (id INTEGER PRIMARY KEY, workspace TEXT NOT NULL, title TEXT NOT NULL, summary TEXT NOT NULL DEFAULT '', compacted_through INTEGER NOT NULL DEFAULT 0);
                CREATE TABLE IF NOT EXISTS workspace_state (workspace TEXT PRIMARY KEY, conversation_id INTEGER NOT NULL);
                CREATE TABLE IF NOT EXISTS workspace_memory (workspace TEXT PRIMARY KEY, content TEXT NOT NULL);
                CREATE TABLE IF NOT EXISTS tool_events (id INTEGER PRIMARY KEY, conversation_id INTEGER NOT NULL, detail TEXT NOT NULL);").map_err(|e| e.to_string())?;
            let has_conversation = self
                .connection
                .prepare("PRAGMA table_info(messages)")
                .map_err(|e| e.to_string())?
                .query_map([], |row| row.get::<_, String>(1))
                .map_err(|e| e.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?
                .iter()
                .any(|column| column == "conversation_id");
            if !has_conversation {
                let workspace = self.settings()?.workspace_root;
                self.connection.execute("INSERT OR IGNORE INTO conversations (id, workspace, title) VALUES (1, ?1, 'Previous conversation')", [&workspace]).map_err(|e| e.to_string())?;
                self.connection.execute_batch("ALTER TABLE messages ADD COLUMN conversation_id INTEGER NOT NULL DEFAULT 1;").map_err(|e| e.to_string())?;
            }
            self.migrate_tasks()?;
            self.connection.execute_batch("CREATE INDEX IF NOT EXISTS messages_conversation ON messages(conversation_id, id);").map_err(|e| e.to_string())?;
            self.connection
                .execute("UPDATE schema_version SET version=?1", [SCHEMA_VERSION])
                .map_err(|e| e.to_string())?;
            Ok(())
        })();
        match migration {
            Ok(()) => self
                .connection
                .execute_batch("COMMIT;")
                .map_err(|e| e.to_string()),
            Err(error) => {
                let _ = self.connection.execute_batch("ROLLBACK;");
                Err(error)
            }
        }
    }

    pub fn load(&self) -> Result<(Settings, Vec<Task>, Vec<ChatMessage>), String> {
        Ok((self.settings()?, self.tasks()?, self.messages()?))
    }

    pub fn save_settings(&self, settings: &Settings) -> Result<(), String> {
        let serialized = serde_json::to_string(settings).map_err(|error| error.to_string())?;
        self.connection
            .execute(
                "INSERT OR REPLACE INTO settings (key, value) VALUES ('application', ?1)",
                [serialized],
            )
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    pub fn add_message(&self, role: MessageRole, content: &str) -> Result<i64, String> {
        self.connection
            .execute(
                "INSERT INTO messages (role, content, conversation_id) VALUES (?1, ?2, ?3)",
                params![role_name(role), content, self.current.get()],
            )
            .map_err(|error| error.to_string())?;
        let id = self.connection.last_insert_rowid();
        if role == MessageRole::User {
            let count: i64 = self
                .connection
                .query_row(
                    "SELECT COUNT(*) FROM messages WHERE conversation_id=?1 AND role='user'",
                    [self.current.get()],
                    |row| row.get(0),
                )
                .map_err(|e| e.to_string())?;
            if count == 1 {
                let title = content
                    .lines()
                    .next()
                    .unwrap_or(content)
                    .chars()
                    .take(48)
                    .collect::<String>();
                self.connection
                    .execute(
                        "UPDATE conversations SET title=?1 WHERE id=?2",
                        params![title, self.current.get()],
                    )
                    .map_err(|e| e.to_string())?;
            }
        }
        Ok(id)
    }

    pub fn replace_message_content(&self, id: i64, content: &str) -> Result<(), String> {
        self.connection
            .execute(
                "UPDATE messages SET content = ?1 WHERE id = ?2 AND conversation_id=?3",
                params![content, id, self.current.get()],
            )
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    fn settings(&self) -> Result<Settings, String> {
        let value = self.connection.query_row(
            "SELECT value FROM settings WHERE key = 'application'",
            [],
            |row| row.get::<_, String>(0),
        );
        match value {
            Ok(value) => serde_json::from_str(&value).map_err(|error| error.to_string()),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(Settings::default()),
            Err(error) => Err(error.to_string()),
        }
    }

    fn messages(&self) -> Result<Vec<ChatMessage>, String> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT id, role, content FROM messages WHERE conversation_id=?1 ORDER BY id ASC",
            )
            .map_err(|error| error.to_string())?;
        let messages = statement
            .query_map([self.current.get()], |row| {
                Ok(ChatMessage {
                    id: row.get(0)?,
                    role: parse_role(&row.get::<_, String>(1)?),
                    content: row.get(2)?,
                })
            })
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        Ok(messages)
    }
}

fn data_directory() -> Result<PathBuf, String> {
    if let Ok(directory) = std::env::var("XDG_DATA_HOME") {
        return Ok(PathBuf::from(directory).join(DATABASE_DIRECTORY));
    }
    let home = std::env::var("HOME")
        .map_err(|_| "HOME is not set; cannot locate Moxi Box data directory".to_owned())?;
    Ok(PathBuf::from(home)
        .join(".local/share")
        .join(DATABASE_DIRECTORY))
}

fn role_name(role: MessageRole) -> &'static str {
    match role {
        MessageRole::User => "user",
        MessageRole::Assistant => "assistant",
        MessageRole::System => "system",
    }
}
fn parse_role(role: &str) -> MessageRole {
    match role {
        "user" => MessageRole::User,
        "assistant" => MessageRole::Assistant,
        _ => MessageRole::System,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_history_migrates_and_conversations_summaries_memory_survive_restart() {
        let path = std::env::temp_dir().join(format!("moxi-storage-{}.sqlite", std::process::id()));
        let _ = fs::remove_file(&path);
        {
            let connection = Connection::open(&path).unwrap();
            connection.execute_batch("CREATE TABLE messages(id INTEGER PRIMARY KEY,role TEXT NOT NULL,content TEXT NOT NULL); INSERT INTO messages(role,content) VALUES('user','legacy message'); CREATE TABLE settings(key TEXT PRIMARY KEY,value TEXT NOT NULL);").unwrap();
            let settings = Settings {
                workspace_root: "/tmp/project-one".into(),
                ..Settings::default()
            };
            connection
                .execute(
                    "INSERT INTO settings VALUES('application',?1)",
                    [serde_json::to_string(&settings).unwrap()],
                )
                .unwrap();
        }
        let first_id;
        {
            let store = Store::at_path(&path).unwrap();
            assert_eq!(
                store.conversation_state().unwrap().messages[0].content,
                "legacy message"
            );
            first_id = store.current.get();
            for _ in 0..510 {
                store
                    .add_message(
                        MessageRole::Assistant,
                        "persisted beyond old retention limit",
                    )
                    .unwrap();
            }
            let last = store
                .conversation_state()
                .unwrap()
                .messages
                .last()
                .unwrap()
                .id;
            store.save_summary("durable summary", last).unwrap();
            store
                .save_memory("/tmp/project-one", "Use cargo test")
                .unwrap();
            store.new_conversation("/tmp/project-one").unwrap();
            assert!(store.conversation_state().unwrap().messages.is_empty());
            assert_eq!(store.memory().unwrap(), "Use cargo test");
            store.select_workspace("/tmp/project-two").unwrap();
            assert!(store.memory().unwrap().is_empty());
            assert!(store
                .select_conversation("/tmp/project-two", first_id)
                .is_err());
            store
                .select_conversation("/tmp/project-one", first_id)
                .unwrap();
            store
                .record_tool(&AgentActivity {
                    message_id: last,
                    call_id: "call1".into(),
                    title: "Read file".into(),
                    detail: "content".into(),
                    running: false,
                })
                .unwrap();
        }
        {
            let store = Store::at_path(&path).unwrap();
            let state = store.conversation_state().unwrap();
            assert_eq!(state.current, first_id);
            assert_eq!(state.messages.len(), 511);
            assert_eq!(state.summary, "durable summary");
            assert_eq!(state.memory, "Use cargo test");
            assert_eq!(state.activity[0].title, "Read file");
            assert_eq!(state.conversations.len(), 2);
        }
        let _ = fs::remove_file(path);
    }
}

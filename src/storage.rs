mod conversations;
mod tasks;

use std::{cell::Cell, fs, path::PathBuf};

use rusqlite::{params, Connection};

use crate::domain::{
    AgentActivity, ChatMessage, ConversationState, MessageRole, Settings, Task, Workspace,
};

const DATABASE_DIRECTORY: &str = "moxi-box";
const DATABASE_FILENAME: &str = "moxi-box.sqlite";
const SCHEMA_VERSION: i64 = 4;

/// A workspace is one conversation with its own memory and tasks, and an optional folder.
pub struct Store {
    connection: Connection,
    current: Cell<i64>,
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
            current: Cell::new(0),
        };
        store.migrate()?;
        store.select_initial()?;
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
                 CREATE TABLE IF NOT EXISTS messages (id INTEGER PRIMARY KEY, role TEXT NOT NULL, content TEXT NOT NULL, conversation_id INTEGER NOT NULL DEFAULT 1);",
            )
            .map_err(|error| error.to_string())?;
        let count: i64 = self
            .connection
            .query_row("SELECT COUNT(*) FROM schema_version", [], |row| row.get(0))
            .map_err(|error| error.to_string())?;
        if count == 0 {
            // Unversioned databases run every migration; each is a no-op on a fresh database.
            self.connection
                .execute("INSERT INTO schema_version (version) VALUES (0)", [])
                .map_err(|error| error.to_string())?;
        }
        let version: i64 = self
            .connection
            .query_row("SELECT MAX(version) FROM schema_version", [], |row| {
                row.get(0)
            })
            .map_err(|error| error.to_string())?;
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
            if version < 4 {
                self.migrate_to_workspaces()?;
            }
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
                        "UPDATE conversations SET title=?1 WHERE id=?2 AND title IN (?3, 'New conversation')",
                        params![title, self.current.get(), conversations::DEFAULT_NAME],
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

    /// Remember the reply being generated so a crash or kill can be detected on restart.
    pub fn set_active_run(&self, reply_id: Option<i64>) -> Result<(), String> {
        match reply_id {
            Some(id) => self.connection.execute(
                "INSERT OR REPLACE INTO settings (key, value) VALUES ('active_run', ?1)",
                [id.to_string()],
            ),
            None => self
                .connection
                .execute("DELETE FROM settings WHERE key='active_run'", []),
        }
        .map_err(|error| error.to_string())?;
        Ok(())
    }

    /// Mark a run left unfinished by a crash; returns true when one was found.
    pub fn recover_interrupted_run(&self) -> Result<bool, String> {
        let id = self.connection.query_row(
            "SELECT value FROM settings WHERE key='active_run'",
            [],
            |row| row.get::<_, String>(0),
        );
        let id = match id {
            Ok(id) => id.parse::<i64>().unwrap_or_default(),
            Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(false),
            Err(error) => return Err(error.to_string()),
        };
        self.connection
            .execute(
                "UPDATE messages SET content = content || ?1 WHERE id = ?2",
                params![
                    "\n\n[Run interrupted: Moxi Box closed before the run finished.]",
                    id
                ],
            )
            .map_err(|error| error.to_string())?;
        self.set_active_run(None)?;
        Ok(true)
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

/// Where conversations and crash.log are stored, for display.
pub fn data_location() -> String {
    data_directory().map_or_else(|error| error, |path| path.display().to_string())
}

/// Append a panic report to `crash.log` beside the database; best effort.
pub fn log_crash(report: &str) {
    use std::io::Write;
    let Ok(directory) = data_directory() else {
        return;
    };
    let _ = fs::create_dir_all(&directory);
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |time| time.as_secs());
    if let Ok(mut file) = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(directory.join("crash.log"))
    {
        let _ = writeln!(file, "[{seconds}] {report}");
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
    fn legacy_folders_become_workspaces_and_state_survives_restart() {
        let path = std::env::temp_dir().join(format!("moxi-storage-{}.sqlite", std::process::id()));
        let _ = fs::remove_file(&path);
        {
            // Schema 3: two conversations in one folder sharing folder-keyed memory.
            let connection = Connection::open(&path).unwrap();
            connection.execute_batch("
                CREATE TABLE settings(key TEXT PRIMARY KEY, value TEXT NOT NULL);
                CREATE TABLE schema_version(version INTEGER NOT NULL); INSERT INTO schema_version VALUES(3);
                CREATE TABLE messages(id INTEGER PRIMARY KEY, role TEXT NOT NULL, content TEXT NOT NULL, conversation_id INTEGER NOT NULL DEFAULT 1);
                CREATE TABLE conversations(id INTEGER PRIMARY KEY, workspace TEXT NOT NULL, title TEXT NOT NULL, summary TEXT NOT NULL DEFAULT '', compacted_through INTEGER NOT NULL DEFAULT 0);
                CREATE TABLE workspace_state(workspace TEXT PRIMARY KEY, conversation_id INTEGER NOT NULL);
                CREATE TABLE workspace_memory(workspace TEXT PRIMARY KEY, content TEXT NOT NULL);
                INSERT INTO conversations(id, workspace, title) VALUES(1, '/tmp/project-one', 'Previous conversation'), (2, '/tmp/project-one', 'Fix the parser');
                INSERT INTO messages(role, content, conversation_id) VALUES('user', 'legacy message', 1), ('user', 'second chat', 2);
                INSERT INTO workspace_state VALUES('/tmp/project-one', 1);
                INSERT INTO workspace_memory VALUES('/tmp/project-one', 'Use cargo test');
            ").unwrap();
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
        let second_id;
        {
            let store = Store::at_path(&path).unwrap();
            let state = store.conversation_state().unwrap();
            assert_eq!(state.current, 1);
            assert_eq!(state.folder, "/tmp/project-one");
            assert_eq!(state.messages[0].content, "legacy message");
            assert_eq!(state.memory, "Use cargo test");
            let names = state
                .workspaces
                .iter()
                .map(|w| w.name.as_str())
                .collect::<Vec<_>>();
            assert_eq!(names, ["Fix the parser", "project-one"]);
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
                .record_tool(&AgentActivity {
                    message_id: last,
                    offset: 0,
                    call_id: "call1".into(),
                    title: "Read file".into(),
                    detail: "content".into(),
                    running: false,
                })
                .unwrap();
            // Memory belongs to each workspace separately after migration.
            store.select(2).unwrap();
            assert_eq!(store.memory().unwrap(), "Use cargo test");
            store.save_memory("Only for the parser").unwrap();
            // A new workspace starts empty, without a folder, and is auto-named.
            store.create_workspace().unwrap();
            second_id = store.current.get();
            let state = store.conversation_state().unwrap();
            assert!(state.messages.is_empty() && state.memory.is_empty());
            assert!(state.folder.is_empty());
            store
                .add_message(MessageRole::User, "Plan a trip\nwith details")
                .unwrap();
            store.set_folder("/tmp/trip").unwrap();
            let state = store.conversation_state().unwrap();
            assert_eq!(state.workspaces[0].name, "Plan a trip");
            assert_eq!(state.folder, "/tmp/trip");
            store.select(first_id).unwrap();
        }
        {
            let store = Store::at_path(&path).unwrap();
            let state = store.conversation_state().unwrap();
            assert_eq!(state.current, first_id);
            assert_eq!(state.messages.len(), 511);
            assert_eq!(state.summary, "durable summary");
            assert_eq!(state.memory, "Use cargo test");
            assert_eq!(state.activity[0].title, "Read file");
            assert_eq!(state.workspaces.len(), 3);
            store.select(2).unwrap();
            assert_eq!(store.memory().unwrap(), "Only for the parser");
            store.select(second_id).unwrap();
            store.set_active_run(Some(1)).unwrap();
        }
        {
            // A run that never finished is marked once after an unexpected shutdown.
            let store = Store::at_path(&path).unwrap();
            assert!(store.recover_interrupted_run().unwrap());
            assert!(!store.recover_interrupted_run().unwrap());
            assert_eq!(store.current.get(), second_id);
            store.select(1).unwrap();
            assert!(store.conversation_state().unwrap().messages[0]
                .content
                .ends_with("[Run interrupted: Moxi Box closed before the run finished.]"));
        }
        let _ = fs::remove_file(path);
    }

    #[test]
    fn workspaces_can_be_named_and_renamed() {
        let store = Store::in_memory();
        store.create_named_workspace("  Trip planning ").unwrap();
        let id = store.current.get();
        // A chosen name is kept when the first message arrives.
        store
            .add_message(MessageRole::User, "Find flights")
            .unwrap();
        assert_eq!(
            store.conversation_state().unwrap().workspaces[0].name,
            "Trip planning"
        );
        store.rename_workspace(id, "Holiday").unwrap();
        assert_eq!(
            store.conversation_state().unwrap().workspaces[0].name,
            "Holiday"
        );
        assert!(store.rename_workspace(id, "  ").is_err());
        assert!(store.rename_workspace(id, "two\nlines").is_err());
        assert!(store.rename_workspace(9999, "Gone").is_err());
        assert!(store.create_named_workspace(&"x".repeat(81)).is_err());
    }

    #[test]
    fn a_fresh_database_starts_with_one_folderless_workspace() {
        let store = Store::in_memory();
        let state = store.conversation_state().unwrap();
        assert_eq!(state.workspaces.len(), 1);
        assert_eq!(state.workspaces[0].name, "New workspace");
        assert!(state.folder.is_empty());
        store.delete_workspace(state.current).unwrap();
        // Deleting the last workspace leaves a fresh one rather than none.
        assert_eq!(store.conversation_state().unwrap().workspaces.len(), 1);
    }
}

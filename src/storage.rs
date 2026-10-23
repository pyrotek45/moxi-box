use std::{fs, path::PathBuf};

use rusqlite::{params, Connection};

use crate::domain::{ChatMessage, MessageRole, Settings, Task, MAX_STORED_MESSAGES};

const DATABASE_DIRECTORY: &str = "moxi-box";
const DATABASE_FILENAME: &str = "moxi-box.sqlite";
const SCHEMA_VERSION: i64 = 1;

pub struct Store {
    connection: Connection,
}

impl Store {
    pub fn open() -> Result<Self, String> {
        let directory = data_directory()?;
        fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
        let connection = Connection::open(directory.join(DATABASE_FILENAME))
            .map_err(|error| error.to_string())?;
        let store = Self { connection };
        store.migrate()?;
        Ok(store)
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
        Ok(())
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

    pub fn add_task(&self, title: &str) -> Result<Task, String> {
        self.connection
            .execute("INSERT INTO tasks (title) VALUES (?1)", [title])
            .map_err(|error| error.to_string())?;
        Ok(Task {
            id: self.connection.last_insert_rowid(),
            title: title.to_owned(),
            complete: false,
        })
    }

    pub fn set_task_complete(&self, id: i64, complete: bool) -> Result<(), String> {
        self.connection
            .execute(
                "UPDATE tasks SET complete = ?1 WHERE id = ?2",
                params![i64::from(complete), id],
            )
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    pub fn add_message(&self, role: MessageRole, content: &str) -> Result<i64, String> {
        self.connection
            .execute(
                "INSERT INTO messages (role, content) VALUES (?1, ?2)",
                params![role_name(role), content],
            )
            .map_err(|error| error.to_string())?;
        self.connection.execute("DELETE FROM messages WHERE id NOT IN (SELECT id FROM messages ORDER BY id DESC LIMIT ?1)", [MAX_STORED_MESSAGES as i64]).map_err(|error| error.to_string())?;
        Ok(self.connection.last_insert_rowid())
    }

    pub fn replace_message_content(&self, id: i64, content: &str) -> Result<(), String> {
        self.connection
            .execute(
                "UPDATE messages SET content = ?1 WHERE id = ?2",
                params![content, id],
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

    fn tasks(&self) -> Result<Vec<Task>, String> {
        let mut statement = self
            .connection
            .prepare("SELECT id, title, complete FROM tasks ORDER BY complete, id DESC")
            .map_err(|error| error.to_string())?;
        let tasks = statement
            .query_map([], |row| {
                Ok(Task {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    complete: row.get::<_, i64>(2)? != 0,
                })
            })
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        Ok(tasks)
    }

    fn messages(&self) -> Result<Vec<ChatMessage>, String> {
        let mut statement = self
            .connection
            .prepare("SELECT id, role, content FROM messages ORDER BY id ASC")
            .map_err(|error| error.to_string())?;
        let messages = statement
            .query_map([], |row| {
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

use super::*;

pub(super) const DEFAULT_NAME: &str = "New workspace";

fn valid_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 80 || name.chars().any(char::is_control) {
        return Err("Use a workspace name of 1–80 characters on one line.".to_owned());
    }
    Ok(name.to_owned())
}

fn folder_name(folder: &str) -> String {
    std::path::Path::new(folder)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(folder)
        .to_owned()
}

impl Store {
    /// Key for this workspace's tasks and memory.
    pub(super) fn scope(&self) -> String {
        format!("#{}", self.current.get())
    }

    /// The attached folder, or an empty string when the workspace has none.
    pub fn folder(&self) -> Result<String, String> {
        self.connection
            .query_row(
                "SELECT workspace FROM conversations WHERE id=?1",
                [self.current.get()],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())
    }

    /// Reopen the last workspace, else the newest, else create one.
    pub(super) fn select_initial(&self) -> Result<(), String> {
        let saved = self
            .connection
            .query_row(
                "SELECT c.id FROM settings s JOIN conversations c ON c.id = CAST(s.value AS INTEGER) WHERE s.key='current_workspace'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .or_else(|_| {
                self.connection.query_row(
                    "SELECT id FROM conversations ORDER BY id DESC LIMIT 1",
                    [],
                    |row| row.get::<_, i64>(0),
                )
            });
        match saved {
            Ok(id) => self.select(id),
            Err(rusqlite::Error::QueryReturnedNoRows) => self.create_workspace(),
            Err(error) => Err(error.to_string()),
        }
    }

    pub fn create_workspace(&self) -> Result<(), String> {
        self.create_named_workspace("")
    }

    /// A blank name uses the default, which is replaced by the first message.
    pub fn create_named_workspace(&self, name: &str) -> Result<(), String> {
        let name = if name.trim().is_empty() {
            DEFAULT_NAME.to_owned()
        } else {
            valid_name(name)?
        };
        self.connection
            .execute(
                "INSERT INTO conversations(workspace, title) VALUES('', ?1)",
                [name],
            )
            .map_err(|e| e.to_string())?;
        self.select(self.connection.last_insert_rowid())
    }

    pub fn rename_workspace(&self, id: i64, name: &str) -> Result<(), String> {
        let changed = self
            .connection
            .execute(
                "UPDATE conversations SET title=?1 WHERE id=?2",
                params![valid_name(name)?, id],
            )
            .map_err(|e| e.to_string())?;
        if changed == 0 {
            return Err("That workspace no longer exists.".to_owned());
        }
        Ok(())
    }

    pub fn select(&self, id: i64) -> Result<(), String> {
        let exists: bool = self
            .connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM conversations WHERE id=?1)",
                [id],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())?;
        if !exists {
            return Err("That workspace no longer exists.".to_owned());
        }
        self.connection
            .execute(
                "INSERT OR REPLACE INTO settings(key, value) VALUES('current_workspace', ?1)",
                [id.to_string()],
            )
            .map_err(|e| e.to_string())?;
        self.current.set(id);
        Ok(())
    }

    /// Remove a workspace's conversation, memory, and tasks. Files are never touched.
    pub fn delete_workspace(&self, id: i64) -> Result<(), String> {
        let scope = format!("#{id}");
        let transaction = self
            .connection
            .unchecked_transaction()
            .map_err(|e| e.to_string())?;
        for (sql, key) in [
            (
                "DELETE FROM messages WHERE conversation_id=?1",
                id.to_string(),
            ),
            (
                "DELETE FROM tool_events WHERE conversation_id=?1",
                id.to_string(),
            ),
            ("DELETE FROM conversations WHERE id=?1", id.to_string()),
            ("DELETE FROM tasks WHERE workspace=?1", scope.clone()),
            (
                "DELETE FROM workspace_memory WHERE workspace=?1",
                scope.clone(),
            ),
        ] {
            transaction.execute(sql, [key]).map_err(|e| e.to_string())?;
        }
        transaction.commit().map_err(|e| e.to_string())?;
        if id == self.current.get() {
            self.current.set(0);
            self.select_initial()?;
        }
        Ok(())
    }

    /// Attach (or with an empty path, detach) the folder the agent works in.
    pub fn set_folder(&self, folder: &str) -> Result<(), String> {
        self.connection
            .execute(
                "UPDATE conversations SET workspace=?1 WHERE id=?2",
                params![folder, self.current.get()],
            )
            .map_err(|e| e.to_string())?;
        if !folder.is_empty() {
            self.connection
                .execute(
                    "UPDATE conversations SET title=?1 WHERE id=?2 AND title=?3",
                    params![folder_name(folder), self.current.get(), DEFAULT_NAME],
                )
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    /// Schema 4: folders become optional attachments; tasks and memory move to workspace ids.
    pub(super) fn migrate_to_workspaces(&self) -> Result<(), String> {
        let strings = |sql: &str| -> Result<Vec<String>, String> {
            self.connection
                .prepare(sql)
                .map_err(|e| e.to_string())?
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(|e| e.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())
        };
        // Every folder that owns tasks or memory keeps at least one workspace.
        for folder in strings(
            "SELECT workspace FROM tasks WHERE workspace NOT LIKE '#%' AND workspace != ''
             UNION SELECT workspace FROM workspace_memory WHERE workspace NOT LIKE '#%' AND workspace != ''",
        )? {
            self.connection
                .execute(
                    "INSERT INTO conversations(workspace, title) SELECT ?1, ?2
                     WHERE NOT EXISTS(SELECT 1 FROM conversations WHERE workspace=?1)",
                    params![folder, folder_name(&folder)],
                )
                .map_err(|e| e.to_string())?;
        }
        self.connection
            .execute_batch(
                "INSERT OR IGNORE INTO workspace_memory(workspace, content)
                   SELECT '#' || c.id, m.content FROM workspace_memory m
                   JOIN conversations c ON c.workspace = m.workspace WHERE m.workspace NOT LIKE '#%';
                 UPDATE tasks SET workspace = '#' || (SELECT MAX(c.id) FROM conversations c WHERE c.workspace = tasks.workspace)
                   WHERE workspace NOT LIKE '#%' AND EXISTS(SELECT 1 FROM conversations c WHERE c.workspace = tasks.workspace);",
            )
            .map_err(|e| e.to_string())?;
        for folder in strings(
            "SELECT DISTINCT workspace FROM conversations
             WHERE workspace != '' AND title IN ('Previous conversation', 'New conversation')",
        )? {
            self.connection
                .execute(
                    "UPDATE conversations SET title=?1 WHERE workspace=?2 AND title IN ('Previous conversation', 'New conversation')",
                    params![folder_name(&folder), folder],
                )
                .map_err(|e| e.to_string())?;
        }
        // Reopen the conversation that was selected for the last open folder.
        let root = self.settings()?.workspace_root;
        self.connection
            .execute(
                "INSERT OR REPLACE INTO settings(key, value)
                 SELECT 'current_workspace', CAST(conversation_id AS TEXT) FROM workspace_state WHERE workspace=?1",
                [root],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn conversation_state(&self) -> Result<ConversationState, String> {
        let mut query = self
            .connection
            .prepare("SELECT id, title, workspace FROM conversations ORDER BY id DESC")
            .map_err(|e| e.to_string())?;
        let workspaces = query
            .query_map([], |row| {
                Ok(Workspace {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    folder: row.get(2)?,
                })
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        let (summary, compacted_through) = self.summary()?;
        let mut query = self.connection.prepare("SELECT detail FROM tool_events WHERE conversation_id=?1 ORDER BY id DESC LIMIT 400").map_err(|e| e.to_string())?;
        let mut activity = query
            .query_map([self.current.get()], |row| row.get::<_, String>(0))
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        activity.reverse();
        let mut steps: Vec<AgentActivity> = Vec::new();
        for detail in activity {
            if let Ok(mut step) = serde_json::from_str::<AgentActivity>(&detail) {
                if step.running {
                    step.running = false;
                    step.title = format!("Interrupted · {}", step.title);
                }
                if let Some(existing) = steps
                    .iter_mut()
                    .find(|old| old.message_id == step.message_id && old.call_id == step.call_id)
                {
                    *existing = step;
                } else {
                    steps.push(step);
                }
            }
        }
        Ok(ConversationState {
            current: self.current.get(),
            folder: self.folder()?,
            workspaces,
            messages: self.messages()?,
            summary,
            compacted_through,
            memory: self.memory()?,
            activity: steps,
        })
    }
    pub fn summary(&self) -> Result<(String, i64), String> {
        self.connection
            .query_row(
                "SELECT summary,compacted_through FROM conversations WHERE id=?1",
                [self.current.get()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|e| e.to_string())
    }
    pub fn save_summary(&self, summary: &str, through: i64) -> Result<(), String> {
        self.connection
            .execute(
                "UPDATE conversations SET summary=?1,compacted_through=?2 WHERE id=?3",
                params![summary, through, self.current.get()],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    pub fn memory(&self) -> Result<String, String> {
        match self.connection.query_row(
            "SELECT content FROM workspace_memory WHERE workspace=?1",
            [self.scope()],
            |row| row.get(0),
        ) {
            Ok(memory) => Ok(memory),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(String::new()),
            Err(error) => Err(error.to_string()),
        }
    }
    pub fn save_memory(&self, content: &str) -> Result<(), String> {
        if content.len() > 6000 {
            return Err("Keep workspace memory under 6,000 bytes.".to_owned());
        }
        self.connection
            .execute(
                "INSERT OR REPLACE INTO workspace_memory(workspace,content) VALUES(?1,?2)",
                params![self.scope(), content],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    pub fn record_tool(&self, activity: &AgentActivity) -> Result<(), String> {
        self.connection
            .execute(
                "INSERT INTO tool_events(conversation_id,detail) VALUES(?1,?2)",
                params![
                    self.current.get(),
                    serde_json::to_string(activity).map_err(|e| e.to_string())?
                ],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}

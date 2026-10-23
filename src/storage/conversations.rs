use super::*;

impl Store {
    pub fn select_workspace(&self, workspace: &str) -> Result<(), String> {
        let id = self.connection.query_row("SELECT c.id FROM workspace_state s JOIN conversations c ON c.id=s.conversation_id WHERE s.workspace=?1 AND c.workspace=?1", [workspace], |row| row.get::<_, i64>(0));
        match id {
            Ok(id) => {
                self.current.set(id);
                *self.workspace.borrow_mut() = workspace.to_owned();
                Ok(())
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                let latest = self.connection.query_row(
                    "SELECT id FROM conversations WHERE workspace=?1 ORDER BY id DESC LIMIT 1",
                    [workspace],
                    |row| row.get::<_, i64>(0),
                );
                match latest {
                    Ok(id) => self.select_conversation(workspace, id),
                    Err(rusqlite::Error::QueryReturnedNoRows) => self.new_conversation(workspace),
                    Err(error) => Err(error.to_string()),
                }
            }
            Err(error) => Err(error.to_string()),
        }
    }
    pub fn new_conversation(&self, workspace: &str) -> Result<(), String> {
        self.connection
            .execute(
                "INSERT INTO conversations(workspace, title) VALUES(?1, 'New conversation')",
                [workspace],
            )
            .map_err(|e| e.to_string())?;
        self.select_conversation(workspace, self.connection.last_insert_rowid())
    }
    pub fn select_conversation(&self, workspace: &str, id: i64) -> Result<(), String> {
        let exists: bool = self
            .connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM conversations WHERE id=?1 AND workspace=?2)",
                params![id, workspace],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())?;
        if !exists {
            return Err("Conversation does not belong to this workspace.".to_owned());
        }
        self.connection
            .execute(
                "INSERT OR REPLACE INTO workspace_state(workspace,conversation_id) VALUES(?1,?2)",
                params![workspace, id],
            )
            .map_err(|e| e.to_string())?;
        self.current.set(id);
        *self.workspace.borrow_mut() = workspace.to_owned();
        Ok(())
    }
    pub fn conversation_state(&self) -> Result<ConversationState, String> {
        let mut query = self
            .connection
            .prepare("SELECT id,title FROM conversations WHERE workspace=?1 ORDER BY id DESC")
            .map_err(|e| e.to_string())?;
        let conversations = query
            .query_map([self.workspace.borrow().as_str()], |row| {
                Ok(Conversation {
                    id: row.get(0)?,
                    title: row.get(1)?,
                })
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        let (summary, compacted_through) = self.summary()?;
        let mut query = self.connection.prepare("SELECT detail FROM tool_events WHERE conversation_id=?1 ORDER BY id DESC LIMIT 100").map_err(|e| e.to_string())?;
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
        let activity = steps;
        Ok(ConversationState {
            current: self.current.get(),
            conversations,
            messages: self.messages()?,
            summary,
            compacted_through,
            memory: self.memory()?,
            activity,
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
            [self.workspace.borrow().as_str()],
            |row| row.get(0),
        ) {
            Ok(memory) => Ok(memory),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(String::new()),
            Err(error) => Err(error.to_string()),
        }
    }
    pub fn save_memory(&self, workspace: &str, content: &str) -> Result<(), String> {
        if content.len() > 6000 {
            return Err("Keep workspace memory under 6,000 bytes.".to_owned());
        }
        self.connection
            .execute(
                "INSERT OR REPLACE INTO workspace_memory(workspace,content) VALUES(?1,?2)",
                params![workspace, content],
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

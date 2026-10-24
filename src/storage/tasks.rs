use super::*;
use crate::domain::{TaskDraft, TaskStatus};

impl Store {
    pub(super) fn migrate_tasks(&self) -> Result<(), String> {
        let columns = self
            .connection
            .prepare("PRAGMA table_info(tasks)")
            .map_err(|e| e.to_string())?
            .query_map([], |row| row.get::<_, String>(1))
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        if !columns.iter().any(|c| c == "workspace") {
            self.connection.execute_batch("ALTER TABLE tasks ADD COLUMN workspace TEXT NOT NULL DEFAULT ''; ALTER TABLE tasks ADD COLUMN status TEXT NOT NULL DEFAULT 'pending'; UPDATE tasks SET status=CASE WHEN complete=1 THEN 'completed' ELSE 'pending' END;").map_err(|e| e.to_string())?;
            self.connection
                .execute(
                    "UPDATE tasks SET workspace=?1",
                    [self.settings()?.workspace_root],
                )
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    pub fn tasks(&self) -> Result<Vec<Task>, String> {
        self.connection
            .prepare("SELECT id, title, status FROM tasks WHERE workspace=?1 ORDER BY id")
            .map_err(|e| e.to_string())?
            .query_map([self.scope()], |row| {
                let status: String = row.get(2)?;
                Ok(Task {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    status: match status.as_str() {
                        "completed" => TaskStatus::Completed,
                        "in_progress" => TaskStatus::InProgress,
                        _ => TaskStatus::Pending,
                    },
                })
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
    }

    pub fn add_task(&self, title: &str) -> Result<Task, String> {
        validate_title(title)?;
        if self.tasks()?.len() >= 100 {
            return Err("Use at most 100 tasks.".into());
        }
        self.connection
            .execute(
                "INSERT INTO tasks(title, workspace, status) VALUES(?1, ?2, 'pending')",
                params![title.trim(), self.scope()],
            )
            .map_err(|e| e.to_string())?;
        Ok(Task {
            id: self.connection.last_insert_rowid(),
            title: title.trim().to_owned(),
            status: TaskStatus::Pending,
        })
    }

    pub fn set_task_complete(&self, id: i64, complete: bool) -> Result<(), String> {
        let status = if complete { "completed" } else { "pending" };
        let changed = self
            .connection
            .execute(
                "UPDATE tasks SET status=?1, complete=?2 WHERE id=?3 AND workspace=?4",
                params![status, complete, id, self.scope()],
            )
            .map_err(|e| e.to_string())?;
        if changed == 0 {
            return Err("Task does not belong to this workspace.".into());
        }
        Ok(())
    }

    pub fn replace_tasks(&self, arguments: &serde_json::Value) -> Result<Vec<Task>, String> {
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Request {
            tasks: Vec<TaskDraft>,
        }
        let request: Request =
            serde_json::from_value(normalize_task_arguments(arguments).map_err(task_error)?)
                .map_err(task_error)?;
        if request.tasks.len() > 100 {
            return Err("Use at most 100 tasks.".into());
        }
        for task in &request.tasks {
            validate_title(&task.title)?;
        }
        let transaction = self
            .connection
            .unchecked_transaction()
            .map_err(|e| e.to_string())?;
        let workspace = self.scope();
        transaction
            .execute("DELETE FROM tasks WHERE workspace=?1", [workspace.as_str()])
            .map_err(|e| e.to_string())?;
        for task in request.tasks {
            transaction
                .execute(
                    "INSERT INTO tasks(title, workspace, status, complete) VALUES(?1, ?2, ?3, ?4)",
                    params![
                        task.title.trim(),
                        workspace.as_str(),
                        task.status.as_str(),
                        task.status == TaskStatus::Completed
                    ],
                )
                .map_err(|e| e.to_string())?;
        }
        transaction.commit().map_err(|e| e.to_string())?;
        self.tasks()
    }
}

fn task_error(error: impl std::fmt::Display) -> String {
    format!("Task list was NOT changed: {error}. Call update_tasks with actual data, for example {{\"tasks\":[{{\"title\":\"Inspect the code\",\"status\":\"in_progress\"}}]}}. Include all tasks to retain. Do not send a schema or required_fields. Valid statuses: pending, in_progress, completed. Correct the call before claiming the plan was updated.")
}

fn normalize_task_arguments(value: &serde_json::Value) -> Result<serde_json::Value, String> {
    fn object(
        value: &serde_json::Value,
    ) -> Result<serde_json::Map<String, serde_json::Value>, String> {
        let fields = value.as_object().ok_or("Expected a JSON object")?;
        let mut normalized = serde_json::Map::new();
        for (key, value) in fields {
            // Some small models include literal quotation marks in property names.
            let key = key
                .strip_prefix('"')
                .and_then(|key| key.strip_suffix('"'))
                .unwrap_or(key);
            if normalized.insert(key.to_owned(), value.clone()).is_some() {
                return Err(format!("Conflicting duplicate field: {key}"));
            }
        }
        Ok(normalized)
    }
    let mut root = object(value)?;
    let tasks = root
        .get_mut("tasks")
        .and_then(serde_json::Value::as_array_mut)
        .ok_or("Missing tasks array; send task data, not a schema")?;
    for task in tasks {
        *task = serde_json::Value::Object(object(task)?);
    }
    // Harmless schema metadata may accompany a real list. Never derive tasks from it.
    root.remove("required_fields");
    Ok(serde_json::Value::Object(root))
}

fn validate_title(title: &str) -> Result<(), String> {
    if title.trim().is_empty() || title.chars().count() > 200 || title.chars().any(char::is_control)
    {
        return Err("Task titles must be 1–200 characters on a single line.".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn batches_complete_many_tasks_and_invalid_batches_preserve_plan() {
        let store = Store::in_memory();
        let project_a = store.current.get();
        store
            .replace_tasks(&json!({"tasks":[
                {"title":"Inspect code", "status":"completed"},
                {"title":"Implement change", "status":"in_progress"},
                {"title":"Verify result", "status":"pending"}
            ]}))
            .unwrap();
        let result = store
            .replace_tasks(&json!({"tasks":[
                {"title":"Inspect code", "status":"completed"},
                {"title":"Implement change", "status":"completed"},
                {"title":"Verify result", "status":"completed"}
            ]}))
            .unwrap();
        assert_eq!(result.len(), 3);
        assert!(result
            .iter()
            .all(|task| task.status == TaskStatus::Completed));
        for invalid in [
            json!({"tasks":[{"title":"valid", "status":"pending"},{"title":" ", "status":"completed"}]}),
            json!({"tasks":[{"title":"bad", "status":"done"}]}),
            json!({"tasks":vec![json!({"title":"too many", "status":"pending"});101]}),
        ] {
            assert!(store.replace_tasks(&invalid).is_err());
            assert_eq!(store.tasks().unwrap().len(), 3);
            assert!(store
                .tasks()
                .unwrap()
                .iter()
                .all(|task| task.status == TaskStatus::Completed));
        }
        store.create_workspace().unwrap();
        let project_b = store.current.get();
        assert!(store.tasks().unwrap().is_empty());
        assert!(store.set_task_complete(result[0].id, false).is_err());
        store.add_task("Other project").unwrap();
        store.select(project_a).unwrap();
        assert_eq!(store.tasks().unwrap().len(), 3);
        store.replace_tasks(&json!({"tasks":[]})).unwrap();
        assert!(store.tasks().unwrap().is_empty());
        store.select(project_b).unwrap();
        assert_eq!(store.tasks().unwrap()[0].title, "Other project");
        // Deleting a workspace removes its tasks with it.
        store.delete_workspace(project_b).unwrap();
        assert_eq!(store.current.get(), project_a);
        assert!(store.select(project_b).is_err());
    }

    #[test]
    fn legacy_tasks_migrate_to_saved_workspace_and_survive_restart() {
        let path = std::env::temp_dir().join(format!(
            "moxi-task-migration-{}-{}.sqlite",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        {
            let connection = Connection::open(&path).unwrap();
            connection.execute_batch("CREATE TABLE settings(key TEXT PRIMARY KEY, value TEXT NOT NULL); CREATE TABLE tasks(id INTEGER PRIMARY KEY, title TEXT NOT NULL, complete INTEGER NOT NULL DEFAULT 0); INSERT INTO tasks VALUES(1, 'Legacy open', 0), (2, 'Legacy done', 1);").unwrap();
            let settings = Settings {
                workspace_root: "/legacy".into(),
                ..Settings::default()
            };
            connection
                .execute(
                    "INSERT INTO settings VALUES('application', ?1)",
                    [serde_json::to_string(&settings).unwrap()],
                )
                .unwrap();
        }
        {
            let store = Store::at_path(&path).unwrap();
            // The folder that owned the tasks becomes a workspace with that folder attached.
            assert_eq!(store.folder().unwrap(), "/legacy");
            assert_eq!(
                store.conversation_state().unwrap().workspaces[0].name,
                "legacy"
            );
            let legacy = store.current.get();
            let tasks = store.tasks().unwrap();
            assert_eq!(tasks.len(), 2);
            store.create_workspace().unwrap();
            assert!(store.tasks().unwrap().is_empty());
            assert!(store.folder().unwrap().is_empty());
            store.select(legacy).unwrap();
            assert_eq!(tasks[1].status, TaskStatus::Completed);
            store
                .replace_tasks(&json!({"tasks":[{"title":"Resume me", "status":"in_progress"}]}))
                .unwrap();
        }
        {
            let store = Store::at_path(&path).unwrap();
            let tasks = store.tasks().unwrap();
            assert_eq!(tasks.len(), 1);
            assert_eq!(tasks[0].status, TaskStatus::InProgress);
        }
        fs::remove_file(path).unwrap();
    }
}

#[cfg(test)]
mod argument_recovery_tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn quoted_keys_and_metadata_recover_without_changing_task_text() {
        let store = Store::in_memory();
        let tasks = store.replace_tasks(&json!({"tasks":[{"\"title\"":"Keep \"quotes\" in text", "\"status\"":"pending"}],"required_fields":["tasks"]})).unwrap();
        assert_eq!(tasks[0].title, "Keep \"quotes\" in text");
    }
    #[test]
    fn schema_only_and_conflicting_keys_preserve_existing_plan() {
        let store = Store::in_memory();
        store.add_task("Keep me").unwrap();
        for arguments in [
            json!({"required_fields":["tasks"]}),
            json!({"tasks":[{"title":"one","\"title\"":"two","status":"pending"}]}),
            json!({"tasks":[{"title":"bad","status":"unknown"}]}),
        ] {
            let error = store.replace_tasks(&arguments).unwrap_err();
            assert!(error.contains("NOT changed"));
            assert!(error.contains("in_progress"));
            assert_eq!(store.tasks().unwrap()[0].title, "Keep me");
        }
    }
}

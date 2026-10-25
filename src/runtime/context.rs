use super::*;
use crate::domain::{ChatMessage, Task, TaskStatus};
use serde_json::{json, Value};

pub(super) fn prepare(
    store: &Store,
    runtime: &Runtime,
    settings: &Settings,
    events: &EventSender,
    cancelled: &AtomicBool,
    learned_limit: Option<usize>,
) -> Result<Vec<ChatMessage>, String> {
    let state = store.conversation_state()?;
    let tasks = store.tasks()?;
    let workspace = workspace_context(&settings.workspace_root);
    let fixed = state.memory.len()
        + workspace.len()
        + tasks.iter().map(|task| task.title.len() + 6).sum::<usize>();
    let (mut summary, through) = store.summary()?;
    let mut recent = state
        .messages
        .into_iter()
        .map(|mut message| {
            note_tools_used(&mut message, &state.activity);
            message
        })
        .filter(|message| {
            message.id > through
                && !message.content.trim().is_empty()
                && message.role != MessageRole::System
        })
        .collect::<Vec<_>>();
    while learned_limit.is_some_and(|limit| {
        recent.iter().map(|m| m.content.len() + 80).sum::<usize>() + summary.len() + fixed > limit
    }) {
        let limit = learned_limit.expect("checked above");
        if recent.len() <= 2 {
            return Err("The latest messages exceed the context size this provider accepted. Start a new conversation with a shorter prompt.".to_owned());
        }
        let room = limit.saturating_sub(summary.len() + fixed + 2000);
        let count = compaction_prefix(&recent, room)?;
        let transcript = recent[..count]
            .iter()
            .map(|m| json!({"role":format!("{:?}",m.role),"content":m.content}))
            .collect::<Vec<_>>();
        send(
            events,
            AppEvent::Notice("Summarizing earlier turns…".to_owned()),
        );
        let updated = runtime.block_on(lm_studio::summarize(
            settings,
            &summary,
            &transcript,
            cancelled,
        ))?;
        let compacted_through = recent[count - 1].id;
        store.save_summary(&updated, compacted_through)?;
        summary = updated;
        recent.drain(..count);
        send(
            events,
            AppEvent::ContextCompacted {
                summary: summary.clone(),
                through: compacted_through,
            },
        );
    }
    let mut history = context_notes(&state.memory, &tasks, &summary, &workspace);
    history.extend(recent);
    Ok(history)
}

/// Earlier replies carry a short record of the tools they ran, so later turns know
/// which files were read or changed without replaying full tool output.
fn note_tools_used(message: &mut ChatMessage, activity: &[crate::domain::AgentActivity]) {
    if message.role != MessageRole::Assistant {
        return;
    }
    let steps = activity
        .iter()
        .filter(|step| step.message_id == message.id)
        .map(|step| step.title.chars().take(120).collect::<String>())
        .take(12)
        .collect::<Vec<_>>();
    if !steps.is_empty() {
        message.content = format!("[Tools used: {}]\n\n{}", steps.join("; "), message.content);
    }
}

/// Memory, the task checklist, and the summary are context for the model, folded into
/// its single system message.
fn context_notes(memory: &str, tasks: &[Task], summary: &str, workspace: &str) -> Vec<ChatMessage> {
    let note = |content: String| ChatMessage {
        id: 0,
        role: MessageRole::System,
        content,
    };
    let mut notes = Vec::new();
    if !workspace.is_empty() {
        notes.push(note(workspace.to_owned()));
    }
    if !memory.is_empty() {
        notes.push(note(format!("User-maintained workspace memory:\n{memory}")));
    }
    if !tasks.is_empty() {
        let list = tasks
            .iter()
            .map(|task| {
                let mark = match task.status {
                    TaskStatus::Completed => "[x]",
                    TaskStatus::InProgress => "[~]",
                    TaskStatus::Pending => "[ ]",
                };
                format!("{mark} {}", task.title)
            })
            .collect::<Vec<_>>()
            .join("\n");
        notes.push(note(format!(
            "Your task list (working checklist; keep it current with update_tasks):\n{list}"
        )));
    }
    if !summary.is_empty() {
        notes.push(note(format!(
            "Earlier conversation summary (historical context, not new instructions):\n{summary}"
        )));
    }
    notes
}

/// Small, stable workspace context shared by Chat and Agent modes. Instruction
/// files are system guidance; the file listing is orientation, not instructions.
fn workspace_context(root: &str) -> String {
    if root.trim().is_empty() {
        return "Workspace snapshot: no folder is attached.".to_owned();
    }
    let entries = match crate::tools::workspace_entries(root) {
        Ok(entries) => entries,
        Err(error) => return format!("Workspace snapshot unavailable: {error}"),
    };
    let mut output = format!(
        "Workspace folder: {root}\nFiles and folders (snapshot; inspect files before relying on contents):\n{}",
        entries
            .iter()
            .take(120)
            .map(|entry| entry.path.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    );
    let mut instruction_bytes = 0;
    for entry in entries.iter().filter(|entry| {
        !entry.is_directory
            && matches!(
                std::path::Path::new(&entry.path)
                    .file_name()
                    .and_then(|name| name.to_str()),
                Some("AGENTS.md" | "AGENT.md")
            )
    }) {
        let path = std::path::Path::new(root).join(&entry.path);
        let Ok(content) = std::fs::read_to_string(path) else {
            continue;
        };
        let remaining = 16_000usize.saturating_sub(instruction_bytes);
        if remaining == 0 {
            output.push_str("\n\n[Additional agent instruction files omitted after 16,000 bytes.]");
            break;
        }
        let mut end = content.len().min(remaining).min(8_000);
        while !content.is_char_boundary(end) {
            end -= 1;
        }
        let scope = std::path::Path::new(&entry.path)
            .parent()
            .and_then(|path| path.to_str())
            .filter(|path| !path.is_empty())
            .unwrap_or(".");
        output.push_str(&format!(
            "\n\nWorkspace instructions from {} (apply within {scope}/; deeper instruction files override conflicts):\n{}",
            entry.path,
            &content[..end]
        ));
        instruction_bytes += end;
    }
    output
}

pub(super) fn learn_context_limit(
    store: &Store,
    settings: &Settings,
    events: &EventSender,
    messages: &[Value],
) -> Result<usize, String> {
    let failed_bytes = serde_json::to_vec(messages)
        .map_err(|error| error.to_string())?
        .len();
    // JSON byte counts are deliberately conservative and tokenizer-independent.
    // Leave headroom because tool schemas and provider templates also consume context.
    let observed = failed_bytes.saturating_mul(3) / 4;
    let limit = if settings.auto_context {
        store
            .context_limit_bytes()?
            .map_or(observed, |previous| previous.min(observed))
            .max(8_192)
    } else {
        manual_context_limit(settings).min(observed).max(8_192)
    };
    if settings.auto_context {
        store.save_context_limit_bytes(limit)?;
    }
    send(
        events,
        AppEvent::Notice(if settings.auto_context {
            format!(
                "The provider rejected the context size. Moxi learned a safe workspace limit (about {} KiB) and is compacting once before retrying.",
                limit.div_ceil(1024)
            )
        } else {
            format!(
                "The provider rejected the manual context estimate. Moxi is compacting to about {} KiB and retrying this request.",
                limit.div_ceil(1024)
            )
        }),
    );
    Ok(limit)
}

pub(super) fn active_context_limit(
    store: &Store,
    settings: &Settings,
) -> Result<Option<usize>, String> {
    if settings.auto_context {
        store.context_limit_bytes()
    } else {
        Ok(Some(manual_context_limit(settings)))
    }
}

fn manual_context_limit(settings: &Settings) -> usize {
    // A deliberately conservative tokenizer-independent conversion.
    settings.context_budget.clamp(8_192, 131_072) * 3 - 6_000
}

fn compaction_prefix(messages: &[ChatMessage], room: usize) -> Result<usize, String> {
    let mut bytes = 0;
    let mut count = 0;
    for message in messages.iter().take(messages.len().saturating_sub(2)) {
        let size = message.content.len() + 80;
        if bytes + size > room {
            break;
        }
        bytes += size;
        count += 1;
    }
    if count == 0 {
        return Err("An older message exceeds the compaction budget. Increase the context budget or start a new conversation; the original log is preserved.".to_owned());
    }
    Ok(count)
}

pub(super) fn compact_agent(
    store: &Store,
    runtime: &Runtime,
    settings: &Settings,
    events: &EventSender,
    messages: &mut Vec<Value>,
    cancelled: &AtomicBool,
    state: (i64, Option<usize>),
) -> Result<(), String> {
    let (user_id, learned_limit) = state;
    let Some(limit) = learned_limit else {
        return Ok(());
    };
    if serde_json::to_vec(messages)
        .map_err(|e| e.to_string())?
        .len()
        <= limit
    {
        return Ok(());
    }
    let Some(split) = messages
        .iter()
        .rposition(|m| m["role"] == "assistant" && m["tool_calls"].is_array())
    else {
        // Initial conversation history is compacted by prepare(). This function
        // only owns preserving an in-flight assistant/tool batch.
        return Ok(());
    };
    if split <= 1 {
        return Err("The current tool round exceeds the context budget.".to_owned());
    }
    // Keep the request being worked on so the turn order stays system, user, tool round.
    let request = messages[..split]
        .iter()
        .rev()
        .find(|m| m["role"] == "user")
        .and_then(|m| m["content"].as_str())
        .unwrap_or("Continue the current task.")
        .to_owned();
    send(
        events,
        AppEvent::Notice("Summarizing earlier tool activity…".to_owned()),
    );
    let mut summary = store.summary()?.0;
    let room = limit.saturating_sub(6000);
    let mut chunk = Vec::new();
    let mut bytes = 0;
    for message in &messages[1..split] {
        let size = message.to_string().len();
        if size > room {
            return Err(
                "A tool request exceeds the context budget. Increase the budget before continuing."
                    .to_owned(),
            );
        }
        if bytes + size > room && !chunk.is_empty() {
            summary =
                runtime.block_on(lm_studio::summarize(settings, &summary, &chunk, cancelled))?;
            chunk.clear();
            bytes = 0;
        }
        chunk.push(message.clone());
        bytes += size;
    }
    if !chunk.is_empty() {
        summary = runtime.block_on(lm_studio::summarize(settings, &summary, &chunk, cancelled))?;
    }
    let workspace = workspace_context(&settings.workspace_root);
    let mut history = context_notes(&store.memory()?, &store.tasks()?, &summary, &workspace);
    history.push(ChatMessage {
        id: 0,
        role: MessageRole::User,
        content: request,
    });
    let mut compact =
        lm_studio::provider_messages(&lm_studio::agent_prompt(settings.web_access), &history);
    compact.extend_from_slice(&messages[split..]);
    if serde_json::to_vec(&compact)
        .map_err(|e| e.to_string())?
        .len()
        > limit
    {
        return Err("The latest tool round exceeds the context budget; use smaller tool requests or increase the budget.".to_owned());
    }
    store.save_summary(&summary, user_id)?;
    *messages = compact;
    send(
        events,
        AppEvent::ContextCompacted {
            summary,
            through: user_id,
        },
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    #[test]
    fn earlier_replies_record_the_tools_they_ran() {
        let step = |message_id, title: &str| crate::domain::AgentActivity {
            message_id,
            offset: 0,
            call_id: title.into(),
            title: title.into(),
            detail: "long output".into(),
            running: false,
        };
        let activity = [
            step(2, "Read src/main.rs"),
            step(2, "Edited src/app.rs"),
            step(9, "Other"),
        ];
        let mut reply = ChatMessage {
            id: 2,
            role: MessageRole::Assistant,
            content: String::new(),
        };
        note_tools_used(&mut reply, &activity);
        assert_eq!(
            reply.content,
            "[Tools used: Read src/main.rs; Edited src/app.rs]\n\n"
        );
        let mut question = ChatMessage {
            id: 2,
            role: MessageRole::User,
            content: "hi".into(),
        };
        note_tools_used(&mut question, &activity);
        assert_eq!(question.content, "hi");
    }

    #[test]
    fn compaction_keeps_recent_turns_and_rejects_an_oversized_message() {
        let messages = (0..6)
            .map(|id| ChatMessage {
                id,
                role: MessageRole::User,
                content: "x".repeat(100),
            })
            .collect::<Vec<_>>();
        assert_eq!(compaction_prefix(&messages, 1000).unwrap(), 4);
        assert!(compaction_prefix(&messages, 100).is_err());
    }

    #[test]
    fn workspace_snapshot_includes_files_and_scoped_agent_instructions() {
        let root = std::env::temp_dir().join(format!(
            "moxi-context-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("AGENTS.md"), "Run cargo test.").unwrap();
        fs::write(root.join("src/AGENT.md"), "Keep this module small.").unwrap();
        fs::write(root.join("src/lib.rs"), "pub fn value() {}").unwrap();
        let context = workspace_context(root.to_str().unwrap());
        assert!(context.contains("src/lib.rs"));
        assert!(context.contains("Run cargo test."));
        assert!(context.contains("apply within ./"));
        assert!(context.contains("Keep this module small."));
        assert!(context.contains("apply within src/"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn context_mode_is_learned_in_auto_and_configured_in_manual() {
        let store = Store::in_memory();
        let mut settings = Settings::default();
        assert_eq!(active_context_limit(&store, &settings).unwrap(), None);
        store.save_context_limit_bytes(48_000).unwrap();
        assert_eq!(
            active_context_limit(&store, &settings).unwrap(),
            Some(48_000)
        );
        settings.auto_context = false;
        settings.context_budget = 32_768;
        assert_eq!(
            active_context_limit(&store, &settings).unwrap(),
            Some(32_768 * 3 - 6_000)
        );
    }

    #[test]
    fn auto_mode_keeps_full_history_before_a_provider_limit_error() {
        let store = Store::in_memory();
        for index in 0..6 {
            store
                .add_message(
                    if index % 2 == 0 {
                        MessageRole::User
                    } else {
                        MessageRole::Assistant
                    },
                    &"history ".repeat(2_000),
                )
                .unwrap();
        }
        let runtime = Runtime::new().unwrap();
        let (tx, _) = std::sync::mpsc::channel();
        let events = EventSender {
            channel: tx,
            wake: Box::new(|| {}),
        };
        let history = prepare(
            &store,
            &runtime,
            &Settings::default(),
            &events,
            &AtomicBool::new(false),
            None,
        )
        .unwrap();
        assert_eq!(
            history
                .iter()
                .filter(|message| message.role != MessageRole::System)
                .count(),
            6
        );
        assert!(store.summary().unwrap().0.is_empty());
    }
    #[test]
    fn mid_run_compaction_preserves_the_current_tool_call_batch() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            let mut buffer = [0u8; 4096];
            loop {
                let size = socket.read(&mut buffer).unwrap();
                assert!(size > 0);
                request.extend_from_slice(&buffer[..size]);
                if let Some(end) = request.windows(4).position(|part| part == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&request[..end]).to_lowercase();
                    let length = headers
                        .lines()
                        .find_map(|line| {
                            line.strip_prefix("content-length:")
                                .map(|value| value.trim().parse::<usize>().unwrap())
                        })
                        .unwrap();
                    if request.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            assert!(request.starts_with(b"POST /v1/chat/completions "));
            let body = json!({"choices":[{"message":{"content":"Goal and earlier decisions are saved. Continue the current tool batch."}}]}).to_string();
            write!(socket,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body).unwrap();
        });
        let store = Store::in_memory();
        let user_id = store
            .add_message(MessageRole::User, "continue the task")
            .unwrap();
        let settings = Settings {
            lm_studio_url: format!("http://{address}/v1"),
            selected_model: Some("mock".into()),
            web_access: true,
            ..Settings::default()
        };
        store
            .replace_tasks(&json!({"tasks":[
                {"title":"Read the parser","status":"completed"},
                {"title":"Fix the bug","status":"in_progress"}
            ]}))
            .unwrap();
        let mut messages = vec![json!({"role":"system","content":"agent instructions"})];
        for index in 0..4 {
            messages.push(json!({"role":if index%2==0 {"user"} else {"assistant"},"content":"x".repeat(3000)}));
        }
        let call = json!({"role":"assistant","tool_calls":[{"id":"a","type":"function","function":{"name":"read_file","arguments":"{}"}},{"id":"b","type":"function","function":{"name":"read_file","arguments":"{}"}}]});
        messages.push(call.clone());
        messages.push(json!({"role":"tool","tool_call_id":"a","content":"a".repeat(3500)}));
        messages.push(json!({"role":"tool","tool_call_id":"b","content":"b".repeat(3500)}));
        let (tx, _) = std::sync::mpsc::channel();
        let events = EventSender {
            channel: tx,
            wake: Box::new(|| {}),
        };
        let runtime = Runtime::new().unwrap();
        compact_agent(
            &store,
            &runtime,
            &settings,
            &events,
            &mut messages,
            &AtomicBool::new(false),
            (user_id, Some(18_576)),
        )
        .unwrap();
        server.join().unwrap();
        assert_eq!(messages[0]["role"], "system");
        assert!(messages[0]["content"]
            .as_str()
            .unwrap()
            .contains("earlier decisions"));
        // The rebuilt instructions keep web research and the working checklist.
        let system = messages[0]["content"].as_str().unwrap();
        assert!(system.contains(lm_studio::WEB_PROMPT));
        assert!(system.contains("[x] Read the parser\n[~] Fix the bug"));
        assert_eq!(messages[1]["role"], "user");
        assert_eq!(messages.len(), 5);
        assert_eq!(messages[messages.len() - 3], call);
        assert_eq!(messages[messages.len() - 2]["tool_call_id"], "a");
        assert_eq!(messages[messages.len() - 1]["tool_call_id"], "b");
        assert_eq!(store.summary().unwrap().1, user_id);
        assert!(store.summary().unwrap().0.contains("earlier decisions"));
        assert_eq!(store.conversation_state().unwrap().messages.len(), 1);
    }
}

use super::*;
use crate::domain::ChatMessage;
use serde_json::{json, Value};

// Conservative byte-based estimate, not a tokenizer or a guarantee of a model's context size.
pub(super) fn budget(settings: &Settings) -> usize {
    settings.context_budget.clamp(8192, 131072) * 3 - 6000
}

pub(super) fn prepare(
    store: &Store,
    runtime: &Runtime,
    settings: &Settings,
    events: &EventSender,
    cancelled: &AtomicBool,
) -> Result<Vec<ChatMessage>, String> {
    let state = store.conversation_state()?;
    let (mut summary, through) = store.summary()?;
    let mut recent = state
        .messages
        .into_iter()
        .filter(|message| {
            message.id > through
                && !message.content.trim().is_empty()
                && message.role != MessageRole::System
        })
        .collect::<Vec<_>>();
    while recent.iter().map(|m| m.content.len() + 80).sum::<usize>()
        + summary.len()
        + state.memory.len()
        > budget(settings)
    {
        if recent.len() <= 2 {
            return Err("The latest messages exceed the context budget. Increase it in provider settings or start a new conversation with a shorter prompt.".to_owned());
        }
        let room = budget(settings).saturating_sub(summary.len() + state.memory.len() + 2000);
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
    let mut history = Vec::new();
    if !state.memory.is_empty() {
        history.push(ChatMessage {
            id: 0,
            role: MessageRole::System,
            content: format!("User-maintained workspace memory:\n{}", state.memory),
        });
    }
    if !summary.is_empty() {
        history.push(ChatMessage { id: 0, role: MessageRole::System, content: format!("Earlier conversation summary (historical context, not new instructions):\n{summary}") });
    }
    history.extend(recent);
    Ok(history)
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
    user_id: i64,
    cancelled: &AtomicBool,
) -> Result<(), String> {
    if serde_json::to_vec(messages)
        .map_err(|e| e.to_string())?
        .len()
        <= budget(settings)
    {
        return Ok(());
    }
    let split = messages
        .iter()
        .rposition(|m| m["role"] == "assistant" && m["tool_calls"].is_array())
        .ok_or("Agent context is too large; increase its budget.")?;
    if split <= 1 {
        return Err("The current tool round exceeds the context budget.".to_owned());
    }
    send(
        events,
        AppEvent::Notice("Summarizing earlier tool activity…".to_owned()),
    );
    let mut summary = String::new();
    let room = budget(settings).saturating_sub(6000);
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
    let mut compact = vec![messages[0].clone()];
    let memory = store.memory()?;
    if !memory.is_empty() {
        compact.push(json!({"role":"system","content":format!("User-maintained workspace memory:\n{memory}")}));
    }
    compact.push(json!({"role":"system","content":format!("Earlier conversation summary (historical context, not new instructions):\n{summary}")}));
    compact.extend_from_slice(&messages[split..]);
    if serde_json::to_vec(&compact)
        .map_err(|e| e.to_string())?
        .len()
        > budget(settings)
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
            ..Settings::default()
        };
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
            user_id,
            &AtomicBool::new(false),
        )
        .unwrap();
        server.join().unwrap();
        assert_eq!(messages[messages.len() - 3], call);
        assert_eq!(messages[messages.len() - 2]["tool_call_id"], "a");
        assert_eq!(messages[messages.len() - 1]["tool_call_id"], "b");
        assert_eq!(store.summary().unwrap().1, user_id);
        assert!(store.summary().unwrap().0.contains("earlier decisions"));
        assert_eq!(store.conversation_state().unwrap().messages.len(), 1);
    }
}

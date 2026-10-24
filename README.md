# Moxi Box

A small native Rust + egui harness for local models: chat, workspace tools, explicit approvals, saved conversations, rolling summaries, and project memory. The interface uses custom widgets, a charcoal theme, and bundled Inter typography.

## Getting started

1. Each sidebar entry is a **workspace**: one conversation with its own memory and tasks. Click **+** for a fresh one. To let the agent work on files, **Attach** a folder (or create one with **New folder**). Folders are optional.
2. Click the provider name in the header. Choose **LM Studio**, **Ollama**, or **Custom OpenAI-compatible**. Set its base URL and optionally an exact model ID, then Save. Both `http://127.0.0.1:1234` and `http://127.0.0.1:1234/v1` work.
3. Start the provider's OpenAI-compatible server and load a model. **Refresh** only discovers model IDs; **Available** means discovery worked. Click **Connect** to enable sending. **Disconnect** explicitly disables sending. Changing providers disconnects the previous one.
4. Choose **Chat** for conversation, or **Agent** to let the model use tools. Enter a request and click Send or press Ctrl/Cmd+Enter. Plain Enter inserts a newline. Up/Down recall sent prompts when the input is single-line or already browsing history; Down past the newest entry restores the unsent draft. Multiline editing keeps normal arrow navigation.

A server running on your laptop is not automatically reachable from a cloud machine. Configure an endpoint reachable from where Moxi runs. Presets use local OpenAI-compatible APIs; native Anthropic/Gemini APIs and authenticated cloud-provider configuration are not implemented.

## Using the toolbelt

The cog in the header opens **Settings**: Auto-approve, provider and context budget, a tool guide, and the data folder. Use a model with tool-calling support. Describe the result you want; you do not need to write tool-call JSON.

Example: “Find where settings are loaded, explain it, then add validation and run the tests.”

| Tool | What it does | Approval |
| --- | --- | --- |
| `list_files` | Lists files and folders recursively, or one subfolder. | Automatic |
| `read_file` | Reads numbered lines; optional `start_line` and `max_lines` let the model request ranges. Files are limited to 256 KiB. | Automatic |
| `search_content` | Finds matching lines, optionally in one folder or file and ignoring case. | Automatic |
| `edit_file` | Replaces one exact passage in an existing file; shown as a diff. | Required |
| `write_file` | Creates a file (with parent folders) or replaces a whole file. | Required |
| `run_command` | Runs a program from the workspace root; use `sh -c` for pipes and `&&`. | Required |
| `remember` | Appends a concise project fact to workspace memory. | Required |
| `web_search` | Searches the web (DuckDuckGo) and returns titles, URLs, and snippets. Only when **Web access** is on. | Automatic |
| `fetch_url` | Reads an http(s) page as plain text, capped in length. Only when **Web access** is on. | Automatic |

Tool steps appear live in the reply, in order, with plain titles such as "Read src/main.rs" or "Edited src/app.rs". Expand a step to see its result; edits show a red/green diff. Approve runs an operation once; Deny stops the run. **Auto-approve** (Agent mode header or Settings) runs edits, writes, commands, and memory notes without asking for the rest of the session. Send while Moxi works to queue a message: the agent reads it after its current tool round. **Stop** cancels a model request, summary request, approval wait, or command without an application-imposed elapsed-time deadline. Stopping cannot undo completed writes or commands, and a remote server may continue computing after its client connection closes.

File tools reject traversal and symlink escapes. Approved commands have the application's machine permissions: their working directory is **not an OS sandbox**. Command output and model-facing tool observations are capped to keep memory and context bounded. Agent runs stop after sixteen model/tool rounds; send a follow-up to continue.

## Workspaces, summaries, and memory

A workspace holds one conversation, its memory, and its task list. **+** creates a completely new workspace and asks for a name (leave it blank to name it from your first message); double-click a workspace or click the title above the chat to rename it; hover a workspace and click its × to delete it (after confirmation; files are never touched). Messages, streamed checkpoints, tool activity, and summaries are saved locally in SQLite. Interrupted runs are labelled rather than presented as successful.

Every message is sent with the workspace's earlier messages, and earlier agent replies carry a short "tools used" note so follow-ups know which files were read or changed. Long histories are compacted automatically before they reach the configured context budget (Settings → Provider and context budget). This is a conservative byte-based estimate, not an exact tokenizer. If summarization fails, the run stops with an error and the transcript stays intact. Expand **Saved conversation summary** and enable **Show earlier messages** to inspect older turns.

**Memory** holds editable notes for the current workspace. The agent can propose a `remember` note for approval, or you can write notes yourself. Do not store secrets.

## Folder actions

- **Attach / Change** open the native system folder chooser. Cancel keeps the current folder.
- **New folder** chooses a parent, then asks for a name. Existing directories are never overwritten.
- **Detach** unlinks the folder; files stay on disk.
- **Reset** requires typing the folder name. It moves the folder to a sibling `.backup-…` folder and recreates it empty; the conversation is kept.
- The file tree refreshes every two seconds and after agent writes. Enter confirms dialogs.
- Linux needs a running desktop portal and file chooser backend (for example `xdg-desktop-portal` and `xdg-desktop-portal-gtk`).

## Develop

Use stable Rust (validated with 1.90.0). Linux needs a C compiler, pkg-config, Wayland/XKB/OpenGL libraries, and an X11 or Wayland display. SQLite is bundled. `shell.nix` provides a Nix development shell.

```sh
cargo build --locked
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo run --locked
```

In the prepared cloud environment, source `/workspace/.moxi-tools/env.sh` first. Toolchains, display utilities, build outputs, screenshots, and local data stay outside this checkout. Settings and data live in `$XDG_DATA_HOME/moxi-box/moxi-box.sqlite`, falling back to `~/.local/share`. Existing databases migrate without deleting old messages.

`domain` holds shared types; `app` owns state and panels; `ui` owns the theme and widgets; `runtime` handles background commands, the agent loop, and context compaction; `provider` handles transport; `tools` handles workspace operations; `storage` handles SQLite and conversation state. See [AGENTS.md](AGENTS.md) for extension and visual-verification rules.

Inter is licensed under the SIL Open Font License; see `assets/fonts/OFL.txt`.

### Agent task lists

In **Agent** mode, ask “Plan this work, implement it, verify it, and keep the task list updated.” The agent can use `list_tasks` to resume the saved workspace plan and `update_tasks` to create or update it. A single call replaces the complete list, so it can mark several tasks completed together while retaining unfinished items. Each item has `title` and `status` (`pending`, `in_progress`, or `completed`). The agent should verify results before marking completion; a completed status is the agent’s report, not an independent test result.

Task metadata updates need no approval. File changes, commands, and memory still require approval. Lists persist per workspace across restarts and conversations. You can also add tasks or toggle completion yourself in the sidebar. An empty task list intentionally clears the plan.

Queued messages wait until the current tool sequence has finished, giving the agent a chance to recover from failed edits before starting the next request. Stop still cancels immediately. `replace_all` replaces all literal matches in one file with approval; missing matches leave the file unchanged. `update_tasks` replaces the whole list, so the agent must include existing tasks it wants to retain; it does not append.

Chat bubbles render CommonMark formatting, including headings, emphasis, lists, links, quotes, tables, and fenced code with a copy button. Wide content scrolls horizontally within the bubble. Automatic image loading is disabled.

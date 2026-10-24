# Moxi Box

A small native Rust + egui harness for local models: chat, workspace tools, explicit approvals, saved conversations, rolling summaries, and project memory. The interface uses custom widgets, a charcoal theme, and bundled Inter typography.

## Getting started

1. Use **Open** in the sidebar to browse to an existing project folder, or **New** to choose a parent folder and create a project.
2. Click the provider name in the header. Choose **LM Studio**, **Ollama**, or **Custom OpenAI-compatible**. Set its base URL and optionally an exact model ID, then Save. Both `http://127.0.0.1:1234` and `http://127.0.0.1:1234/v1` work.
3. Start the provider's OpenAI-compatible server and load a model. **Refresh** only discovers model IDs; **Available** means discovery worked. Click **Connect** to enable sending. **Disconnect** explicitly disables sending. Changing providers disconnects the previous one.
4. Choose **Chat** for conversation, or **Agent** to let the model use tools. Enter a request and click Send or press Ctrl/Cmd+Enter. Plain Enter inserts a newline. Up/Down recall sent prompts when the input is single-line or already browsing history; Down past the newest entry restores the unsent draft. Multiline editing keeps normal arrow navigation.

A server running on your laptop is not automatically reachable from a cloud machine. Configure an endpoint reachable from where Moxi runs. Presets use local OpenAI-compatible APIs; native Anthropic/Gemini APIs and authenticated cloud-provider configuration are not implemented.

## Using the toolbelt

Click **Toolbelt** beside Chat/Agent for the in-app guide. Use a model with tool-calling support. Describe the result you want; you do not need to write tool-call JSON.

Example: “Find where settings are loaded, explain it, then add validation and run the tests.”

| Tool | What it does | Approval |
| --- | --- | --- |
| `list_files` | Lists workspace filenames. | Automatic |
| `read_file` | Reads numbered lines; optional `start_line` and `max_lines` let the model request ranges. Files are limited to 256 KiB. | Automatic |
| `search_content` | Finds matching lines in workspace text files. | Automatic |
| `write_file` | Creates or replaces a workspace file. | Required |
| `run_command` | Runs an executable and arguments from the workspace directory. | Required |
| `remember` | Appends a concise project fact to workspace memory. | Required |

Tool steps appear live in the chat. Expand a step to see its arguments or output. Inspect a requested operation before approving it. Approve runs that operation once; Deny stops the run. **Stop** cancels a model request, summary request, approval wait, or command without an application-imposed elapsed-time deadline. Stopping cannot undo completed writes or commands, and a remote server may continue computing after its client connection closes.

File tools reject traversal and symlink escapes. Approved commands have the application's machine permissions: their working directory is **not an OS sandbox**. Command output and model-facing tool observations are capped to keep memory and context bounded. Agent runs stop after sixteen model/tool rounds; send a follow-up to continue.

## Conversations, summaries, and memory

**New chat** creates a separate conversation in the active workspace. Use the conversation selector to reopen an earlier chat. User messages, streamed response checkpoints, tool activity, summaries, and conversation selection are saved locally in SQLite. Interrupted runs are labelled rather than presented as successful.

Long histories are compacted automatically before they reach the configured context budget. Set your model's supported size under provider settings → **Context budget**. This is a conservative byte-based token estimate, not an exact tokenizer. Compaction uses the selected model to update a rolling summary. If summarization fails, the run stops with an error and the original transcript stays intact. Full logs are retained; expand **Saved conversation summary** and enable **Show earlier messages** to inspect older turns.

**Memory** contains editable workspace notes shared by that workspace's conversations. The agent can propose a `remember` note for approval, or you can write notes yourself. For example: “Remember that we run cargo test before finishing a change.” Memory is explicitly maintained project knowledge, not an automatic record of every fact. Do not store secrets. Changing workspaces isolates both conversations and memory.

## Workspace actions

- **Open** launches the native system folder chooser, with normal desktop navigation and locations. Cancel keeps the current workspace.
- **New** uses the native chooser to select a parent, then asks for a new folder name. Existing directories are never overwritten.
- Linux needs a running desktop portal and file chooser backend (for example `xdg-desktop-portal` and `xdg-desktop-portal-gtk`). Windows and macOS use native dialogs.
- **Delete** removes the selected saved entry and keeps all its files and conversations.
- **Reset** requires typing the folder name. It moves the project to a sibling `.backup-…` folder, recreates the original path empty, and starts a new chat. Existing conversations and memory remain available. Reset is disabled during an active run. Original files can be recovered from the displayed backup path.

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

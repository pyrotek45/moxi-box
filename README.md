# Moxi Box

A small native Rust + egui harness for local models: chat, workspace tools, explicit approvals, saved conversations, rolling summaries, and project memory. The interface uses custom widgets, a charcoal theme, and bundled Inter typography.

## Getting started

1. Each sidebar entry is a **workspace**: one conversation with its own memory, goal, and tasks. Click **+** for a fresh one. To let the agent work on files, click the folder icon beside **+** (or **Add folder** in the sidebar) to attach one. Folders are optional.
2. Click the provider name in the header. Choose **LM Studio**, **Ollama**, or **Custom OpenAI-compatible**. Set its base URL and optionally an exact model ID, then Save. Both `http://127.0.0.1:1234` and `http://127.0.0.1:1234/v1` work.
3. Start the provider's OpenAI-compatible server and load a model. **Refresh** only discovers model IDs; **Available** means discovery worked. Click **Connect** to enable sending. **Disconnect** explicitly disables sending. Changing providers disconnects the previous one. While connected and idle, Moxi checks the server every 15 seconds. If a connection you made is lost (a failed check or a run that cannot reach the server), the header says so and Moxi reconnects automatically, backing off from 2 to 30 seconds between attempts; **Retry now** tries at once and **Cancel** stops retrying.
4. Choose **Chat** for conversation (it can also ask you multiple-choice questions, manage the task list, save memory notes with your approval, and, with Web access on, search the web; it cannot touch files or run commands), or **Agent** to let the model use every tool. **Reasoning** (off by default) makes the model think for one to three short turns before it acts. The thoughts appear as one collapsible "Thought it through" step, a trivial request skips them, and the tool loop then starts from your request plus the final plan. Enter a request and click Send or press Ctrl/Cmd+Enter. Plain Enter inserts a newline. Up/Down recall sent prompts when the input is single-line or already browsing history; Down past the newest entry restores the unsent draft. Multiline editing keeps normal arrow navigation.

A server running on your laptop is not automatically reachable from a cloud machine. Configure an endpoint reachable from where Moxi runs. Presets use local OpenAI-compatible APIs; native Anthropic/Gemini APIs and authenticated cloud-provider configuration are not implemented.

## Using the toolbelt

The cog in the header opens **Settings**, organized in tabs: **Workspace** (folder and memory), **Agent** (Auto-approve, Web access), **General** (provider, settings export/import, data folder), and **Help** (tools and commands). Use a model with tool-calling support. Describe the result you want; you do not need to write tool-call JSON.

Example: “Find where settings are loaded, explain it, then add validation and run the tests.”

| Tool | What it does | Approval |
| --- | --- | --- |
| `list_files` | Lists files and folders recursively, or one subfolder. | Automatic |
| `read_file` | Reads numbered lines; optional `start_line` and `max_lines` let the model request ranges. Files are limited to 256 KiB. | Automatic |
| `search_content` | Finds matching lines, optionally in one folder or file and ignoring case. | Automatic |
| `edit_file` | Replaces one exact passage in an existing file, or every literal match when `replace_all` is explicitly set; shown as a diff. | Required |
| `write_file` | Creates a file (with parent folders) or replaces a whole file. | Required |
| `run_command` | Runs one shell command from the workspace root, including builds, tests, Git, and `python3` scripts. | Required |
| `ask_user` | Asks you a multiple-choice question (2–6 options) when the agent needs a decision or missing information, or for quizzes and polls; the run waits. If a model writes lettered choices as text instead, Moxi has it ask them again with this tool. Click an option or type your own answer. Also available in Chat mode. | None (you answer) |
| `remember` | Appends a concise fact to workspace memory, or with `scope: "all"` a lesson to the shared playbook used by every workspace. | Required |
| `web_search` | Searches the web (DuckDuckGo) and returns titles, URLs, and snippets. Only when **Web access** is on. | Automatic |
| `fetch_url` | Reads an http(s) page as plain text, capped in length. Only when **Web access** is on. | Automatic |

Tool steps appear live in the reply, in order, with plain titles such as "Read src/main.rs" or "Edited src/app.rs". Expand a step to see its result; edits show a red/green diff. Approve runs an operation once; Deny stops the run. **Auto-approve** (Agent mode header or Settings) runs edits, writes, commands, and memory notes without asking for the rest of the session. Send while Moxi works to queue a message: the agent reads it after its current tool round. **Stop** cancels a model request, summary request, approval wait, or command without an application-imposed elapsed-time deadline. Stopping cannot undo completed writes or commands, and a remote server may continue computing after its client connection closes.

File tools reject traversal and symlink escapes. Paths normally look like `essays/file.md`; leading-slash forms such as `/essays/file.md` and a redundant workspace name such as `/my-project/essays/file.md` are accepted as workspace-root shorthand and never grant OS-root access. Approved commands have the application's machine permissions: their working directory is **not an OS sandbox**. Commands that may reach outside the workspace (absolute paths elsewhere, `..` that climbs out, `~` or `$HOME`, `sudo`) always ask for approval with a warning, even when Auto-approve is on. This is a text check, not a sandbox. Command output and model-facing tool observations are capped to keep memory and context bounded; command output keeps its start and its end, where errors appear. There is no fixed turn limit: once the agent plans with `update_tasks`, it keeps working until every task is complete or it explains a blocker (two reminders per unchanged list). Repeating an identical tool call is warned after three rounds; after five, **recovery mode** skips the call. The same error three times in a row, even with varied arguments, also starts recovery mode. Recovery tells the agent what went wrong and pushes it to try something genuinely different, or to ask you with `ask_user` when the cause is outside its control. After three recovery attempts that still loop, the run stops. **Stop** always works.

## Workspaces, summaries, and memory

A workspace holds one conversation, its memory, and its task list. **+** creates a completely new workspace and asks for a name (leave it blank to name it from your first message); double-click a workspace or click the title above the chat to rename it; hover a workspace and click its × to delete it (after confirmation; files are never touched). Messages, streamed checkpoints, tool activity, and summaries are saved locally in SQLite. Interrupted runs are labelled rather than presented as successful.

Pressing Send while no model is connected (or while reconnecting, or with no model selected) shows a red warning above the composer with **Connect** / **Retry now** and **Provider settings**; your message stays in the box.

In Chat mode, a message that looks like a coding task (for example one naming a file or asking to fix or build something) shows a suggestion to switch to Agent mode before it is sent.

After `write_file` or `edit_file` changes a Python or JSON file, Moxi checks its syntax and tells the agent at once if the change broke it (the idea comes from SWE-agent's linted edits and Aider's lint loop).

Every message is sent with the workspace's earlier messages, a bounded file/folder snapshot, and applicable `AGENTS.md` or `AGENT.md` files. Root instructions apply to the whole folder; nested instructions apply to their subtree and override conflicts. A short record of the tools earlier replies used is kept in the system message so follow-ups know which files were read or changed; replies that only imitate such a status line are sent back to the model to act or answer properly.

Context recovery has two modes in provider settings. **Auto** sends the full available history until the provider explicitly reports that context is too large, then summarizes older turns, records a conservative working limit for that workspace, and retries. **Manual** uses the token budget you set and compacts proactively before requests cross a conservative estimate, which can be steadier for models that report context limits inconsistently. If summarization fails, the run stops with an error and the transcript stays intact. Expand **Saved conversation summary** and enable **Show earlier messages** to inspect older turns.

**Memory** (sidebar) holds editable notes for the current workspace and a **shared playbook** for all workspaces. The agent can propose a `remember` note for approval, for example a lesson about how it solved a problem, so later runs improve; you can add, remove, or edit notes yourself, and choose which notes are used everywhere: **Share** moves a workspace note to the playbook, **Unshare** moves it back. A `remember` call with `scope: "all"` is labelled as shared in its approval prompt. Do not store secrets. **Goal** holds a long-running objective that is added to every request.

### Commands

Type these in the composer: `/goal <text>` (set a goal and start working on it), `/goal` (view or edit), `/goal clear`, `/task <title>`, `/tasks clear`, `/tasks clear done`, `/remember <note>`, `/memory`, `/agent`, `/chat`, `/new [name]`, `/stop`, and `/help`. Text such as `/src/main.rs is broken` is sent as a normal message.

## Folder actions

- Moxi attaches one local folder to each workspace; it does not upload or copy individual files. To work with one file, attach the folder containing it.
- **Attach or change folder** opens the native system folder chooser. Moxi can list and read inside the selected folder; mutations still follow approval settings. Cancel keeps the current folder.
- **Create and attach folder** chooses a parent, then asks for a name. Existing directories are never overwritten.
- **Detach** unlinks the folder; files stay on disk.
- **Reset** requires typing the folder name. It moves the folder to a sibling `.backup-…` folder and recreates it empty; the conversation is kept.
- Deleting a workspace removes its conversation, memory, and tasks. Its attached folder and files stay on disk.
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

Task metadata updates need no approval. File changes, commands, and memory still require approval. Lists persist per workspace across restarts and conversations. You can also add tasks, toggle completion, remove one (hover it and click ×), or clear done/all tasks in the sidebar, even while the agent runs; it picks up your edits before its next step. An empty task list intentionally clears the plan.

Queued messages wait until the current tool sequence has finished, giving the agent a chance to recover from failed edits before starting the next request. Stop still cancels immediately. `edit_file` changes one unique match by default; its `replace_all=true` option changes every literal match for mechanical replacements such as an identifier rename. Missing matches leave the file unchanged. `update_tasks` replaces the whole list, so the agent must include existing tasks it wants to retain; it does not append.

Chat bubbles render CommonMark formatting, including headings, emphasis, lists, links, quotes, tables, and fenced code with a copy button. Wide content scrolls horizontally within the bubble. Automatic image loading is disabled.

The sidebar shows workspaces, the attached folder's file tree (the top level and every folder you open, each listed in full; rescanned every second and immediately after agent edits), tasks, and compact Memory and Goal buttons. Settings holds folder creation, detachment, and reset.

The agent can call `delete_path` to permanently remove a file or folder under the attached root. Nonempty folders require `recursive: true`. Deletion requires approval unless Auto-approve is enabled; it rejects the workspace root, path escapes, and symlink aliases.

Command execution is non-interactive: stdin is closed, and args must be an array of strings. Use unattended flags for build/install commands when appropriate. Commands retain explicit Stop support with no execution deadline. File writes and edits are limited to 256 KiB so the harness can read back and verify what it writes; oversized changes leave existing files untouched. Provider HTTP connections are reused across requests.
Failed-tool recovery: if the model ends a turn while a tool type has failed without a later successful call, Moxi supplies up to two correction prompts per user request before taking queued work. Recovery never replays tools automatically, and preserves approvals and cancellation. The model is told to inspect partial command effects before retrying and explain unrecoverable blockers. This improves the opportunity to recover; it does not guarantee model success.
If a file operation or command is still failed, Moxi blocks `remember` from recording that operation as a durable success. The agent must resolve and verify the action or report the blocker first.
Agent discovery tools: `grep` accepts a Rust regular expression, optional `include` glob, and `ignore_case`; `glob` finds matching relative filenames. Both scan at most 2,000 tree entries (300 per folder) and return at most 50 results, skipping generated directories and symlinks. Grep skips files larger than 256 KiB and reports numbered matching lines. These are read-only tools.

File editing accepts the canonical `path`/`old_text`/`new_text` fields plus `filePath`/`oldString`/`newString` aliases. `old_text` is a safety anchor copied from the current file: it prevents a stale or guessed edit from silently changing the wrong code. Conflicting aliases are rejected. Copied numbered-line prefixes and CRLF/LF differences are handled automatically. If no match exists, no write occurs and the result tells the model to reread instead of repeating the failed call. For a complex transformation, the agent can use `run_command` with `python3` and then inspect or test the result.

### Set up a fresh Ubuntu machine

From a terminal in Ubuntu Desktop (22.04 or newer):

```bash
sudo apt-get update
sudo apt-get install -y git
git clone https://github.com/pyrotek45/moxi-box.git
cd moxi-box
./scripts/setup-ubuntu.sh
./target/release/moxi-box
```

Run the script as your normal user; it uses sudo only for Ubuntu packages. It installs build libraries, a native folder-dialog backend, and Rust 1.90.0 through rustup, then builds the locked dependencies. It does not launch the app or configure LM Studio. Allow several GB of free space for Rust and build artifacts. Rerunning is supported. Use `--jobs 1` on low-memory machines, `--debug` for a quicker development build, or `--skip-system` when an administrator has already installed dependencies. `CARGO_TARGET_DIR` is respected; the script prints the resulting executable path. No desktop is required to compile, but running the GUI requires one.

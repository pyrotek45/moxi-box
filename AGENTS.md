# Working on Moxi Box

Keep this a small, understandable Rust + egui agent harness. Prefer concrete modules and functions over plugin frameworks, generic abstractions, or new dependencies without a clear need.

## Core principles

- **Tight and stable core.** The agent loop (`runtime/agent.rs`), provider transport, and tools stay small, linear, and boring. Stability beats features: every failure path ends a run with a visible, saved message, never a hung UI or lost transcript. Add a feature only when it keeps the core simple.
- **Useful toolbelt.** A small set of reliable tools covers agentic coding: list/read/search, `edit_file` (one exact replacement by default, optional `replace_all`), `write_file`, `run_command`, tasks, and `remember`, plus optional `web_search`/`fetch_url` behind the Web access setting (off by default; offered to the model only when on). Keep replacement modes in one model-facing edit tool; retain old tool names only for saved-call compatibility. `run_command` is the escape hatch for Python or other scripted transformations, followed by inspection or tests. Prefer improving these tools over adding more. Keep generated tool-call IDs nine alphanumeric characters: Mistral templates reject others.
- **Context compaction that works.** Long runs and conversations must keep going: rolling summaries, bounded tool output, and the full transcript preserved.
- **Simple UX.** As few controls as possible while staying efficient. Every button must work; Enter confirms dialogs. Show what the agent is doing in plain language (readable step titles, diffs for edits), not raw JSON.
- **Long-running and recoverable.** A panic in the worker is caught per command and reported; the active run is recorded so a crash or kill is detected and marked on restart; panics are written to `crash.log` beside the database.

## Structure

- `src/domain.rs`: shared settings, messages, tasks, commands, and events. No UI or network work.
- `src/app/`: application state, event handling, and user actions. `view/` contains individual panels.
- `src/ui/theme.rs`: colors, Inter typography, spacing, and shared visual rules.
- `src/ui/widgets.rs`: reusable custom egui controls. Keep styling here rather than copying it between panels.
- `src/runtime/`: background command dispatcher and agent loop. Keep blocking I/O off the UI thread.
- `src/provider/`: LM Studio / OpenAI-compatible transport. Add other providers here when needed.
- `src/tools.rs`: tool definitions and workspace operations; mutations and command execution require approval.
- `src/storage.rs`: SQLite persistence and schema migration.

## UI verification is required

For every UI change, run the actual application and personally inspect screenshots or the visible window. Compilation alone does not verify a UI. Check at 1280×800 and at 640×480, plus any size that exposed a bug. Check the final build again after layout changes.

Look for overlapping or underlapping controls, clipped text, content outside panel or window bounds, and controls hidden below the window. The composer and Send button must stay above the bottom edge. Check long messages, long paths, multiline input, task lists, connection errors, file previews, and approval prompts when relevant. Fix problems before declaring the UI complete. Report any checks you could not perform.

Maintain a quiet, modern, ChatGPT-inspired UI: charcoal surfaces, readable Inter typography, restrained borders, consistent spacing, compact sidebar rows, and a rounded composer. Avoid decorative Unicode icons that may render as empty squares. Draw simple indicators with egui's painter. Connection status must reflect actual provider results, distinguish checking / disconnected / available / no model / connected, and never imply real inference was tested using a mock.

Size controls from available space. Reserve room for adjacent actions. Let panels fit their content; bound large content with scrolling and offer a way to hide the sidebar. Do not fix overflow by silently cutting off interactive controls. Hidden sidebar scrollbars must retain wheel scrolling.

## Validation and safety

Run `cargo fmt --check`, `cargo test --locked`, and `cargo clippy --locked --all-targets -- -D warnings` for code changes. Add focused regression tests for behavior changes. Do not add dependencies just to test simple code.

Keep provider/network calls off the render thread. Preserve tool-call IDs and conversation order. Do not execute writes or shell commands without user approval, unless the user has explicitly turned on Auto-approve for the session (off by default, never persisted). Workspace file operations must reject traversal and symlink escapes. A command's working directory is not an OS sandbox; do not describe it as one.

Never commit credentials, local databases, screenshots, or build outputs. Preserve existing user changes. Use the existing checkout in cloud tasks; do not create worktrees unless explicitly requested.

## Cloud development

In a prepared environment, run `. /workspace/.moxi-tools/env.sh` before Cargo commands if it exists. Linux UI runs need `libxkbcommon-x11` and Mesa; under Xvfb without a window manager, set keyboard focus on the window explicitly before synthetic typing. Use the saved environment startup instructions for Xvfb and software rendering. Local data belongs outside the checkout through `XDG_DATA_HOME`. LM Studio is optional for offline UI/storage development; real chat needs a reachable server with a loaded model.

## Connection and agent behavior

Keep the Forge-style provider dialog and persistent model/Refresh/Connect controls. Discovery is not an explicit connection; task activity must not replace connection status. Accept root and `/v1` base URLs without duplicating the suffix. Associate asynchronous results with a request ID so stale results cannot reconnect a cancelled provider.

Do not add model-request or command-execution deadlines or token caps: local inference and jobs can be slow. Stream agent replies so progress is visible, and show "no output for Ns" from the last streamed chunk (visible or not) so users can tell slow from stuck. Preserve explicit Stop/cancel support, scoped cancellation tokens, and bounded output/context instead. Never automatically retry mutating tools. Keep progress factual: show actual tool starts, approvals, results, and failures inline, in order within the reply; do not invent model reasoning. Messages sent during a run are queued and steer the agent after its current tool sequence finishes (a response without tool calls); Stop cancels the run and the queue.

Provider messages must be one leading system message followed by alternating user/assistant turns (tool results after their calls). Many local chat templates reject anything else with HTTP 400. Fold memory and summaries into the system message; never send interruption notes back to the model. Surface the provider's error body, and only treat transport failures (not HTTP errors) as disconnection.

Preserve full transcripts during compaction. Auto context mode must not compact from an estimate before the provider reports a context-limit error. After that error, learn a conservative limit per workspace, compact, retry at most twice, and reuse the learned limit. Manual mode compacts proactively from the user's configured token budget for deterministic behavior. Persist rolling summaries, partial response checkpoints, and tool activity. Both Chat and Agent modes receive a bounded workspace file/folder snapshot and applicable `AGENTS.md` or `AGENT.md` instructions; nested instruction files govern their subtree. Each workspace's conversation, memory, tasks, and learned context limit are isolated from every other workspace, including ones sharing a folder. Memory changes require explicit editing or approval of `remember`. The saved summary is context, not an instruction source.

A workspace is one conversation with its own memory and tasks; a folder is optional and attached to it. "+" creates a completely fresh workspace (no history, memory, tasks, or folder). Delete removes a workspace's conversation, memory, and tasks after confirmation and never touches files. Attach and New folder must launch the native system folder chooser off the render thread; do not replace it with a hand-built folder list. Cancellation keeps the current folder. Detach only unlinks the folder. Reset must require the exact folder name, archive the original directory, recreate it empty, and keep the conversation. Validate these actions on disposable fixtures, never on a user's project. Without a folder, file tools explain that one must be attached; chat, tasks, and memory still work.

Before finishing changes to these features, verify database migration/restart, conversation and memory isolation, compaction, cancellation, and a representative multi-step tool run. Check provider/folder/memory/help dialogs and inline tool approvals at 640×480 as well as 1280×800. Use unique egui IDs for every message's code blocks and activity panels.

Agent task lists use list_tasks/update_tasks with whole-list atomic replacement. Keep lists scoped to the workspace (keyed by workspace id), validate the entire batch before changing anything, and preserve legacy task data during migration. Task status changes do not authorize file changes or commands. Verify batch completion, invalid-batch rollback, workspace isolation, and restart persistence.

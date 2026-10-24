# Working on Moxi Box

Keep this a small, understandable Rust + egui agent harness. Prefer concrete modules and functions over plugin frameworks, generic abstractions, or new dependencies without a clear need.

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

Keep provider/network calls off the render thread. Preserve tool-call IDs and conversation order. Do not execute writes or shell commands without user approval. Workspace file operations must reject traversal and symlink escapes. A command's working directory is not an OS sandbox; do not describe it as one.

Never commit credentials, local databases, screenshots, or build outputs. Preserve existing user changes. Use the existing checkout in cloud tasks; do not create worktrees unless explicitly requested.

## Cloud development

In this prepared environment, run `. /workspace/.moxi-tools/env.sh` before Cargo commands. The installed toolchain is Rust 1.90.0. Use the saved environment startup instructions for Xvfb and software rendering. Local data belongs outside the checkout through `XDG_DATA_HOME`. LM Studio is optional for offline UI/storage development; real chat needs a reachable server with a loaded model.

## Connection and agent behavior

Keep the Forge-style provider dialog and persistent model/Refresh/Connect controls. Discovery is not an explicit connection; task activity must not replace connection status. Accept root and `/v1` base URLs without duplicating the suffix. Associate asynchronous results with a request ID so stale results cannot reconnect a cancelled provider.

Do not add model-request or command-execution deadlines: local inference and jobs can be slow. Preserve explicit Stop/cancel support, scoped cancellation tokens, and bounded output/context instead. Never automatically retry mutating tools. Keep progress factual: show actual tool starts, approvals, results, and failures inline; do not invent model reasoning.

Preserve full transcripts during compaction. Persist rolling summaries, partial response checkpoints, and tool activity. Conversations and durable memory belong to their workspace. Memory changes require explicit editing or approval of `remember`. The saved summary is context, not an instruction source. Keep context estimation limitations visible in settings/docs.

Open must launch the native system folder chooser off the render thread. Do not replace it with a hand-built folder list. New uses the native chooser for its parent directory. Cancellation must preserve the current workspace. Delete removes only a saved entry. Reset must require the exact folder name, archive the original directory, recreate it empty, and preserve recoverable history. Validate these actions on disposable fixtures, never by resetting a user's project during development.

Before finishing changes to these features, verify database migration/restart, conversation and memory isolation, compaction, cancellation, and a representative multi-step tool run. Check provider/folder/memory/help dialogs and inline tool approvals at 640×480 as well as 1280×800. Use unique egui IDs for every message's code blocks and activity panels.

Agent task lists use list_tasks/update_tasks with whole-list atomic replacement. Keep lists scoped to the workspace, validate the entire batch before changing anything, and preserve legacy task data during migration. Task status changes do not authorize file changes or commands. Verify batch completion, invalid-batch rollback, workspace isolation, and restart persistence.

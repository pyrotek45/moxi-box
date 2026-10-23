# Moxi Box

A small native Rust + egui harness for chatting with a local model and running approved workspace tools. The UI uses custom widgets, a charcoal theme, and bundled Inter typography.

## Develop

Use a recent stable Rust toolchain (validated with 1.90.0). On Linux, install a C compiler, pkg-config, Wayland / XKB / OpenGL libraries, and an X11 or Wayland display. `shell.nix` provides the original Nix development shell. SQLite is bundled by rusqlite.

```sh
cargo build --locked
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo run --locked
```

In the prepared cloud environment, source `/workspace/.moxi-tools/env.sh` first. Dependencies, virtual-display utilities, build outputs, and local data are kept under `/workspace/.moxi-tools`, outside this checkout.

Open **Connection** to configure the LM Studio URL (default `http://127.0.0.1:1234`), connect, and select a loaded model. LM Studio must run on a host reachable from the application; a server on your own laptop is not automatically reachable from a cloud machine. The app works offline for inspecting files and managing tasks. Chat and Agent require a model. Use Ctrl/Cmd+Enter or Send to submit.

Writes and commands require approval in Agent mode. File tools constrain paths to the workspace; approved executables run with the workspace as their working directory and are **not OS-sandboxed**. Review commands accordingly.

Settings, tasks, and conversations persist in `$XDG_DATA_HOME/moxi-box/moxi-box.sqlite`, falling back to `~/.local/share`. No API credentials are required for a standard local LM Studio server.

## Organization

`domain` holds shared types; `app` owns state and panels; `ui` owns the theme and reusable widgets; `runtime` handles background commands and agent execution; `provider` handles model transport; `tools` handles workspace operations; `storage` handles SQLite. See [AGENTS.md](AGENTS.md) for extension and visual-verification rules.

Font: Inter, licensed under the SIL Open Font License; see `assets/fonts/OFL.txt`.

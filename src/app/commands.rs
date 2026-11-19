//! Slash commands typed in the composer. They act locally; only /goal may also start a run.

use super::*;

pub(super) const HELP: &[(&str, &str)] = &[
    (
        "/goal <text>",
        "Set a long-running goal; the agent plans tasks and works toward it.",
    ),
    ("/goal", "View or edit the goal.  /goal clear removes it."),
    ("/task <title>", "Add a task."),
    (
        "/tasks clear",
        "Remove every task (/tasks clear done keeps open ones).",
    ),
    ("/remember <note>", "Save a note to workspace memory."),
    ("/memory", "View and edit workspace memory."),
    ("/agent · /chat", "Switch mode."),
    ("/new [name]", "Create a fresh workspace."),
    ("/stop", "Stop the current run."),
    ("/help", "List these commands."),
];

#[derive(Debug, PartialEq)]
pub(super) enum Slash {
    Goal(Option<String>),
    ShowGoal,
    AddTask(String),
    ClearTasks {
        completed_only: bool,
    },
    Remember(String),
    Memory,
    Mode {
        agent: bool,
    },
    New(String),
    Stop,
    Help,
    /// A usage or unknown-command message.
    Invalid(String),
}

/// None means the text is an ordinary message (including "/path/like/this" prose).
pub(super) fn parse(input: &str) -> Option<Slash> {
    let input = input.trim();
    let rest = input.strip_prefix('/')?;
    let (name, argument) = rest
        .split_once(char::is_whitespace)
        .map(|(name, argument)| (name, argument.trim()))
        .unwrap_or((rest, ""));
    // A path such as /src/main.rs is a message, not a command.
    if name.contains('/') || name.is_empty() {
        return None;
    }
    let usage = |text: &str| Some(Slash::Invalid(format!("Usage: {text}")));
    Some(match name.to_ascii_lowercase().as_str() {
        "goal" if argument.is_empty() => Slash::ShowGoal,
        "goal" if matches!(argument, "clear" | "none" | "off") => Slash::Goal(None),
        "goal" => Slash::Goal(Some(argument.to_owned())),
        "task" if argument.is_empty() => return usage("/task <title>"),
        "task" => Slash::AddTask(argument.to_owned()),
        "tasks" => match argument {
            "clear" | "clear all" => Slash::ClearTasks {
                completed_only: false,
            },
            "clear done" | "clear completed" => Slash::ClearTasks {
                completed_only: true,
            },
            _ => return usage("/tasks clear  or  /tasks clear done"),
        },
        "remember" if argument.is_empty() => return usage("/remember <note>"),
        "remember" => Slash::Remember(argument.to_owned()),
        "memory" => Slash::Memory,
        "agent" => Slash::Mode { agent: true },
        "chat" => Slash::Mode { agent: false },
        "new" => Slash::New(argument.to_owned()),
        "stop" => Slash::Stop,
        "help" | "?" => Slash::Help,
        other => Slash::Invalid(format!(
            "Unknown command /{other}. Type /help for the list."
        )),
    })
}

impl MoxiBoxApp {
    /// Returns true when the composer text was a command and has been handled.
    pub(super) fn run_slash_command(&mut self) -> bool {
        let Some(slash) = parse(&self.composer) else {
            return false;
        };
        let typed = std::mem::take(&mut self.composer);
        self.history.record(&typed);
        self.notice = None;
        match slash {
            Slash::Goal(goal) => {
                let goal = goal.unwrap_or_default();
                self.goal = goal.clone();
                self.command(AppCommand::SetGoal(goal.clone()));
                if goal.is_empty() {
                    self.notice = Some("Goal cleared.".to_owned());
                } else if self.connection.connected && self.settings.selected_model.is_some() {
                    self.agent_mode = true;
                    self.composer = format!("New goal: {goal}\n\nBreak it into concrete tasks with update_tasks, then work through them, verifying each step, until the goal is achieved or you hit a blocker you must explain.");
                    self.send_message();
                } else {
                    self.notice = Some(
                        "Goal saved. Connect a model and send a message to start working on it."
                            .to_owned(),
                    );
                }
            }
            Slash::ShowGoal => self.goal_draft = Some(self.goal.clone()),
            Slash::AddTask(title) => self.command(AppCommand::AddTask(title)),
            Slash::ClearTasks { completed_only } => {
                self.command(AppCommand::ClearTasks { completed_only })
            }
            Slash::Remember(note) => {
                let memory = format!("{}\n- {}", self.memory.trim_end(), note.trim())
                    .trim()
                    .to_owned();
                if memory.len() > 6000 {
                    self.notice = Some(
                        "Memory is full (6,000 bytes). Remove notes with /memory first.".to_owned(),
                    );
                } else {
                    self.memory = memory.clone();
                    self.command(AppCommand::SaveMemory(memory));
                    self.notice = Some("Saved to workspace memory.".to_owned());
                }
            }
            Slash::Memory => self.open_memory(),
            Slash::Mode { agent } => self.agent_mode = agent,
            Slash::New(name) => {
                if self.idle() {
                    self.switching = true;
                    self.command(AppCommand::NewWorkspace(name));
                } else {
                    self.notice = Some("Wait for the current run to finish.".to_owned());
                }
            }
            Slash::Stop => {
                if self.run_pending {
                    self.stop();
                }
            }
            Slash::Help => {
                self.settings_tab = 3;
                self.show_settings = true;
            }
            Slash::Invalid(message) => self.notice = Some(message),
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_commands_and_leaves_paths_alone() {
        assert_eq!(parse("hello"), None);
        assert_eq!(parse("/src/main.rs is broken"), None);
        assert_eq!(parse("/goal"), Some(Slash::ShowGoal));
        assert_eq!(parse(" /goal clear "), Some(Slash::Goal(None)));
        assert_eq!(
            parse("/goal Ship a CLI\nwith tests"),
            Some(Slash::Goal(Some("Ship a CLI\nwith tests".into())))
        );
        assert_eq!(
            parse("/tasks clear done"),
            Some(Slash::ClearTasks {
                completed_only: true
            })
        );
        assert!(matches!(parse("/tasks"), Some(Slash::Invalid(_))));
        assert!(matches!(parse("/remember"), Some(Slash::Invalid(_))));
        assert_eq!(
            parse("/Remember use uv"),
            Some(Slash::Remember("use uv".into()))
        );
        assert!(matches!(parse("/frobnicate"), Some(Slash::Invalid(m)) if m.contains("/help")));
    }
}

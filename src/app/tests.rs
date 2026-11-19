use super::workspaces::FolderPick;
use super::*;

fn fixture() -> (MoxiBoxApp, Sender<AppEvent>) {
    fixture_with_commands().0
}

fn fixture_with_commands() -> ((MoxiBoxApp, Sender<AppEvent>), Receiver<AppCommand>) {
    let (commands, command_receiver) = mpsc::channel();
    let (events_tx, events) = mpsc::channel();
    let app = (
        MoxiBoxApp {
            commands,
            cancelled: Arc::new(AtomicBool::new(false)),
            show_settings: false,
            events,
            settings: Settings::default(),
            models: vec![],
            tasks: vec![],
            messages: vec![],
            composer: String::new(),
            new_task: String::new(),
            agent_mode: false,
            streaming_id: None,
            notice: None,
            connection: Connection::default(),
            provider_draft: None,
            provider_form_error: None,
            workspace_dialog: None,
            workspace_path: String::new(),
            workspace_error: None,
            workspace_pending: false,
            workspace_name: String::new(),
            folder_picker: None,
            history: PromptHistory::default(),
            approval: None,
            auto_approve: Arc::new(AtomicBool::new(false)),
            approval_forced: false,
            send_blocked: None,
            send_after_connect: false,
            question: None,
            mode_hint: false,
            chat_confirmed: false,
            activity: vec![],
            workspaces: vec![],
            workspace_id: None,
            switching: false,
            summary: String::new(),
            compacted_through: 0,
            show_full_history: false,
            memory: String::new(),
            memory_draft: None,
            goal: String::new(),
            goal_draft: None,
            skills: Vec::new(),
            show_skills: false,
            skill_to_delete: None,
            playbook: String::new(),
            playbook_draft: String::new(),
            memory_tab: 0,
            file_dialog: None,
            memory_note: String::new(),
            settings_tab: 0,
            workspace_entries: vec![],
            expanded_folders: Default::default(),
            preview: None,
            preview_requested: None,
            run_pending: false,
            show_workspace: true,
            worker_lost: false,
            queued: Default::default(),
            tree_requested: std::time::Instant::now(),
            tree_pending: false,
            activity_at: std::time::Instant::now(),
        },
        events_tx,
    );
    (app, command_receiver)
}

#[test]
fn unrelated_error_does_not_unlock_an_active_run() {
    let (mut app, tx) = fixture();
    app.run_pending = true;
    let context = egui::Context::default();
    tx.send(AppEvent::Error("preview failed".into())).unwrap();
    app.handle_events(&context);
    assert!(app.run_pending);
    tx.send(AppEvent::RunFailed("provider failed".into()))
        .unwrap();
    app.handle_events(&context);
    assert!(!app.run_pending);
}

#[test]
fn discovery_is_not_connection_and_stale_results_are_ignored() {
    let (mut app, _) = fixture();
    app.refresh_models(false);
    let discovery = app.connection.request_id;
    assert!(matches!(app.provider_status(), ProviderStatus::Checking));
    app.models_loaded(discovery, Ok(vec!["local-model".into()]));
    assert!(matches!(app.provider_status(), ProviderStatus::Available));
    assert!(!app.connection.connected);
    app.refresh_models(true);
    let connect = app.connection.request_id;
    app.models_loaded(connect, Ok(vec!["local-model".into()]));
    assert!(matches!(app.provider_status(), ProviderStatus::Ready));
    app.run_pending = true;
    assert!(
        matches!(app.provider_status(), ProviderStatus::Ready),
        "run activity must not replace connection status"
    );
    app.connection.disconnect();
    app.models_loaded(connect, Ok(vec!["late-model".into()]));
    assert!(matches!(app.provider_status(), ProviderStatus::Offline));
}

#[test]
fn empty_server_and_connection_failure_have_distinct_states() {
    let (mut app, _) = fixture();
    app.refresh_models(true);
    app.models_loaded(app.connection.request_id, Ok(vec![]));
    assert!(matches!(
        app.provider_status(),
        ProviderStatus::OnlineNoModel
    ));
    app.refresh_models(false);
    app.models_loaded(app.connection.request_id, Err("server stopped".into()));
    assert!(!app.connection.checking);
    assert!(!app.connection.connected);
    assert_eq!(app.connection.error.as_deref(), Some("server stopped"));
}

#[test]
fn provider_save_validates_and_preserves_exact_model_id() {
    let (mut app, _) = fixture();
    let mut draft = app.settings.clone();
    draft.lm_studio_url = "file:///tmp/test".into();
    assert!(app.save_provider(draft.clone()).is_err());
    assert_eq!(
        app.settings.lm_studio_url,
        Settings::default().lm_studio_url
    );
    draft.lm_studio_url = "http://localhost:11434/v1/".into();
    draft.provider_name = "Ollama".into();
    draft.selected_model = Some("manual-model".into());
    app.save_provider(draft).unwrap();
    app.models_loaded(app.connection.request_id, Ok(vec!["another-model".into()]));
    assert_eq!(app.settings.selected_model.as_deref(), Some("manual-model"));
    assert!(!app.connection.connected);
}

#[test]
fn switching_workspaces_follows_the_loaded_folder() {
    let ((mut app, tx), commands) = fixture_with_commands();
    app.settings.workspace_root = "/tmp/first".into();
    app.workspace_id = Some(1);
    app.select_workspace(2);
    assert!(matches!(
        commands.try_recv(),
        Ok(AppCommand::SelectWorkspace(2))
    ));
    tx.send(AppEvent::ConversationLoaded(
        crate::domain::ConversationState {
            current: 2,
            folder: String::new(),
            workspaces: vec![],
            messages: vec![],
            summary: String::new(),
            compacted_through: 0,
            memory: String::new(),
            goal: String::new(),
            playbook: String::new(),
            activity: vec![],
        },
    ))
    .unwrap();
    app.handle_events(&egui::Context::default());
    assert!(!app.switching);
    assert_eq!(app.workspace_id, Some(2));
    assert!(app.settings.workspace_root.is_empty());
    assert!(app.workspace_entries.is_empty());
}

#[test]
fn composer_actions_fit_small_and_default_windows() {
    for size in [egui::vec2(640.0, 480.0), egui::vec2(1280.0, 800.0)] {
        let (mut app, _tx) = fixture();
        app.connection.connected = true;
        app.composer = "A long multiline message\n".repeat(20);
        app.connection.error = Some("Long provider error ".repeat(20));
        app.messages.push(ChatMessage {
            id: 1,
            role: MessageRole::User,
            content: "very-long-workspace-name/".repeat(40),
        });
        app.tasks.push(Task {
            id: 1,
            title: "Long task title ".repeat(12),
            status: crate::domain::TaskStatus::Pending,
        });
        let context = egui::Context::default();
        theme::install_theme(&context);
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
        let mut found = false;
        for _ in 0..3 {
            let output = context.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    ..Default::default()
                },
                |context| app.render(context),
            );
            for clipped in output.shapes {
                if let egui::Shape::Text(text) = clipped.shape {
                    if text.galley.text() == "Send" {
                        found = true;
                        let bounds = egui::Rect::from_min_size(text.pos, text.galley.size());
                        assert!(
                            screen.contains_rect(bounds),
                            "Send outside {size:?}: {bounds:?}"
                        );
                        assert!(
                            clipped.clip_rect.contains_rect(bounds),
                            "Send clipped: {bounds:?}"
                        );
                    }
                }
            }
        }
        assert!(found, "Send action was not rendered at {size:?}");
    }
}

#[test]
fn ctrl_enter_sends_before_the_editor_consumes_enter() {
    let (mut app, _) = fixture();
    let (tx, rx) = mpsc::channel();
    app.commands = tx;
    app.connection.connected = true;
    app.settings.selected_model = Some("model".into());
    app.workspace_id = Some(1);
    app.composer = "a real keyboard shortcut".into();
    let context = egui::Context::default();
    theme::install_theme(&context);
    for _ in 0..3 {
        let _ = context.run(egui::RawInput::default(), |context| app.render(context));
    }
    context.memory_mut(|memory| memory.request_focus(egui::Id::new("composer_text")));
    let _ = context.run(
        egui::RawInput {
            modifiers: egui::Modifiers::CTRL,
            events: vec![egui::Event::Key {
                key: egui::Key::Enter,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::CTRL,
            }],
            ..Default::default()
        },
        |context| app.render(context),
    );
    assert!(app.composer.is_empty());
    assert!(
        matches!(rx.try_recv(),Ok(AppCommand::SendChat {content,..}) if content == "a real keyboard shortcut")
    );
}

#[test]
fn modal_actions_remain_inside_compact_windows() {
    for kind in 0..5 {
        let (mut app, _) = fixture();
        let expected = match kind {
            0 => {
                app.provider_draft = Some(app.settings.clone());
                "Save"
            }
            1 => {
                app.workspace_dialog = Some(WorkspaceDialog::NewFolder);
                app.workspace_path = "/tmp".into();
                "Create and attach"
            }
            2 => {
                app.workspace_dialog = Some(WorkspaceDialog::Reset);
                "Archive and reset"
            }
            3 => {
                app.memory_draft = Some(String::new());
                "Save"
            }
            _ => {
                app.show_settings = true;
                "Done"
            }
        };
        let context = egui::Context::default();
        theme::install_theme(&context);
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(640.0, 480.0));
        let mut found = false;
        for _ in 0..4 {
            let output = context.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    ..Default::default()
                },
                |context| app.render(context),
            );
            for clipped in output.shapes {
                if let egui::Shape::Text(text) = clipped.shape {
                    if text.galley.text() == expected {
                        found = true;
                        let bounds = egui::Rect::from_min_size(text.pos, text.galley.size());
                        assert!(
                            screen.contains_rect(bounds),
                            "{expected} outside viewport: {bounds:?}"
                        );
                        assert!(
                            clipped.clip_rect.contains_rect(bounds),
                            "{expected} clipped: {bounds:?}"
                        );
                    }
                }
            }
        }
        assert!(found, "{expected} action was not rendered");
    }
}

#[test]
fn native_picker_cancel_and_open() {
    let (mut app, _) = fixture();
    let (sender, commands) = mpsc::channel();
    app.commands = sender;
    app.settings.workspace_root = "/original".into();
    let (sender, receiver) = mpsc::channel();
    app.folder_picker = Some((FolderPick::Attach, receiver));
    sender.send(None).unwrap();
    app.poll_folder_picker();
    assert_eq!(app.settings.workspace_root, "/original");
    assert!(app.folder_picker.is_none());
    assert!(commands.try_recv().is_err());
    let (sender, receiver) = mpsc::channel();
    app.folder_picker = Some((FolderPick::Attach, receiver));
    sender.send(Some("/selected".into())).unwrap();
    app.poll_folder_picker();
    assert!(
        matches!(commands.try_recv(), Ok(AppCommand::AttachFolder {path, create: false}) if path == "/selected")
    );
}
#[test]
fn native_new_parent_waits_for_name() {
    let (mut app, _) = fixture();
    let (sender, commands) = mpsc::channel();
    app.commands = sender;
    let (sender, receiver) = mpsc::channel();
    app.folder_picker = Some((FolderPick::NewFolderParent, receiver));
    sender.send(Some("/parent".into())).unwrap();
    app.poll_folder_picker();
    assert!(app.workspace_dialog == Some(WorkspaceDialog::NewFolder));
    assert_eq!(app.workspace_path, "/parent");
    assert!(commands.try_recv().is_err());
}

#[test]
fn a_lost_worker_unlocks_the_ui_with_a_notice() {
    let (mut app, tx) = fixture();
    app.run_pending = true;
    drop(tx);
    app.handle_events(&egui::Context::default());
    assert!(!app.run_pending);
    assert!(app.worker_lost);
    assert!(app
        .notice
        .as_deref()
        .unwrap()
        .contains("background worker stopped"));
}

#[test]
fn messages_sent_during_a_run_are_queued_and_share_its_stop_token() {
    let ((mut app, tx), commands) = fixture_with_commands();
    app.settings.selected_model = Some("model".into());
    app.workspace_id = Some(1);
    app.connection.connected = true;
    app.composer = "first".into();
    app.send_message();
    let first = app.cancelled.clone();
    app.composer = "steer this".into();
    app.send_message();
    assert!(Arc::ptr_eq(&first, &app.cancelled));
    assert_eq!(app.queued.front().map(String::as_str), Some("steer this"));
    assert_eq!(commands.try_iter().count(), 2);
    let context = egui::Context::default();
    tx.send(AppEvent::ChatUser(ChatMessage {
        id: 9,
        role: MessageRole::User,
        content: "steer this".into(),
    }))
    .unwrap();
    app.handle_events(&context);
    assert!(app.queued.is_empty());
}

#[test]
fn turning_on_auto_approve_approves_the_waiting_request() {
    let ((mut app, _tx), commands) = fixture_with_commands();
    app.approval = Some(("call-1".into(), "Edit a.rs?".into(), "- a\n+ b".into()));
    app.set_auto_approve(true);
    assert!(app.approval.is_none());
    assert!(matches!(
        commands.try_recv(),
        Ok(AppCommand::ResolveApproval { request_id, approved: true }) if request_id == "call-1"
    ));
    app.set_auto_approve(false);
    app.approval = Some(("call-2".into(), "Warning".into(), "$ cat /etc/x".into()));
    app.approval_forced = true;
    app.set_auto_approve(true);
    assert!(app.approval.is_some() && commands.try_recv().is_err());
}

#[test]
fn waiting_label_reports_silence_not_total_time() {
    let (mut app, tx) = fixture();
    assert_eq!(app.waiting_label("Thinking"), "Thinking");
    app.activity_at = std::time::Instant::now() - std::time::Duration::from_secs(42);
    assert_eq!(
        app.waiting_label("Thinking"),
        "Thinking · nothing received from the model for 42s"
    );
    // Any streamed output, visible or not, resets the silence timer.
    tx.send(AppEvent::RunProgress).unwrap();
    app.handle_events(&egui::Context::default());
    assert_eq!(app.waiting_label("Thinking"), "Thinking");
}

#[test]
fn lost_connection_warns_and_reconnects_automatically() {
    let ((mut app, _tx), commands) = fixture_with_commands();
    let context = egui::Context::default();
    app.settings.selected_model = Some("model".into());
    app.refresh_models(true);
    app.models_loaded(app.connection.request_id, Ok(vec!["model".into()]));
    assert!(matches!(app.provider_status(), ProviderStatus::Ready));
    while commands.try_recv().is_ok() {}

    // A quiet health check that fails marks the loss and schedules a retry.
    app.connection.checked_at = std::time::Instant::now() - std::time::Duration::from_secs(60);
    app.tick_connection(&context);
    assert!(app.connection.probing);
    assert!(matches!(
        commands.try_recv(),
        Ok(AppCommand::RefreshModels { .. })
    ));
    app.models_loaded(
        app.connection.request_id,
        Err("Cannot reach the provider: refused".into()),
    );
    assert!(!app.connection.connected);
    assert!(matches!(
        app.provider_status(),
        ProviderStatus::Reconnecting
    ));

    // A failed attempt backs off; a later success restores the connection.
    app.connection.reconnect.as_mut().unwrap().next_at = std::time::Instant::now();
    app.tick_connection(&context);
    assert!(app.connection.checking);
    app.models_loaded(
        app.connection.request_id,
        Err("Cannot reach the provider".into()),
    );
    let reconnect = app.connection.reconnect.as_ref().unwrap();
    assert_eq!(reconnect.attempt, 1);
    assert!(app.connection.retry_in().unwrap() >= 3);
    app.retry_connection();
    app.models_loaded(app.connection.request_id, Ok(vec!["model".into()]));
    assert!(app.connection.connected && app.connection.reconnect.is_none());
    assert_eq!(
        app.notice.as_deref(),
        Some("Reconnected to the model server.")
    );

    // Disconnect stops retrying, and a never-connected provider does not auto-retry.
    app.connection_lost("gone".into());
    app.connection.disconnect();
    assert!(app.connection.reconnect.is_none());
    app.connection_lost("gone".into());
    assert!(app.connection.reconnect.is_none());
}

#[test]
fn a_dropped_run_reports_and_starts_reconnecting() {
    let (mut app, tx) = fixture();
    app.connection.connected = true;
    app.run_pending = true;
    tx.send(AppEvent::ProviderUnreachable).unwrap();
    tx.send(AppEvent::RunFailed(
        "Cannot reach the provider: reset".into(),
    ))
    .unwrap();
    app.handle_events(&egui::Context::default());
    assert!(app.connection.reconnect.is_some());
    assert!(!app.run_pending);
    assert!(app.notice.is_none());
    assert_eq!(
        app.connection.error.as_deref(),
        Some("Cannot reach the provider: reset")
    );
}

#[test]
fn a_scan_for_an_old_folder_cannot_freeze_the_tree() {
    let ((mut app, tx), commands) = fixture_with_commands();
    let context = egui::Context::default();
    app.settings.workspace_root = "/old".into();
    app.rescan_tree_now();
    app.refresh_tree(&context);
    assert!(
        matches!(commands.try_recv(), Ok(AppCommand::BrowseWorkspace { root, .. }) if root == "/old")
    );
    // The folder changes while that scan is in flight; its late reply must still end the scan.
    app.settings.workspace_root = "/new".into();
    tx.send(AppEvent::WorkspaceLoaded {
        root: "/old".into(),
        entries: vec![],
    })
    .unwrap();
    app.handle_events(&context);
    assert!(!app.tree_pending);
    app.rescan_tree_now();
    app.expanded_folders.insert("src".into());
    app.refresh_tree(&context);
    assert!(matches!(commands.try_recv(),
        Ok(AppCommand::BrowseWorkspace { root, expanded }) if root == "/new" && expanded == ["src"]));
    // Even a scan that never answers is retried after a few seconds.
    app.tree_requested = std::time::Instant::now() - std::time::Duration::from_secs(6);
    app.refresh_tree(&context);
    assert!(matches!(
        commands.try_recv(),
        Ok(AppCommand::BrowseWorkspace { .. })
    ));
    tx.send(AppEvent::WorkspaceChanged).unwrap();
    app.handle_events(&context);
    assert!(app.tree_requested.elapsed() >= std::time::Duration::from_secs(1));
}

#[test]
fn coding_requests_in_chat_mode_suggest_agent_mode() {
    use crate::app::looks_like_coding_task as coding;
    assert!(coding("make a snake game in python"));
    assert!(coding("fix the bug in main.py"));
    assert!(coding("Please add tests for the parser module"));
    assert!(!coding("hi, how are you?"));
    assert!(!coding("what is a closure in rust?"));
    assert!(!coding("make me a cup of tea"));

    let ((mut app, _tx), commands) = fixture_with_commands();
    app.workspace_id = Some(1);
    app.connection.connected = true;
    app.settings.selected_model = Some("model".into());
    app.composer = "fix the bug in main.py".into();
    app.send_message();
    assert!(app.mode_hint && !app.run_pending);
    assert_eq!(app.composer, "fix the bug in main.py");
    assert!(commands.try_recv().is_err());
    app.mode_hint = false;
    app.agent_mode = true;
    app.send_message();
    assert!(matches!(
        commands.try_recv(),
        Ok(AppCommand::SendChat {
            agent_mode: true,
            ..
        })
    ));
}

#[test]
fn questions_are_answered_by_choice_or_typed_text() {
    let ((mut app, tx), commands) = fixture_with_commands();
    app.run_pending = true;
    tx.send(AppEvent::QuestionAsked {
        request_id: "call00001".into(),
        question: "Which?".into(),
        options: vec!["A".into(), "B".into()],
    })
    .unwrap();
    app.handle_events(&egui::Context::default());
    assert!(app.question.is_some());
    app.answer_question("B".into());
    assert!(app.question.is_none());
    assert!(matches!(commands.try_recv(),
        Ok(AppCommand::AnswerQuestion { request_id, answer }) if request_id == "call00001" && answer == "B"));
    // Typed text answers the waiting question instead of being queued as a new message.
    app.question = Some((
        "call00002".into(),
        "Which?".into(),
        vec!["A".into(), "B".into()],
    ));
    app.workspace_id = Some(1);
    app.connection.connected = true;
    app.settings.selected_model = Some("model".into());
    app.composer = "neither, use C".into();
    app.send_message();
    assert!(app.queued.is_empty() && app.composer.is_empty());
    assert!(matches!(commands.try_recv(),
        Ok(AppCommand::AnswerQuestion { answer, .. }) if answer == "neither, use C"));
}

#[test]
fn sending_without_a_connection_shows_a_clear_warning_and_keeps_the_message() {
    let ((mut app, _tx), commands) = fixture_with_commands();
    app.workspace_id = Some(1);
    app.composer = "hello".into();
    app.send_message();
    assert_eq!(app.send_blocked, Some(super::SendBlocked::NotConnected));
    assert_eq!(app.composer, "hello");
    assert!(commands.try_recv().is_err());
    app.connection.connected = true;
    app.send_message();
    assert_eq!(app.send_blocked, Some(super::SendBlocked::NoModel));
    app.settings.selected_model = Some("model".into());
    app.connection.reconnect = Some(super::connection::Reconnect {
        attempt: 1,
        next_at: std::time::Instant::now(),
    });
    app.send_message();
    assert_eq!(app.send_blocked, Some(super::SendBlocked::Reconnecting));
    app.connection.reconnect = None;
    app.send_message();
    assert!(app.send_blocked.is_none());
    assert!(matches!(
        commands.try_recv(),
        Ok(AppCommand::SendChat { .. })
    ));
}

#[test]
fn copy_buttons_copy_the_message_without_moxi_notices() {
    use crate::domain::{ChatMessage, MessageRole};
    let message = |role, content: &str| ChatMessage {
        id: 1,
        role,
        content: content.into(),
    };
    let reply = format!(
        "Here is the fix.\n\n{}\n\n[Run interrupted: Stopped by user.]",
        crate::provider::lm_studio::harness_note("Recovery mode 1/3: skipped.")
    );
    assert_eq!(
        crate::ui::widgets::copy_text(&message(MessageRole::Assistant, &reply)),
        "Here is the fix."
    );
    assert_eq!(
        crate::ui::widgets::copy_text(&message(MessageRole::User, "  fix main.py \n")),
        "fix main.py"
    );
}

#[test]
fn the_send_button_connects_when_disconnected_and_then_sends() {
    let ((mut app, _tx), commands) = fixture_with_commands();
    app.workspace_id = Some(1);
    app.settings.selected_model = Some("model".into());
    app.composer = "hello".into();
    assert_eq!(app.connect_label(), Some("Connect"));
    app.connect_from_composer();
    assert_eq!(app.connect_label(), Some("Connecting…"));
    assert!(matches!(
        commands.try_recv(),
        Ok(AppCommand::RefreshModels { .. })
    ));
    app.models_loaded(app.connection.request_id, Ok(vec!["model".into()]));
    assert_eq!(app.connect_label(), None);
    app.finish_connect_and_send();
    assert!(
        matches!(commands.try_recv(), Ok(AppCommand::SendChat { content, .. }) if content == "hello")
    );
    // A failed connection keeps the message and stops waiting.
    app.run_pending = false;
    app.connection.disconnect();
    app.composer = "again".into();
    app.connect_from_composer();
    app.models_loaded(app.connection.request_id, Err("refused".into()));
    app.finish_connect_and_send();
    assert!(!app.send_after_connect && app.composer == "again");
    assert_eq!(app.connect_label(), Some("Connect"));
}

#[test]
fn active_memory_is_summarized_for_the_composer() {
    let (mut app, _tx) = fixture();
    assert_eq!(app.context_summary(), "");
    app.memory = "- a\n- b\n\n- c".into();
    app.playbook = "- shared lesson".into();
    app.skills.push(crate::domain::Skill {
        name: "count_lines".into(),
        description: "Count lines".into(),
        usage: String::new(),
        code: "print(1)".into(),
    });
    assert_eq!(app.context_summary(), "Memory: 3 here · 1 shared · 1 skill");
    app.memory.clear();
    assert_eq!(app.context_summary(), "Memory: 1 shared · 1 skill");
}

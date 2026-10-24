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
            activity: vec![],
            conversations: vec![],
            conversation_id: None,
            conversation_pending: false,
            summary: String::new(),
            compacted_through: 0,
            show_full_history: false,
            memory: String::new(),
            memory_draft: None,
            workspace_entries: vec![],
            expanded_folders: Default::default(),
            preview: None,
            run_pending: false,
            show_workspace: true,
            worker_lost: false,
            queued: Default::default(),
            tree_requested: std::time::Instant::now(),
            tree_pending: false,
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
fn removing_a_workspace_only_updates_saved_state() {
    let (mut app, _) = fixture();
    app.settings.workspace_root = "/tmp/first".into();
    app.settings.workspaces = vec!["/tmp/first".into(), "/tmp/second".into()];
    app.remove_workspace();
    assert!(app.settings.workspace_root.is_empty());
    assert_eq!(app.settings.workspaces, vec!["/tmp/second"]);
    assert!(app.workspace_entries.is_empty());
}

#[test]
fn composer_actions_fit_small_and_default_windows() {
    for size in [egui::vec2(640.0, 480.0), egui::vec2(1280.0, 800.0)] {
        let (mut app, _tx) = fixture();
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
    app.conversation_id = Some(1);
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
                app.workspace_dialog = Some(WorkspaceDialog::New);
                app.workspace_path = "/tmp".into();
                "Create"
            }
            2 => {
                app.workspace_dialog = Some(WorkspaceDialog::Reset);
                "Archive and reset"
            }
            3 => {
                app.memory_draft = Some(String::new());
                "Save memory"
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
    app.folder_picker = Some((WorkspaceDialog::Open, receiver));
    sender.send(None).unwrap();
    app.poll_folder_picker();
    assert_eq!(app.settings.workspace_root, "/original");
    assert!(app.folder_picker.is_none());
    assert!(commands.try_recv().is_err());
    let (sender, receiver) = mpsc::channel();
    app.folder_picker = Some((WorkspaceDialog::Open, receiver));
    sender.send(Some("/selected".into())).unwrap();
    app.poll_folder_picker();
    assert!(
        matches!(commands.try_recv(), Ok(AppCommand::OpenWorkspace {path, create: false}) if path == "/selected")
    );
}
#[test]
fn native_new_parent_waits_for_name() {
    let (mut app, _) = fixture();
    let (sender, commands) = mpsc::channel();
    app.commands = sender;
    let (sender, receiver) = mpsc::channel();
    app.folder_picker = Some((WorkspaceDialog::New, receiver));
    sender.send(Some("/parent".into())).unwrap();
    app.poll_folder_picker();
    assert!(app.workspace_dialog == Some(WorkspaceDialog::New));
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
    app.conversation_id = Some(1);
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
}

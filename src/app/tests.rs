use super::*;

fn fixture() -> (MoxiBoxApp, Sender<AppEvent>) {
    let (commands, _) = mpsc::channel();
    let (events_tx, events) = mpsc::channel();
    (
        MoxiBoxApp {
            commands,
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
            provider_online: false,
            approval: None,
            activity: vec![],
            workspace_entries: vec![],
            preview: None,
            run_pending: false,
            show_workspace: true,
            connection_settings: false,
            provider_checking: false,
        },
        events_tx,
    )
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
fn provider_status_tracks_observed_results() {
    let (mut app, tx) = fixture();
    app.provider_checking = true;
    assert!(matches!(app.provider_status(), ProviderStatus::Checking));
    tx.send(AppEvent::ConnectionChanged(true)).unwrap();
    tx.send(AppEvent::ModelsLoaded(vec![])).unwrap();
    app.handle_events(&egui::Context::default());
    assert!(matches!(
        app.provider_status(),
        ProviderStatus::OnlineNoModel
    ));
    tx.send(AppEvent::ModelsLoaded(vec!["local-model".into()]))
        .unwrap();
    app.handle_events(&egui::Context::default());
    assert!(matches!(app.provider_status(), ProviderStatus::Ready));
    tx.send(AppEvent::ConnectionChanged(false)).unwrap();
    app.handle_events(&egui::Context::default());
    assert!(matches!(app.provider_status(), ProviderStatus::Offline));
}

#[test]
fn composer_actions_fit_small_and_default_windows() {
    for size in [egui::vec2(640.0, 480.0), egui::vec2(1280.0, 800.0)] {
        let (mut app, _tx) = fixture();
        app.composer = "A long multiline message\n".repeat(20);
        app.connection_settings = true;
        app.messages.push(ChatMessage {
            id: 1,
            role: MessageRole::User,
            content: "very-long-workspace-name/".repeat(40),
        });
        app.tasks.push(Task {
            id: 1,
            title: "Long task title ".repeat(12),
            complete: false,
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

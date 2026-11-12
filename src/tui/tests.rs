use super::*;
use crate::core::ScanStatus;
use ratatui::{Terminal, backend::TestBackend};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

struct TempDir(std::path::PathBuf);
impl TempDir {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("pscon-tui-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn app() -> App {
    let store = Store::open_in_memory().unwrap();
    let mut config = Config::default();
    config.decisions.enabled = false;
    let events = EventBus::new(2048);
    let engine = Arc::new(ScanEngine::new(
        config.clone(),
        store.clone(),
        events.clone(),
    ));
    App::new(engine, store, events, config, None)
}
fn key(app: &mut App, code: KeyCode) {
    app.on_key(KeyEvent::new(code, KeyModifiers::NONE));
}
fn render(app: &mut App, width: u16, height: u16) -> String {
    let mut term = Terminal::new(TestBackend::new(width, height)).unwrap();
    term.draw(|f| ui::draw(f, app)).unwrap();
    let mut result = String::new();
    for y in 0..height {
        for x in 0..width {
            result.push_str(term.backend().buffer()[(x, y)].symbol());
        }
        result.push('\n');
    }
    result
}
fn endpoint(app: &App, name: &str, priority: f32) -> Endpoint {
    serde_json::from_value(serde_json::json!({
        "id": Uuid::new_v4(), "scan_id": app.scan.as_ref().unwrap().id,
        "method": "GET", "url": format!("https://example.test/{name}"),
        "normalized": name, "path_template": name, "params": [], "source_asset": "https://example.test/app.js",
        "source_line": 42, "context": "fetch('/api')", "role": "api", "sensitivity": priority,
        "auth_required": true, "verify_safe": true, "priority": priority, "confidence": 0.9,
        "provider": "heuristic", "discovered_at": app.scan.as_ref().unwrap().started_at,
    })).unwrap()
}
fn seeded() -> App {
    let mut a = app();
    let scan = a.store.create_scan("https://example.test").unwrap();
    a.load_scan(scan);
    a.endpoints = vec![endpoint(&a, "users", 0.9), endpoint(&a, "health", 0.2)];
    a.set_screen(Screen::Endpoints);
    a
}

#[test]
fn editor_handles_unicode_cursors_deletion_paste_and_horizontal_scroll() {
    let mut input = Input::new("a界éz");
    input.key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
    input.key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
    assert_eq!(input.text, "a界z");
    input.insert("ø\n\u{1b}\t");
    assert_eq!(input.text, "a界øz");
    input.key(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE));
    input.key(KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE));
    assert_eq!(input.text, "界øz");
    input.key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE));
    let (visible, cursor) = input.visible(3);
    assert_eq!(visible, "øz");
    assert_eq!(cursor, 2);
    input.key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
    assert!(input.text.is_empty());
    assert_eq!(input.cursor, 0);
}
#[test]
fn validates_form_without_changing_committed_options() {
    let mut a = app();
    a.open_form(None);
    let Modal::Form(form) = &mut a.modal else {
        panic!()
    };
    form.fields = [
        Input::new("file:///etc/passwd"),
        Input::new("-1"),
        Input::new("0"),
        Input::new("NaN"),
    ];
    assert!(!form.validate());
    assert!(form.errors.iter().all(|e| !e.is_empty()));
    form.fields = [
        Input::new("https://example.test"),
        Input::new("0"),
        Input::new("1"),
        Input::new("1"),
    ];
    assert!(form.validate());
    key(&mut a, KeyCode::Esc);
    assert!(a.target.is_none());
    assert_eq!(a.scan_options.max_pages, a.config.recon.max_pages);
}
#[test]
fn form_text_and_paste_cannot_trigger_global_actions() {
    let mut a = app();
    key(&mut a, KeyCode::Char('s'));
    a.on_paste("https://example.test/s?q=one\n");
    key(&mut a, KeyCode::Char('q'));
    key(&mut a, KeyCode::Char('1'));
    assert!(!a.should_quit);
    assert!(!a.scan_running);
    assert_eq!(a.screen, Screen::Home);
    let Modal::Form(form) = &a.modal else {
        panic!()
    };
    assert_eq!(form.fields[0].text, "https://example.test/s?q=oneq1");
    key(&mut a, KeyCode::BackTab);
    let Modal::Form(form) = &a.modal else {
        panic!()
    };
    assert_eq!(form.focus, 7);
}
#[test]
fn cli_target_opens_prefilled_form_and_no_target_opens_welcome() {
    let a = app();
    assert_eq!(a.screen, Screen::Home);
    assert!(matches!(a.modal, Modal::None));
    let a = App::new(
        a.engine.clone(),
        a.store.clone(),
        a.events.clone(),
        a.config.clone(),
        Some(Url::parse("https://example.test").unwrap()),
    );
    let Modal::Form(form) = a.modal else { panic!() };
    assert_eq!(form.fields[0].text, "https://example.test/");
}
#[test]
fn search_is_live_cancelable_and_retained_per_view() {
    let mut a = seeded();
    let original = a.selected_record();
    key(&mut a, KeyCode::Char('/'));
    a.on_paste("health");
    assert_eq!(a.records().len(), 1);
    assert!(a.records()[0].cells[2].contains("health"));
    key(&mut a, KeyCode::Esc);
    assert_eq!(a.records().len(), 2);
    key(&mut a, KeyCode::Char('/'));
    a.on_paste("users");
    key(&mut a, KeyCode::Enter);
    assert_eq!(a.view().filter, "users");
    a.set_screen(Screen::Assets);
    assert!(a.view().filter.is_empty());
    a.set_screen(Screen::Endpoints);
    assert_eq!(a.records().len(), 1);
    assert_eq!(a.selected_record(), original);
    key(&mut a, KeyCode::Esc);
    assert_eq!(a.records().len(), 2);
}
#[test]
fn sorting_is_numeric_and_selection_survives_live_updates() {
    let mut a = seeded();
    let selected = a.selected_record().unwrap();
    let ep = endpoint(&a, "new", 0.99);
    a.on_event(AppEvent::Scan(ScanEvent::Endpoint {
        scan_id: ep.scan_id,
        endpoint: ep,
    }));
    assert_eq!(a.selected_record(), Some(selected));
    assert_eq!(a.view().position, 1);
    key(&mut a, KeyCode::Char('O'));
    assert_eq!(a.selected_record(), Some(selected));
    assert_eq!(a.records()[0].cells[0], "0.20");
    key(&mut a, KeyCode::Char('o'));
    key(&mut a, KeyCode::Down);
    key(&mut a, KeyCode::Enter);
    assert_eq!(a.view().sort, 1);
    a.view_mut().sort = 0;
    a.view_mut().descending = false;
    assert_eq!(a.records()[2].cells[0], "0.99");
}
#[test]
fn details_include_evidence_and_verification_and_mask_secrets() {
    let mut a = seeded();
    key(&mut a, KeyCode::Enter);
    let Modal::Details { body, .. } = &a.modal else {
        panic!()
    };
    assert!(body.contains("app.js"));
    assert!(body.contains("VERIFICATION RESULTS"));
    let scan = a.scan.as_ref().unwrap();
    a.secrets.push(serde_json::from_value(serde_json::json!({
        "id": Uuid::new_v4(), "scan_id": scan.id, "asset_url": "https://example.test/app.js", "line": 1, "column": 1,
        "rule_id": "test", "provider": "test", "description": "Test credential", "severity": "high", "masked_value": "abc***xyz",
        "entropy": 3.0, "is_real": null, "risk": 0.8, "validated": null, "context": "RAW_SECRET_SHOULD_NOT_RENDER", "discovered_at": scan.started_at,
    })).unwrap());
    a.set_screen(Screen::Secrets);
    a.open_details();
    let output = render(&mut a, 100, 30);
    assert!(output.contains("abc***xyz"));
    assert!(!output.contains("RAW_SECRET_SHOULD_NOT_RENDER"));
}
#[test]
fn palette_filters_dispatches_and_isolates_shortcuts() {
    let mut a = app();
    a.on_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL));
    a.on_paste("recent");
    key(&mut a, KeyCode::Enter);
    assert_eq!(a.screen, Screen::History);
    a.on_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL));
    key(&mut a, KeyCode::Char('q'));
    assert!(!a.should_quit);
    key(&mut a, KeyCode::Esc);
    assert!(matches!(a.modal, Modal::None));
}
#[test]
fn active_scan_disables_new_scan_history_load_and_export() {
    let mut a = seeded();
    a.refresh_history();
    a.scan_running = true;
    a.open_form(None);
    assert!(matches!(a.modal, Modal::None));
    a.set_screen(Screen::History);
    key(&mut a, KeyCode::Enter);
    assert_eq!(a.screen, Screen::History);
    a.start_export();
    assert!(a.export.is_none());
    assert!(a.status.contains("finish"));
}
#[test]
fn renders_all_screens_and_dialogs_at_supported_sizes() {
    let mut a = seeded();
    for (width, height) in [(140, 45), (90, 28), (60, 18)] {
        for screen in Screen::ALL {
            a.set_screen(screen);
            let text = render(&mut a, width, height);
            assert!(text.contains("PSCON"));
            assert!(
                text.contains(screen.title()) || screen == Screen::Home,
                "{screen:?} at {width}x{height}:\n{text}"
            );
        }
        a.open_form(None);
        assert!(render(&mut a, width, height).contains("NEW SCAN"));
        a.modal = Modal::Help { scroll: 0 };
        assert!(render(&mut a, width, height).contains("HELP"));
        a.modal = Modal::Palette {
            input: Input::new("no such command"),
            selected: 0,
        };
        assert!(render(&mut a, width, height).contains("No matching commands"));
        a.modal = Modal::Details {
            title: "Evidence".into(),
            body: "long line\n".repeat(100),
            scroll: u16::MAX,
        };
        assert!(render(&mut a, width, height).contains("Evidence"));
        a.modal = Modal::None;
    }
    assert!(render(&mut a, 40, 10).contains("Resize terminal"));
    render(&mut a, 1, 1);
}

#[tokio::test]
async fn local_scan_form_to_results_history_and_four_report_exports() {
    let temp = TempDir::new();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            tokio::spawn(async move {
                let mut buf = [0; 4096];
                let _ = socket.read(&mut buf).await;
                let body = "<html><a href='/api/users'>Users</a><script>fetch('/api/session')</script></html>";
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(response.as_bytes()).await;
            });
        }
    });
    let mut a = app();
    a.config.general.data_dir = Some(temp.0.clone());
    a.config.general.timeout_secs = 2;
    a.config.recon.rate_limit_per_host_ms = 0;
    a.engine = Arc::new(ScanEngine::new(
        a.config.clone(),
        a.store.clone(),
        a.events.clone(),
    ));
    let mut rx = a.events.subscribe();
    a.open_form(Some(format!("http://{addr}/")));
    let Modal::Form(form) = &mut a.modal else {
        panic!()
    };
    form.fields[1] = Input::new("0");
    form.fields[2] = Input::new("1");
    form.fields[3] = Input::new("1");
    form.toggles = [false; 3];
    form.focus = 7;
    key(&mut a, KeyCode::Enter);
    assert!(a.scan_running);
    assert_eq!(a.screen, Screen::Overview);
    let mut stages = 0;
    tokio::time::timeout(Duration::from_secs(15), async {
        while a.scan_running {
            let event = rx.recv().await.unwrap();
            if matches!(event, AppEvent::Scan(ScanEvent::Stage { .. })) {
                stages += 1;
            }
            a.on_event(event);
        }
    })
    .await
    .unwrap();
    assert!(stages > 0);
    assert!(!a.endpoints.is_empty());
    assert_eq!(a.scan.as_ref().unwrap().status, ScanStatus::Completed);
    let id = a.scan.as_ref().unwrap().id;
    a.open_details();
    assert!(matches!(a.modal, Modal::Details { .. }));
    a.set_screen(Screen::History);
    assert_eq!(a.history.len(), 1);
    a.open_details();
    assert_eq!(a.scan.as_ref().unwrap().id, id);
    assert_eq!(a.screen, Screen::Overview);
    a.start_export();
    assert!(a.export.is_some());
    a.start_export();
    assert!(a.status.contains("already running"));
    tokio::time::timeout(Duration::from_secs(5), async {
        while a.export.is_some() {
            a.poll_tasks().await;
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(a.export_message.contains("Exported reports"));
    for ext in ["json", "md", "html", "sarif"] {
        let path = a
            .config
            .scans_dir()
            .join(id.to_string())
            .join(format!("report.{ext}"));
        assert!(std::fs::metadata(path).unwrap().len() > 0);
    }
    server.abort();
}

#[tokio::test]
async fn snapshot_starts_workspace_and_queues_without_changing_active_target() {
    let mut a = app();
    a.scan_running = true;
    a.target = Some(Url::parse("http://127.0.0.1:9/active").unwrap());
    let snapshot: ScanSnapshot = serde_json::from_value(serde_json::json!({
        "target": "http://127.0.0.1:9/queued", "page_url": "http://127.0.0.1:9/queued", "html": "", "title": null,
        "resources": [], "network": [], "inline_scripts": [], "storage_keys": [], "extension_version": null,
        "captured_at": a.store.create_scan("http://127.0.0.1:9/").unwrap().started_at,
    })).unwrap();
    a.on_event(AppEvent::Bridge(BridgeEvent::Snapshot { snapshot }));
    assert!(a.pending_snapshot.is_some());
    assert!(a.target.as_ref().unwrap().path().contains("active"));
    a.modal = Modal::Help { scroll: 0 };
    a.on_event(AppEvent::Scan(ScanEvent::Failed {
        scan_id: Uuid::nil(),
        error: "test failure".into(),
    }));
    assert!(a.pending_snapshot.is_none());
    assert_eq!(a.screen, Screen::Overview);
    assert!(matches!(a.modal, Modal::None));
    assert_eq!(a.target.as_ref().unwrap().path(), "/queued");
    a.scan_task.take().unwrap().abort();
}

#[tokio::test]
async fn failed_export_is_visible_and_does_not_block_navigation() {
    let temp = TempDir::new();
    let mut a = seeded();
    let file = temp.0.join("not-a-directory");
    std::fs::write(&file, "test").unwrap();
    a.config.general.data_dir = Some(file);
    a.start_export();
    a.set_screen(Screen::Home);
    tokio::time::timeout(Duration::from_secs(5), async {
        while a.export.is_some() {
            a.poll_tasks().await;
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(a.export_message.contains("Export failed"));
    assert_eq!(a.screen, Screen::Home);
}

#[test]
fn minimum_layout_shows_home_actions_results_and_scrollable_help() {
    let mut a = seeded();
    a.set_screen(Screen::Home);
    let home = render(&mut a, 60, 18);
    for text in [
        "New scan",
        "Recent scans",
        "Resume latest",
        "Help and shortcuts",
    ] {
        assert!(home.contains(text), "{home}");
    }
    a.set_screen(Screen::Overview);
    assert!(render(&mut a, 60, 18).contains("users"));
    a.modal = Modal::Help { scroll: 0 };
    key(&mut a, KeyCode::PageDown);
    assert!(render(&mut a, 60, 18).contains("Ctrl+U clear"));
}

#[test]
fn compact_scan_form_keeps_target_visible_while_selecting_options() {
    let mut a = app();
    a.open_form(Some("https://example.test/".into()));
    for _ in 0..4 {
        key(&mut a, KeyCode::Tab);
    }
    let screen = render(&mut a, 60, 18);
    assert!(screen.contains("Target URL"), "{screen}");
    assert!(screen.contains("https://example.test/"), "{screen}");
    assert!(screen.contains("Safe endpoint verification"), "{screen}");
}
#[test]
fn asset_size_sort_uses_bytes_and_history_target_can_seed_form() {
    let mut a = seeded();
    for size in [10_240, 2_048] {
        a.assets.push(serde_json::from_value(serde_json::json!({
            "id": Uuid::new_v4(), "scan_id": a.scan.as_ref().unwrap().id, "url": format!("https://example.test/{size}.js"),
            "kind": "java_script", "content_type": "application/javascript", "status": 200, "size": size,
            "sha256": null, "source": null, "fetched_at": a.scan.as_ref().unwrap().started_at,
        })).unwrap());
    }
    a.set_screen(Screen::Assets);
    a.view_mut().sort = 2;
    assert_eq!(a.records()[0].cells[2], "2.0 KiB");
    a.set_screen(Screen::History);
    key(&mut a, KeyCode::Char('n'));
    let Modal::Form(form) = &a.modal else {
        panic!()
    };
    assert_eq!(form.fields[0].text, "https://example.test");
}
#[tokio::test]
async fn ctrl_enter_submits_from_toggle_without_changing_its_value() {
    let mut a = app();
    a.open_form(Some("http://127.0.0.1:9/".into()));
    let Modal::Form(form) = &mut a.modal else {
        panic!()
    };
    form.focus = 4;
    form.toggles = [false; 3];
    a.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL));
    assert!(a.scan_running);
    assert!(!a.scan_options.verify);
    a.scan_task.take().unwrap().abort();
}
#[test]
fn scan_events_continue_while_a_dialog_is_open() {
    let mut a = seeded();
    a.modal = Modal::Help { scroll: 0 };
    a.on_event(AppEvent::Scan(ScanEvent::Stage {
        scan_id: a.scan.as_ref().unwrap().id,
        stage: "parse".into(),
        message: "Reading scripts".into(),
        progress: 0.5,
    }));
    assert_eq!(a.progress, 0.5);
    assert_eq!(a.stage, "parse");
    assert!(matches!(a.modal, Modal::Help { .. }));
}

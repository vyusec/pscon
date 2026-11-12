use super::*;
use std::path::PathBuf;
use tokio::task::JoinHandle;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Screen {
    Overview,
    Endpoints,
    Secrets,
    Assets,
    Decisions,
    Report,
    Home,
    History,
}
impl Screen {
    pub const ALL: [Self; 8] = [
        Self::Overview,
        Self::Endpoints,
        Self::Secrets,
        Self::Assets,
        Self::Decisions,
        Self::Report,
        Self::Home,
        Self::History,
    ];
    pub fn index(self) -> usize {
        Self::ALL.iter().position(|s| *s == self).unwrap()
    }
    pub fn title(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::Endpoints => "Endpoints",
            Self::Secrets => "Secrets",
            Self::Assets => "Assets",
            Self::Decisions => "Decisions",
            Self::Report => "Report",
            Self::Home => "Home",
            Self::History => "Recent scans",
        }
    }
    pub fn headers(self) -> &'static [&'static str] {
        match self {
            Self::Overview | Self::Endpoints => {
                &["Priority", "Method", "Endpoint", "Role", "Auth", "Provider"]
            }
            Self::Secrets => &[
                "Risk",
                "Severity",
                "Rule",
                "Provider",
                "Masked value",
                "Location",
            ],
            Self::Assets => &["Kind", "Status", "Size", "URL"],
            Self::Decisions => &[
                "Provider",
                "Subject",
                "Question",
                "Confidence",
                "Latency",
                "Cost",
            ],
            Self::History => &[
                "Started",
                "Status",
                "Target",
                "Endpoints",
                "Secrets",
                "Assets",
            ],
            _ => &[],
        }
    }
}
#[derive(Default)]
pub(super) struct ViewState {
    pub filter: String,
    pub selected: Option<Uuid>,
    pub position: usize,
    pub sort: usize,
    pub descending: bool,
    pub offset: usize,
}
#[derive(Default)]
pub(super) enum Modal {
    #[default]
    None,
    Form(Form),
    Palette {
        input: Input,
        selected: usize,
    },
    Details {
        title: String,
        body: String,
        scroll: u16,
    },
    Sort {
        selected: usize,
    },
    Help {
        scroll: u16,
    },
}
pub(super) struct Form {
    pub fields: [Input; 4],
    pub toggles: [bool; 3],
    pub focus: usize,
    pub errors: [String; 4],
}
impl Form {
    pub fn new(target: String, options: &ScanOptions) -> Self {
        Self {
            fields: [
                Input::new(target),
                Input::new(options.crawl_depth.to_string()),
                Input::new(options.max_pages.to_string()),
                Input::new(options.max_assets.to_string()),
            ],
            toggles: [options.verify, options.decisions, options.notify],
            focus: 0,
            errors: Default::default(),
        }
    }
    pub fn validate(&mut self) -> bool {
        self.errors = Default::default();
        match Url::parse(self.fields[0].text.trim()) {
            Ok(url) if matches!(url.scheme(), "http" | "https") && url.host_str().is_some() => {}
            _ => self.errors[0] = "Enter a full http:// or https:// URL".into(),
        }
        for i in 1..4 {
            if self.fields[i]
                .text
                .parse::<usize>()
                .ok()
                .is_none_or(|n| i > 1 && n == 0)
            {
                self.errors[i] = if i == 1 {
                    "Use a non-negative whole number"
                } else {
                    "Use a positive whole number"
                }
                .into();
            }
        }
        !self.errors.iter().any(|s| !s.is_empty())
    }
}
#[derive(Clone)]
pub(super) struct Record {
    pub id: Uuid,
    pub cells: Vec<String>,
    pub numbers: Vec<Option<f64>>,
    pub risk: f32,
}
impl Record {
    fn new(id: Uuid, cells: Vec<String>, numbers: &[(usize, f64)], risk: f32) -> Self {
        let mut n = vec![None; cells.len()];
        for &(i, v) in numbers {
            n[i] = Some(v);
        }
        Self {
            id,
            cells,
            numbers: n,
            risk,
        }
    }
}
#[derive(Clone, Copy)]
pub(super) enum Action {
    NewScan,
    History,
    Resume,
    Home,
    View(Screen),
    Reload,
    Export,
    Help,
    Quit,
}
impl Action {
    pub fn label(self) -> String {
        match self {
            Self::NewScan => "New scan",
            Self::History => "Recent scans",
            Self::Resume => "Resume latest scan",
            Self::Home => "Home",
            Self::Reload => "Reload current scan",
            Self::Export => "Export reports",
            Self::Help => "Help and shortcuts",
            Self::Quit => "Quit",
            Self::View(s) => return format!("Go to {}", s.title()),
        }
        .into()
    }
    pub fn all() -> Vec<Self> {
        let mut a = vec![Self::NewScan, Self::History, Self::Resume, Self::Home];
        a.extend(Screen::ALL[..6].iter().copied().map(Self::View));
        a.extend([Self::Reload, Self::Export, Self::Help, Self::Quit]);
        a
    }
}

pub(super) struct App {
    pub screen: Screen,
    pub target: Option<Url>,
    pub scan: Option<ScanRecord>,
    pub assets: Vec<Asset>,
    pub endpoints: Vec<Endpoint>,
    pub secrets: Vec<SecretFinding>,
    pub verifications: Vec<Verification>,
    pub decisions: Vec<DecisionRecord>,
    pub logs: Vec<String>,
    pub bridge_connected: bool,
    pub bridge_connected_flag: Option<Arc<AtomicBool>>,
    pub bridge_status: String,
    pub stage: String,
    pub progress: f32,
    pub status: String,
    pub should_quit: bool,
    pub scan_running: bool,
    pub pending_snapshot: Option<ScanSnapshot>,
    pub engine: Arc<ScanEngine>,
    pub store: Store,
    pub events: EventBus,
    pub config: Config,
    pub scan_options: ScanOptions,
    pub modal: Modal,
    pub views: [ViewState; 8],
    pub search: Option<Input>,
    pub history: Vec<ScanRecord>,
    pub home_selected: usize,
    pub log_scroll: usize,
    pub scan_task: Option<JoinHandle<Result<ScanRecord>>>,
    pub export: Option<JoinHandle<Result<PathBuf>>>,
    pub export_message: String,
}
impl App {
    pub fn new(
        engine: Arc<ScanEngine>,
        store: Store,
        events: EventBus,
        config: Config,
        target: Option<Url>,
    ) -> Self {
        let scan_options = ScanOptions::from_config(&config);
        let modal = target.as_ref().map_or(Modal::None, |t| {
            Modal::Form(Form::new(t.to_string(), &scan_options))
        });
        let mut app = Self {
            screen: Screen::Home,
            target,
            scan: None,
            assets: vec![],
            endpoints: vec![],
            secrets: vec![],
            verifications: vec![],
            decisions: vec![],
            logs: vec!["Ready. Start a scan or send a snapshot from JSner.".into()],
            bridge_connected: false,
            bridge_connected_flag: None,
            bridge_status: "bridge unavailable".into(),
            stage: "idle".into(),
            progress: 0.,
            status: "Ready".into(),
            should_quit: false,
            scan_running: false,
            pending_snapshot: None,
            engine,
            store,
            events,
            config,
            scan_options,
            modal,
            views: std::array::from_fn(|_| ViewState::default()),
            search: None,
            history: vec![],
            home_selected: 0,
            log_scroll: 0,
            scan_task: None,
            export: None,
            export_message: String::new(),
        };
        app.views[Screen::Overview.index()].descending = true;
        app.views[Screen::Endpoints.index()].descending = true;
        app.views[Screen::Secrets.index()].descending = true;
        app.views[Screen::History.index()].descending = true;
        app.refresh_history();
        if let Some(scan) = app.history.first().cloned() {
            app.load_scan(scan);
        }
        app
    }
    pub fn view(&self) -> &ViewState {
        &self.views[self.screen.index()]
    }
    pub fn view_mut(&mut self) -> &mut ViewState {
        &mut self.views[self.screen.index()]
    }
    pub fn load_scan(&mut self, scan: ScanRecord) {
        let loaded = (|| -> Result<_> {
            Ok((
                self.store.assets_for_scan(scan.id)?,
                self.store.endpoints_for_scan(scan.id)?,
                self.store.secrets_for_scan(scan.id)?,
                self.store.verifications_for_scan(scan.id)?,
                self.store.decisions_for_scan(scan.id)?,
            ))
        })();
        match loaded {
            Ok((assets, endpoints, secrets, verifications, decisions)) => {
                self.assets = assets;
                self.endpoints = endpoints;
                self.secrets = secrets;
                self.verifications = verifications;
                self.decisions = decisions;
                if !self.scan_running {
                    self.stage = scan.status.as_str().into();
                    self.progress = if scan.status == crate::core::ScanStatus::Completed {
                        1.0
                    } else {
                        0.0
                    };
                    if self.scan.as_ref().map(|s| s.id) != Some(scan.id) {
                        self.logs = vec![format!(
                            "Opened scan {} ({})",
                            scan.id,
                            scan.status.as_str()
                        )];
                        self.log_scroll = 0;
                    }
                }
                self.scan = Some(scan);
                self.reconcile_selection();
            }
            Err(err) => self.status = format!("Could not load scan: {err}"),
        }
    }
    pub fn refresh_history(&mut self) {
        match self.store.list_scans(50) {
            Ok(scans) => self.history = scans,
            Err(e) => self.status = format!("Could not load history: {e}"),
        }
    }
    pub fn set_screen(&mut self, screen: Screen) {
        if screen == Screen::History {
            self.refresh_history();
        }
        self.search = None;
        self.screen = screen;
        self.modal = Modal::None;
        self.reconcile_selection();
    }
    pub fn open_form(&mut self, target: Option<String>) {
        if self.scan_running {
            self.status =
                "A scan is running. Wait for it to finish before starting another.".into();
            return;
        }
        let target = target
            .or_else(|| self.target.as_ref().map(ToString::to_string))
            .unwrap_or_default();
        self.search = None;
        self.modal = Modal::Form(Form::new(target, &self.scan_options));
    }
    pub fn dispatch(&mut self, action: Action) {
        self.modal = Modal::None;
        match action {
            Action::NewScan => self.open_form(None),
            Action::History => self.set_screen(Screen::History),
            Action::Home => self.set_screen(Screen::Home),
            Action::View(s) => self.set_screen(s),
            Action::Resume => {
                if self.scan_running {
                    self.set_screen(Screen::Overview);
                } else {
                    self.refresh_history();
                    if let Some(scan) = self.history.first().cloned() {
                        self.target = Url::parse(&scan.target).ok();
                        self.load_scan(scan);
                        self.set_screen(Screen::Overview);
                    } else {
                        self.status = "No saved scans yet. Press s to start one.".into();
                    }
                }
            }
            Action::Reload => {
                if let Some(id) = self.scan.as_ref().map(|s| s.id) {
                    match self.store.load_scan(id) {
                        Ok(Some(scan)) => {
                            self.load_scan(scan);
                            self.status = "Reloaded current scan".into();
                        }
                        Ok(None) => self.status = "Scan no longer exists".into(),
                        Err(e) => self.status = format!("Reload failed: {e}"),
                    }
                } else {
                    self.status = "No scan to reload".into();
                }
            }
            Action::Export => self.start_export(),
            Action::Help => self.modal = Modal::Help { scroll: 0 },
            Action::Quit => self.should_quit = true,
        }
    }
    pub fn start_export(&mut self) {
        if self.export.is_some() {
            self.status = "Report export is already running".into();
            return;
        }
        if self.scan_running {
            self.status = "Wait for the scan to finish before exporting".into();
            return;
        }
        let Some(id) = self.scan.as_ref().map(|s| s.id) else {
            self.status = "No scan to export".into();
            return;
        };
        let store = self.store.clone();
        let config = self.config.clone();
        self.export_message = "Writing JSON, Markdown, HTML, and SARIF reports…".into();
        self.export = Some(tokio::task::spawn_blocking(move || {
            crate::engine::report::write_reports(&store, id, &config)
        }));
        self.set_screen(Screen::Report);
    }
    pub async fn poll_tasks(&mut self) {
        if self.scan_task.as_ref().is_some_and(|t| t.is_finished()) {
            let result = self.scan_task.take().unwrap().await;
            let error = match result {
                Ok(Err(e)) => Some(e.to_string()),
                Err(e) => Some(e.to_string()),
                _ => None,
            };
            if self.scan_running
                && let Some(error) = error
            {
                self.on_event(AppEvent::Scan(ScanEvent::Failed {
                    scan_id: Uuid::nil(),
                    error,
                }));
            }
        }
        if self.export.as_ref().is_some_and(|t| t.is_finished()) {
            self.export_message = match self.export.take().unwrap().await {
                Ok(Ok(path)) => format!(
                    "Exported reports to {}",
                    path.parent().unwrap_or(&path).display()
                ),
                Ok(Err(e)) => {
                    format!("Export failed: {e}. Check that the report directory is writable.")
                }
                Err(e) => format!("Export worker failed: {e}"),
            };
            self.status = self.export_message.clone();
        }
    }
    pub fn palette_actions(input: &Input) -> Vec<Action> {
        let filter = input.text.to_lowercase();
        Action::all()
            .into_iter()
            .filter(|a| a.label().to_lowercase().contains(&filter))
            .collect()
    }
    pub fn records(&self) -> Vec<Record> {
        let mut records: Vec<_> = match self.screen {
            Screen::Overview | Screen::Endpoints => self
                .endpoints
                .iter()
                .map(|e| {
                    Record::new(
                        e.id,
                        vec![
                            format!("{:.2}", e.priority),
                            e.method.clone(),
                            e.url.clone(),
                            e.role.clone(),
                            e.auth_required
                                .map_or("unknown", |v| if v { "required" } else { "public" })
                                .into(),
                            e.provider.clone(),
                        ],
                        &[(0, e.priority as f64)],
                        e.priority,
                    )
                })
                .collect(),
            Screen::Secrets => self
                .secrets
                .iter()
                .map(|s| {
                    Record::new(
                        s.id,
                        vec![
                            format!("{:.2}", s.risk),
                            s.severity.clone(),
                            s.rule_id.clone(),
                            s.provider.clone(),
                            s.masked_value.clone(),
                            format!("{}:{}", s.asset_url, s.line),
                        ],
                        &[(0, s.risk as f64)],
                        s.risk,
                    )
                })
                .collect(),
            Screen::Assets => self
                .assets
                .iter()
                .map(|a| {
                    Record::new(
                        a.id,
                        vec![
                            a.kind.as_str().into(),
                            a.status.map_or("—".into(), |s| s.to_string()),
                            human_size(a.size),
                            a.url.clone(),
                        ],
                        &[(1, a.status.unwrap_or(0) as f64), (2, a.size as f64)],
                        0.,
                    )
                })
                .collect(),
            Screen::Decisions => self
                .decisions
                .iter()
                .map(|d| {
                    Record::new(
                        d.id,
                        vec![
                            d.provider.clone(),
                            d.subject_kind.clone(),
                            d.question.clone(),
                            format!("{:.2}", d.confidence),
                            format!("{}ms", d.latency_ms),
                            format!("${:.4}", d.cost_usd),
                        ],
                        &[
                            (3, d.confidence as f64),
                            (4, d.latency_ms as f64),
                            (5, d.cost_usd),
                        ],
                        0.,
                    )
                })
                .collect(),
            Screen::History => self
                .history
                .iter()
                .map(|s| {
                    Record::new(
                        s.id,
                        vec![
                            s.started_at.format("%Y-%m-%d %H:%M:%S UTC").to_string(),
                            s.status.as_str().into(),
                            s.target.clone(),
                            s.summary.endpoints.to_string(),
                            s.summary.secrets.to_string(),
                            s.summary.assets.to_string(),
                        ],
                        &[
                            (3, s.summary.endpoints as f64),
                            (4, s.summary.secrets as f64),
                            (5, s.summary.assets as f64),
                        ],
                        0.,
                    )
                })
                .collect(),
            _ => vec![],
        };
        let state = self.view();
        let filter = self
            .search
            .as_ref()
            .map_or(state.filter.as_str(), |s| s.text.as_str())
            .to_lowercase();
        records.retain(|r| {
            filter.is_empty() || r.cells.iter().any(|c| c.to_lowercase().contains(&filter))
        });
        let col = state
            .sort
            .min(self.screen.headers().len().saturating_sub(1));
        records.sort_by(|a, b| {
            let order = match (a.numbers[col], b.numbers[col]) {
                (Some(a), Some(b)) => a.total_cmp(&b),
                _ => a.cells[col]
                    .to_lowercase()
                    .cmp(&b.cells[col].to_lowercase()),
            };
            let order = if state.descending {
                order.reverse()
            } else {
                order
            };
            order.then_with(|| a.id.cmp(&b.id))
        });
        records
    }
    pub fn reconcile_selection(&mut self) {
        let records = self.records();
        let view = self.view_mut();
        if let Some(i) = records.iter().position(|r| Some(r.id) == view.selected) {
            view.position = i;
        } else {
            view.position = view.position.min(records.len().saturating_sub(1));
            view.selected = records.get(view.position).map(|r| r.id);
        }
        view.offset = view.offset.min(view.position);
    }
    pub fn move_selection(&mut self, delta: isize) {
        let records = self.records();
        let state = self.view_mut();
        state.position = state
            .position
            .saturating_add_signed(delta)
            .min(records.len().saturating_sub(1));
        state.selected = records.get(state.position).map(|r| r.id);
    }
    pub fn selected_record(&self) -> Option<Uuid> {
        let rows = self.records();
        self.view()
            .selected
            .filter(|id| rows.iter().any(|r| r.id == *id))
            .or_else(|| rows.get(self.view().position).map(|r| r.id))
    }
    pub fn open_details(&mut self) {
        let Some(id) = self.selected_record() else {
            self.status = "No result selected".into();
            return;
        };
        if self.screen == Screen::History {
            if self.scan_running {
                self.status =
                    "Wait for the active scan to finish before opening saved results".into();
                return;
            }
            if let Some(scan) = self.history.iter().find(|s| s.id == id).cloned() {
                self.target = Url::parse(&scan.target).ok();
                self.load_scan(scan);
                self.set_screen(Screen::Overview);
                self.status = "Opened saved scan".into();
            }
            return;
        }
        let body = match self.screen {
            Screen::Overview | Screen::Endpoints => self.endpoints.iter().find(|e| e.id == id).map(|e| {
                let mut body = serde_json::to_string_pretty(e).unwrap_or_default();
                body.push_str("\n\nVERIFICATION RESULTS\n");
                let checks: Vec<_> = self.verifications.iter().filter(|v| v.endpoint_id == id).collect();
                body.push_str(&if checks.is_empty() { "No verification results available.".into() } else { serde_json::to_string_pretty(&checks).unwrap_or_default() });
                body
            }),
            Screen::Secrets => self.secrets.iter().find(|s| s.id == id).map(|s| {
                // Render only the stored masked value; omit source context which can contain credentials.
                format!("Rule: {}\nProvider: {}\nSeverity: {}\nRisk: {:.2}\nValue: {}\nLocation: {}:{}:{}\nDescription: {}\nValidated: {:?}\nIs real: {:?}\nEntropy: {:.2}\n\nSource context is omitted to preserve secret masking.",
                    s.rule_id, s.provider, s.severity, s.risk, s.masked_value, s.asset_url, s.line, s.column, s.description, s.validated, s.is_real, s.entropy)
            }),
            Screen::Assets => self.assets.iter().find(|a| a.id == id).map(|a| serde_json::to_string_pretty(a).unwrap_or_default()),
            Screen::Decisions => self.decisions.iter().find(|d| d.id == id).map(|d| serde_json::to_string_pretty(d).unwrap_or_default()),
            _ => None,
        };
        if let Some(body) = body {
            self.modal = Modal::Details {
                title: format!("{} · {}", self.screen.title(), id),
                body,
                scroll: 0,
            };
        }
    }
    pub fn on_paste(&mut self, text: &str) {
        match &mut self.modal {
            Modal::Form(form) if form.focus < 4 => {
                form.fields[form.focus].insert(text);
                form.validate();
            }
            Modal::Palette { input, selected } => {
                input.insert(text);
                *selected = 0;
            }
            Modal::None => {
                if let Some(input) = &mut self.search {
                    input.insert(text);
                    self.reconcile_selection();
                }
            }
            _ => {}
        }
    }
    pub fn on_key(&mut self, key: KeyEvent) {
        if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
            return;
        }
        // Ctrl+C is always available, even from a focused input.
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.should_quit = true;
            return;
        }
        match &mut self.modal {
            Modal::Help { scroll } => {
                match key.code {
                    KeyCode::Esc | KeyCode::Enter | KeyCode::Char('?') => self.modal = Modal::None,
                    KeyCode::Down | KeyCode::Char('j') => *scroll = scroll.saturating_add(1),
                    KeyCode::Up | KeyCode::Char('k') => *scroll = scroll.saturating_sub(1),
                    KeyCode::PageDown => *scroll = scroll.saturating_add(10),
                    KeyCode::PageUp => *scroll = scroll.saturating_sub(10),
                    KeyCode::Home | KeyCode::Char('g') => *scroll = 0,
                    _ => {}
                }
                return;
            }
            Modal::Details { scroll, body, .. } => {
                match key.code {
                    KeyCode::Esc | KeyCode::Enter => self.modal = Modal::None,
                    KeyCode::Down | KeyCode::Char('j') => {
                        *scroll = scroll
                            .saturating_add(1)
                            .min(body.len().min(u16::MAX as usize) as u16)
                    }
                    KeyCode::Up | KeyCode::Char('k') => *scroll = scroll.saturating_sub(1),
                    KeyCode::PageDown => *scroll = scroll.saturating_add(10),
                    KeyCode::PageUp => *scroll = scroll.saturating_sub(10),
                    KeyCode::Home | KeyCode::Char('g') => *scroll = 0,
                    _ => {}
                }
                return;
            }
            Modal::Form(form) => {
                match key.code {
                    KeyCode::Esc => self.modal = Modal::None,
                    KeyCode::Tab => form.focus = (form.focus + 1) % 8,
                    KeyCode::BackTab => form.focus = (form.focus + 7) % 8,
                    KeyCode::Char(' ') | KeyCode::Enter
                        if (4..7).contains(&form.focus)
                            && !key.modifiers.contains(KeyModifiers::CONTROL) =>
                    {
                        form.toggles[form.focus - 4] ^= true
                    }
                    KeyCode::Enter
                        if form.focus == 7 || key.modifiers.contains(KeyModifiers::CONTROL) =>
                    {
                        if form.validate() {
                            self.target = Url::parse(form.fields[0].text.trim()).ok();
                            self.scan_options.crawl_depth = form.fields[1].text.parse().unwrap();
                            self.scan_options.max_pages = form.fields[2].text.parse().unwrap();
                            self.scan_options.max_assets = form.fields[3].text.parse().unwrap();
                            [
                                self.scan_options.verify,
                                self.scan_options.decisions,
                                self.scan_options.notify,
                            ] = form.toggles;
                            self.start_scan();
                        } else {
                            form.focus = form.errors.iter().position(|s| !s.is_empty()).unwrap();
                        }
                    }
                    KeyCode::Enter => form.focus = (form.focus + 1) % 8,
                    _ if form.focus < 4 => {
                        form.fields[form.focus].key(key);
                        form.validate();
                    }
                    _ => {}
                }
                return;
            }
            Modal::Palette { input, selected } => {
                match key.code {
                    KeyCode::Esc => self.modal = Modal::None,
                    KeyCode::Down => {
                        *selected = (*selected + 1)
                            .min(Self::palette_actions(input).len().saturating_sub(1))
                    }
                    KeyCode::Up => *selected = selected.saturating_sub(1),
                    KeyCode::Enter => {
                        if let Some(action) = Self::palette_actions(input).get(*selected).copied() {
                            self.dispatch(action);
                        }
                    }
                    _ => {
                        input.key(key);
                        *selected = 0;
                    }
                }
                return;
            }
            Modal::Sort { selected } => {
                match key.code {
                    KeyCode::Esc => self.modal = Modal::None,
                    KeyCode::Down | KeyCode::Char('j') => {
                        *selected =
                            (*selected + 1).min(self.screen.headers().len().saturating_sub(1))
                    }
                    KeyCode::Up | KeyCode::Char('k') => *selected = selected.saturating_sub(1),
                    KeyCode::Enter => {
                        let col = *selected;
                        self.view_mut().sort = col;
                        self.modal = Modal::None;
                        self.reconcile_selection();
                    }
                    _ => {}
                }
                return;
            }
            Modal::None => {}
        }
        if let Some(input) = &mut self.search {
            match key.code {
                KeyCode::Esc => {
                    self.search = None;
                }
                KeyCode::Enter => {
                    self.view_mut().filter = self.search.take().unwrap().text;
                }
                _ => input.key(key),
            }
            self.reconcile_selection();
            return;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('p') {
            self.modal = Modal::Palette {
                input: Input::default(),
                selected: 0,
            };
            return;
        }
        match key.code {
            KeyCode::Char('q') => self.dispatch(Action::Quit),
            KeyCode::Char('?') => self.dispatch(Action::Help),
            KeyCode::Char('s') => self.open_form(None),
            KeyCode::Char('h') => self.set_screen(Screen::Home),
            KeyCode::Char('H') => self.set_screen(Screen::History),
            KeyCode::Char('e') => self.start_export(),
            KeyCode::Char('r') => {
                if self.screen == Screen::History {
                    self.refresh_history();
                    self.reconcile_selection();
                } else {
                    self.dispatch(Action::Reload);
                }
            }
            KeyCode::Char('1'..='6') => {
                if let KeyCode::Char(c) = key.code {
                    self.set_screen(Screen::ALL[c as usize - '1' as usize]);
                }
            }
            KeyCode::Tab | KeyCode::BackTab => {
                let step = if key.code == KeyCode::Tab { 1 } else { 5 };
                self.set_screen(Screen::ALL[(self.screen.index().min(5) + step) % 6]);
            }
            KeyCode::Char('/') if !self.screen.headers().is_empty() => {
                self.search = Some(Input::new(self.view().filter.clone()))
            }
            KeyCode::Esc => {
                self.view_mut().filter.clear();
                self.reconcile_selection();
            }
            KeyCode::Char('o') if !self.screen.headers().is_empty() => {
                self.modal = Modal::Sort {
                    selected: self.view().sort,
                }
            }
            KeyCode::Char('O') => {
                self.view_mut().descending ^= true;
                self.reconcile_selection();
            }
            KeyCode::Char('n') if self.screen == Screen::History => {
                if let Some(id) = self.selected_record() {
                    let target = self
                        .history
                        .iter()
                        .find(|s| s.id == id)
                        .map(|s| s.target.clone());
                    self.open_form(target);
                }
            }
            KeyCode::Enter if self.screen == Screen::Home => self.dispatch(
                [
                    Action::NewScan,
                    Action::History,
                    Action::Resume,
                    Action::Help,
                ][self.home_selected],
            ),
            KeyCode::Enter => self.open_details(),
            KeyCode::Down | KeyCode::Char('j') if self.screen == Screen::Home => {
                self.home_selected = (self.home_selected + 1) % 4
            }
            KeyCode::Up | KeyCode::Char('k') if self.screen == Screen::Home => {
                self.home_selected = (self.home_selected + 3) % 4
            }
            KeyCode::Down | KeyCode::Char('j') => self.move_selection(1),
            KeyCode::Up | KeyCode::Char('k') => self.move_selection(-1),
            KeyCode::PageDown => self.move_selection(10),
            KeyCode::PageUp => self.move_selection(-10),
            KeyCode::Home | KeyCode::Char('g') => self.move_selection(isize::MIN),
            KeyCode::End | KeyCode::Char('G') => self.move_selection(isize::MAX),
            KeyCode::Char(']') => self.log_scroll = self.log_scroll.saturating_sub(1),
            KeyCode::Char('[') => {
                self.log_scroll = (self.log_scroll + 1).min(self.logs.len().saturating_sub(1))
            }
            _ => {}
        }
    }
}

pub(super) fn human_size(size: usize) -> String {
    if size >= 1024 * 1024 {
        format!("{:.1} MiB", size as f64 / (1024. * 1024.))
    } else if size >= 1024 {
        format!("{:.1} KiB", size as f64 / 1024.)
    } else {
        format!("{size} B")
    }
}

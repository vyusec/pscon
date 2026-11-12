use super::*;

impl App {
    pub(super) fn start_scan(&mut self) {
        if self.scan_running {
            self.status = "scan already running".into();
            return;
        }
        let Some(target) = self.target.clone() else {
            self.status = "No target selected; press s to enter a URL".into();
            return;
        };
        self.reset_scan_state();
        self.screen = Screen::Overview;
        self.modal = Modal::None;
        self.scan_running = true;
        self.status = format!("scanning {target}");
        let engine = self.engine.clone();
        let options = self.scan_options.clone();
        self.scan_task = Some(tokio::spawn(
            async move { engine.run(target, options).await },
        ));
    }

    pub(super) fn start_snapshot(&mut self, snapshot: ScanSnapshot) {
        if self.scan_running {
            self.status = "scan already running; snapshot queued".into();
            self.pending_snapshot = Some(snapshot);
            return;
        }
        self.target = Url::parse(&snapshot.page_url).ok();
        self.reset_scan_state();
        self.screen = Screen::Overview;
        self.modal = Modal::None;
        self.scan_running = true;
        self.status = "processing extension snapshot".into();
        let engine = self.engine.clone();
        let options = self.scan_options.clone();
        self.scan_task = Some(tokio::spawn(async move {
            engine.run_snapshot(snapshot, options).await
        }));
    }

    pub(super) fn reset_scan_state(&mut self) {
        self.assets.clear();
        self.endpoints.clear();
        self.secrets.clear();
        self.verifications.clear();
        self.decisions.clear();
        self.scan = None;
        for view in &mut self.views {
            view.selected = None;
            view.position = 0;
            view.offset = 0;
        }
        self.search = None;
        self.log_scroll = 0;
        self.export_message.clear();
        self.progress = 0.0;
        self.stage = "starting".into();
        self.logs.clear();
    }

    pub(super) fn on_event(&mut self, event: AppEvent) {
        match event {
            AppEvent::Scan(event) => self.on_scan_event(event),
            AppEvent::Bridge(event) => self.on_bridge_event(event),
            AppEvent::BridgeStatus { connected, message } => {
                self.bridge_connected = connected;
                self.bridge_status = message;
            }
        }
        if self.logs.len() > 200 {
            self.logs.drain(..self.logs.len() - 200);
        }
        self.log_scroll = self.log_scroll.min(self.logs.len().saturating_sub(1));
        self.reconcile_selection();
    }

    pub(super) fn on_scan_event(&mut self, event: ScanEvent) {
        match event {
            ScanEvent::Started { scan_id, target } => {
                self.scan_running = true;
                self.target = Url::parse(&target).ok();
                self.status = format!("scanning {target}");
                if let Ok(Some(scan)) = self.store.load_scan(scan_id) {
                    self.scan = Some(scan);
                }
            }
            ScanEvent::Stage {
                stage,
                message,
                progress,
                ..
            } => {
                self.stage = stage;
                self.progress = progress;
                self.status = message;
            }
            ScanEvent::Asset { asset, .. } => {
                self.assets.push(asset);
            }
            ScanEvent::Endpoint { endpoint, .. } => {
                self.endpoints.push(endpoint);
                self.endpoints
                    .sort_by(|a, b| b.priority.total_cmp(&a.priority));
            }
            ScanEvent::Secret { secret, .. } => {
                self.secrets.push(secret);
                self.secrets.sort_by(|a, b| b.risk.total_cmp(&a.risk));
            }
            ScanEvent::Decision { decision, .. } => {
                self.decisions.push(decision);
            }
            ScanEvent::Verification { verification, .. } => {
                self.verifications.push(verification);
            }
            ScanEvent::Log { level, message, .. } => {
                self.logs.push(format!("[{level}] {message}"));
                if self.logs.len() > 200 {
                    self.logs.remove(0);
                }
            }
            ScanEvent::Completed { scan_id, summary } => {
                self.scan_running = false;
                self.progress = 1.0;
                self.stage = "complete".into();
                self.status = format!(
                    "complete: {} endpoints, {} secrets, {} decisions",
                    summary.endpoints, summary.secrets, summary.decisions
                );
                if let Ok(Some(scan)) = self.store.load_scan(scan_id) {
                    self.load_scan(scan);
                }
                if let Some(snapshot) = self.pending_snapshot.take() {
                    self.start_snapshot(snapshot);
                }
            }
            ScanEvent::Failed { scan_id, error } => {
                // A failed worker can finish after the next queued snapshot has started.
                if !scan_id.is_nil() && self.scan.as_ref().map(|s| s.id) != Some(scan_id) {
                    return;
                }
                self.scan_running = false;
                self.stage = "failed".into();
                self.status = format!("scan failed: {error}");
                self.logs.push(format!("[error] {error}"));
                if let Some(scan) = self.scan.clone()
                    && let Ok(Some(scan)) = self.store.load_scan(scan.id)
                {
                    self.load_scan(scan);
                }
                if let Some(snapshot) = self.pending_snapshot.take() {
                    self.start_snapshot(snapshot);
                }
            }
        }
    }

    pub(super) fn on_bridge_event(&mut self, event: BridgeEvent) {
        match event {
            BridgeEvent::Hello {
                extension_version,
                browser,
                tab_url,
            } => {
                self.bridge_connected = true;
                self.bridge_status = format!(
                    "connected: {browser} / JSner {extension_version}{}",
                    tab_url.map(|url| format!(" / {url}")).unwrap_or_default()
                );
                self.status = "extension connected".into();
            }
            BridgeEvent::Snapshot { snapshot } => {
                self.bridge_status = format!("snapshot from {}", snapshot.page_url);
                self.start_snapshot(snapshot);
            }
            BridgeEvent::Resource { url, status, .. } => {
                self.logs.push(format!("[bridge] resource {status} {url}"));
            }
            BridgeEvent::ScanProgress {
                stage,
                message,
                progress,
                ..
            } => {
                self.stage = stage;
                self.status = message;
                self.progress = progress;
            }
            BridgeEvent::Disconnected => {
                self.bridge_connected = false;
                self.bridge_status = "disconnected".into();
            }
            BridgeEvent::Log { level, message } => {
                self.logs.push(format!("[bridge:{level}] {message}"));
            }
        }
    }
}

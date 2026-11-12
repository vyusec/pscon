use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use crate::core::{
    Asset, AssetKind, Config, Endpoint, EventBus, Notifier, ScanEvent, ScanRecord, ScanSnapshot,
    ScanStatus, Scope, SecretFinding, Store, Verification,
};
use anyhow::{Context, Result};
use futures_util::future::join_all;
use reqwest::Method;
use reqwest::header::{COOKIE, HeaderMap, HeaderName, HeaderValue};
use serde_json::json;
use tokio::sync::Semaphore;
use tracing::info;
use url::Url;
use uuid::Uuid;

use crate::engine::classify::{classify_content_type, is_static};
use crate::engine::decision::{
    DecisionEngine, EndpointTriage, SecretTriage, heuristic_endpoint, heuristic_secret,
};
use crate::engine::http::{Fetched, HttpClient};
use crate::engine::normalize::{Normalizer, is_page_url};
use crate::engine::parse::{self, Extracted};
use crate::engine::report;
use crate::engine::secrets;

#[derive(Debug, Clone)]
pub struct ScanOptions {
    pub crawl_depth: usize,
    pub max_pages: usize,
    pub max_assets: usize,
    pub max_model_endpoints: usize,
    pub max_model_secrets: usize,
    pub max_verify: usize,
    pub verify: bool,
    pub unsafe_verify: bool,
    pub decisions: bool,
    pub notify: bool,
    pub scope_file: Option<PathBuf>,
    pub headers: Vec<(String, String)>,
    pub cookie: Option<String>,
}

impl ScanOptions {
    pub fn from_config(config: &Config) -> Self {
        Self {
            crawl_depth: config.recon.crawl_depth,
            max_pages: config.recon.max_pages,
            max_assets: config.recon.max_assets,
            max_model_endpoints: 150,
            max_model_secrets: 50,
            max_verify: 200,
            verify: config.recon.verify,
            unsafe_verify: false,
            decisions: config.decisions.enabled,
            notify: true,
            scope_file: config.scope.file.clone(),
            headers: Vec::new(),
            cookie: None,
        }
    }
}

pub struct ScanEngine {
    config: Config,
    store: Store,
    events: EventBus,
    notifier: Notifier,
}

impl ScanEngine {
    pub fn new(config: Config, store: Store, events: EventBus) -> Self {
        let notifier = Notifier::new(config.notify.clone());
        Self {
            config,
            store,
            events,
            notifier,
        }
    }

    pub async fn run(&self, target: Url, options: ScanOptions) -> Result<ScanRecord> {
        let mut session = self.session(target.clone(), options.clone()).await?;
        let scan_id = session.scan_id;
        let result = async {
            let fetched = session
                .http
                .fetch(&target, Method::GET, Some(session.request_headers()), None)
                .await
                .with_context(|| format!("fetching target {target}"))?;
            let kind =
                classify_content_type(fetched.content_type.as_deref(), Some(&fetched.final_url));
            if kind == AssetKind::Html {
                session.ingest_html(
                    &fetched.final_url,
                    &fetched.text(),
                    Some(fetched.final_url.to_string()),
                );
            } else {
                session.ingest_text(
                    &fetched.final_url,
                    kind,
                    &fetched.text(),
                    Some(fetched.final_url.to_string()),
                );
            }
            session.enqueue_well_known();
            session.enqueue_page_links(0);
            session.drain_pages().await?;
            session.drain_assets().await?;
            session.finalize().await
        }
        .await;
        if let Err(err) = &result {
            self.mark_failed(scan_id, err);
        }
        result
    }

    pub async fn run_snapshot(
        &self,
        snapshot: ScanSnapshot,
        options: ScanOptions,
    ) -> Result<ScanRecord> {
        let target = Url::parse(&snapshot.page_url)
            .or_else(|_| Url::parse(&snapshot.target))
            .context("snapshot has no valid target URL")?;
        let mut session = self.session(target.clone(), options.clone()).await?;
        let scan_id = session.scan_id;
        let result = async {
            if let Some(html) = &snapshot.html {
                session.ingest_html(&target, html, Some("extension snapshot".into()));
            }
            for script in &snapshot.inline_scripts {
                session.ingest_text(
                    &target,
                    AssetKind::JavaScript,
                    script,
                    Some("inline script".into()),
                );
            }
            for resource in &snapshot.resources {
                if let Ok(url) = Url::parse(&resource.url) {
                    if let Some(inline) = &resource.inline {
                        let kind = AssetKind::parse(&resource.kind);
                        session.ingest_text(
                            &url,
                            kind,
                            inline,
                            Some("extension inline resource".into()),
                        );
                    } else {
                        let kind = AssetKind::parse(&resource.kind);
                        session.enqueue_asset(url, kind, Some("extension resource".into()));
                    }
                }
            }
            for request in &snapshot.network {
                if let Ok(url) = Url::parse(&request.url) {
                    session.add_endpoint(
                        &url,
                        &request.method,
                        Some("browser network".into()),
                        None,
                    );
                }
            }
            session.enqueue_well_known();
            session.enqueue_page_links(0);
            session.drain_pages().await?;
            session.drain_assets().await?;
            session.finalize().await
        }
        .await;
        if let Err(err) = &result {
            self.mark_failed(scan_id, err);
        }
        result
    }

    fn mark_failed(&self, scan_id: Uuid, err: &anyhow::Error) {
        let summary = self.store.count_for_scan(scan_id).unwrap_or_default();
        let _ = self
            .store
            .finish_scan(scan_id, ScanStatus::Failed, &summary);
        self.events.scan(ScanEvent::Failed {
            scan_id,
            error: err.to_string(),
        });
    }

    async fn session(&self, target: Url, options: ScanOptions) -> Result<ScanSession> {
        let scope = match options.scope_file.as_deref() {
            Some(path) if path.exists() => Scope::from_file(path, &target)?,
            _ => Scope::from_target(&target),
        };
        let scope = Arc::new(scope);
        let http = Arc::new(HttpClient::new(self.config.clone(), scope.clone())?);
        let mut decision_config = self.config.decisions.clone();
        if !options.decisions {
            decision_config.enabled = false;
        }
        let decisions = Arc::new(DecisionEngine::new(decision_config, self.store.clone()).await?);
        let scan = self.store.create_scan(target.as_str())?;
        self.events.scan(ScanEvent::Started {
            scan_id: scan.id,
            target: target.to_string(),
        });
        self.events.scan(ScanEvent::Stage {
            scan_id: scan.id,
            stage: "init".into(),
            message: format!("starting scan of {target}"),
            progress: 0.0,
        });
        Ok(ScanSession {
            scan_id: scan.id,
            target,
            scope,
            http,
            decisions,
            notifier: self.notifier.clone(),
            store: self.store.clone(),
            events: self.events.clone(),
            config: self.config.clone(),
            options,
            pages: VecDeque::new(),
            assets: VecDeque::new(),
            seen_assets: HashSet::new(),
            seen_pages: HashSet::new(),
            endpoints: HashMap::new(),
            secrets: HashMap::new(),
            asset_count: 0,
            page_count: 0,
            started: Instant::now(),
        })
    }
}

#[derive(Clone)]
struct PendingAsset {
    url: Url,
    kind: AssetKind,
    source: Option<String>,
    inline: Option<String>,
}

struct ScanSession {
    scan_id: Uuid,
    target: Url,
    scope: Arc<Scope>,
    http: Arc<HttpClient>,
    decisions: Arc<DecisionEngine>,
    notifier: Notifier,
    store: Store,
    events: EventBus,
    config: Config,
    options: ScanOptions,
    pages: VecDeque<(Url, usize)>,
    assets: VecDeque<PendingAsset>,
    seen_assets: HashSet<String>,
    seen_pages: HashSet<String>,
    endpoints: HashMap<String, Endpoint>,
    secrets: HashMap<String, SecretFinding>,
    asset_count: usize,
    page_count: usize,
    started: Instant,
}

impl ScanSession {
    fn request_headers(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for (name, value) in &self.options.headers {
            if let (Ok(name), Ok(value)) = (
                HeaderName::from_bytes(name.as_bytes()),
                HeaderValue::from_str(value),
            ) {
                headers.insert(name, value);
            }
        }
        if let Some(cookie) = &self.options.cookie
            && let Ok(value) = HeaderValue::from_str(cookie)
        {
            headers.insert(COOKIE, value);
        }
        headers
    }

    fn ingest_html(&mut self, base: &Url, html: &str, source: Option<String>) {
        self.seen_pages.insert(base.to_string());
        let extracted = parse::html::parse(html, base);
        self.process_extracted(extracted, base, source.clone());
        self.add_endpoint(base, "GET", source, Some("page".into()));
    }

    fn ingest_text(&mut self, base: &Url, kind: AssetKind, text: &str, source: Option<String>) {
        let extracted = parse::parse(kind, text, base);
        self.process_extracted(extracted, base, source.clone());
        if kind == AssetKind::JavaScript || kind == AssetKind::Json || kind == AssetKind::GraphQl {
            self.scan_secrets(base, text);
        }
    }

    fn process_extracted(&mut self, extracted: Extracted, base: &Url, source: Option<String>) {
        for asset in extracted.assets {
            if let Ok(url) = Url::parse(&asset.url) {
                self.enqueue_asset(url, asset.kind, source.clone());
            }
        }
        for endpoint in extracted.endpoints {
            if let Ok(url) = self.resolve(&endpoint.url, base) {
                let method = endpoint.method.as_deref().unwrap_or("GET");
                self.add_endpoint_with_context(
                    &url,
                    method,
                    source.clone(),
                    endpoint.context.clone(),
                    endpoint.source_line,
                );
            }
        }
        for map in extracted.source_map_urls {
            if let Ok(url) = self.resolve(&map, base) {
                self.enqueue_asset(url, AssetKind::SourceMap, source.clone());
            }
        }
        for script in extracted.inline_scripts {
            if script.len() <= self.config.general.max_body_bytes {
                self.ingest_text(base, AssetKind::JavaScript, &script, source.clone());
            }
        }
        for _comment in extracted.comments {
            // Comments are already scanned by the raw extractor. Keeping this loop
            // makes the intent explicit for future comment-specific rules.
        }
    }

    fn resolve(&self, raw: &str, base: &Url) -> Result<Url> {
        if raw.starts_with("http://")
            || raw.starts_with("https://")
            || raw.starts_with("ws://")
            || raw.starts_with("wss://")
        {
            Ok(Url::parse(raw)?)
        } else {
            Ok(base.join(raw)?)
        }
    }

    fn enqueue_well_known(&mut self) {
        for path in [
            "/robots.txt",
            "/sitemap.xml",
            "/.well-known/security.txt",
            "/manifest.json",
            "/asset-manifest.json",
            "/webpack-manifest.json",
        ] {
            if let Ok(url) = self.target.join(path) {
                let kind = crate::engine::classify::classify_url(&url);
                self.enqueue_asset(url, kind, Some("well-known".into()));
            }
        }
    }

    fn enqueue_asset(&mut self, url: Url, kind: AssetKind, source: Option<String>) {
        if self.asset_count >= self.options.max_assets {
            return;
        }
        if !self.scope.is_allowed(&url) {
            return;
        }
        let key = url.to_string();
        if !self.seen_assets.insert(key) {
            return;
        }
        self.assets.push_back(PendingAsset {
            url,
            kind,
            source,
            inline: None,
        });
    }

    fn add_endpoint(
        &mut self,
        url: &Url,
        method: &str,
        source: Option<String>,
        context: Option<String>,
    ) {
        self.add_endpoint_with_context(url, method, source, context, None);
    }

    fn add_endpoint_with_context(
        &mut self,
        url: &Url,
        method: &str,
        source: Option<String>,
        context: Option<String>,
        line: Option<u32>,
    ) {
        let normalizer = Normalizer::new(self.target.clone(), (*self.scope).clone());
        let raw = crate::engine::parse::ExtractedEndpoint {
            url: url.to_string(),
            method: Some(method.to_ascii_uppercase()),
            source_line: line,
            context: context.clone(),
        };
        let Some(normalized) = normalizer.normalize_endpoint(&raw, source.clone()) else {
            return;
        };
        let key = format!("{} {}", normalized.method, normalized.normalized);
        if self.endpoints.contains_key(&key) {
            return;
        }
        let generic_context = normalized
            .context
            .as_deref()
            .map(|ctx| {
                ctx.contains("fetch")
                    || ctx.contains("axios")
                    || ctx.contains(".open")
                    || ctx.contains("sendBeacon")
                    || ctx.is_empty()
            })
            .unwrap_or(true);
        if normalized.method == "GET" && generic_context {
            let has_other_method = ["POST", "PUT", "PATCH", "DELETE"].iter().any(|method| {
                self.endpoints
                    .contains_key(&format!("{} {}", method, normalized.normalized))
            });
            if has_other_method {
                return;
            }
        }
        if normalized.method != "GET" {
            let get_key = format!("GET {}", normalized.normalized);
            let remove_generic_get = self
                .endpoints
                .get(&get_key)
                .map(|existing| {
                    existing
                        .context
                        .as_deref()
                        .map(|ctx| {
                            ctx.contains("fetch")
                                || ctx.contains("axios")
                                || ctx.contains(".open")
                                || ctx.contains("sendBeacon")
                                || ctx.is_empty()
                        })
                        .unwrap_or(true)
                })
                .unwrap_or(false);
            if remove_generic_get {
                self.endpoints.remove(&get_key);
            }
        }
        let heuristic = heuristic_endpoint(&normalized.url, &normalized.method);
        let endpoint = Endpoint {
            id: Uuid::new_v4(),
            scan_id: self.scan_id,
            method: normalized.method,
            url: normalized.url,
            normalized: normalized.normalized,
            path_template: normalized.path_template,
            params: normalized.params,
            source_asset: source,
            source_line: normalized.source_line,
            context: normalized.context,
            role: heuristic.role,
            sensitivity: heuristic.sensitivity,
            auth_required: heuristic.auth_required,
            verify_safe: heuristic.verify_safe,
            priority: heuristic.priority,
            confidence: heuristic.confidence,
            provider: heuristic.provider,
            discovered_at: chrono::Utc::now(),
        };
        self.endpoints.insert(key, endpoint);
    }

    fn scan_secrets(&mut self, base: &Url, text: &str) {
        if !self.config.recon.scan_secrets {
            return;
        }
        let asset_url = base.to_string();
        for candidate in secrets::scan(text, &asset_url) {
            let key = format!(
                "{}:{}:{}:{}",
                candidate.rule_id, candidate.masked_value, candidate.line, asset_url
            );
            if self.secrets.contains_key(&key) {
                continue;
            }
            let heuristic = heuristic_secret(&candidate.severity, candidate.entropy);
            let finding = SecretFinding {
                id: Uuid::new_v4(),
                scan_id: self.scan_id,
                asset_url: asset_url.clone(),
                line: candidate.line,
                column: candidate.column,
                rule_id: candidate.rule_id,
                provider: candidate.provider,
                description: candidate.description,
                severity: candidate.severity,
                masked_value: candidate.masked_value,
                entropy: candidate.entropy,
                is_real: heuristic.is_real,
                risk: heuristic.risk,
                validated: None,
                context: Some(candidate.context),
                discovered_at: chrono::Utc::now(),
            };
            self.secrets.insert(key, finding);
        }
    }

    async fn drain_pages(&mut self) -> Result<()> {
        while let Some((url, depth)) = self.pages.pop_front() {
            if self.page_count >= self.options.max_pages {
                break;
            }
            if depth > self.options.crawl_depth {
                continue;
            }
            self.page_count += 1;
            self.emit_stage(
                "crawl",
                format!("fetching page {url}"),
                (self.page_count as f32 / self.options.max_pages.max(1) as f32).min(0.35),
            );
            match self
                .http
                .fetch(&url, Method::GET, Some(self.request_headers()), None)
                .await
            {
                Ok(fetched) => {
                    let kind = classify_content_type(
                        fetched.content_type.as_deref(),
                        Some(&fetched.final_url),
                    );
                    if kind == AssetKind::Html {
                        self.ingest_html(
                            &fetched.final_url,
                            &fetched.text(),
                            Some(fetched.final_url.to_string()),
                        );
                    } else {
                        self.ingest_text(
                            &fetched.final_url,
                            kind,
                            &fetched.text(),
                            Some(fetched.final_url.to_string()),
                        );
                    }
                    self.enqueue_page_links(depth);
                }
                Err(err) => self.log("warn", format!("page fetch failed: {err}")),
            }
        }
        Ok(())
    }

    fn enqueue_page_links(&mut self, depth: usize) {
        if depth >= self.options.crawl_depth {
            return;
        }
        // Page links are enqueued by the HTML parser as endpoints, but only URLs
        // that look like pages are promoted to crawl targets here.
        let page_candidates: Vec<Url> = self
            .endpoints
            .values()
            .filter(|ep| {
                ep.method.eq_ignore_ascii_case("GET")
                    && ep.context.as_deref().is_some_and(|ctx| {
                        ctx.contains("a[href]")
                            || ctx.contains("iframe")
                            || ctx.contains("form[action]")
                            || ctx == "page"
                    })
            })
            .filter_map(|ep| Url::parse(&ep.url).ok())
            .filter(|url| is_page_url(url) && self.scope.is_allowed(url))
            .collect();
        for url in page_candidates {
            self.enqueue_page(url, depth + 1);
        }
    }

    fn enqueue_page(&mut self, url: Url, depth: usize) {
        if depth > self.options.crawl_depth {
            return;
        }
        if !self.scope.is_allowed(&url) {
            return;
        }
        let key = url.to_string();
        if self.seen_pages.insert(key) {
            self.pages.push_back((url, depth));
        }
    }

    async fn drain_assets(&mut self) -> Result<()> {
        let concurrency = self.config.general.concurrency.max(1);
        let semaphore = Arc::new(Semaphore::new(concurrency));
        while !self.assets.is_empty() {
            if self.asset_count >= self.options.max_assets {
                break;
            }
            let mut batch = Vec::new();
            for _ in 0..concurrency {
                if let Some(asset) = self.assets.pop_front() {
                    batch.push(asset);
                }
            }
            let mut futures = Vec::new();
            for pending in batch {
                let permit = semaphore.clone().acquire_owned().await?;
                let http = self.http.clone();
                let headers = self.request_headers();
                futures.push(async move {
                    let _permit = permit;
                    let inline = pending.inline.clone();
                    if let Some(inline) = inline {
                        return (pending, Ok(None), Some(inline));
                    }
                    let result = http
                        .fetch(&pending.url, Method::GET, Some(headers), None)
                        .await;
                    (pending, result.map(Some), None)
                });
            }
            let results = join_all(futures).await;
            for (pending, result, inline) in results {
                self.asset_count += 1;
                let fetched = match result {
                    Ok(fetched) => fetched,
                    Err(err) => {
                        self.log("warn", format!("asset fetch failed {}: {err}", pending.url));
                        continue;
                    }
                };
                if let Some(text) = inline {
                    self.ingest_text(&pending.url, pending.kind, &text, pending.source);
                    continue;
                }
                if let Some(fetched) = fetched {
                    self.process_fetched(pending, fetched).await;
                }
            }
            self.emit_stage(
                "assets",
                format!("processed {} assets", self.asset_count),
                0.35 + 0.35
                    * (self.asset_count as f32 / self.options.max_assets.max(1) as f32).min(1.0),
            );
        }
        Ok(())
    }

    async fn process_fetched(&mut self, pending: PendingAsset, fetched: Fetched) {
        let kind = if pending.kind == AssetKind::Other {
            classify_content_type(fetched.content_type.as_deref(), Some(&fetched.final_url))
        } else {
            pending.kind
        };
        let text = if kind.is_text() {
            Some(fetched.text())
        } else {
            None
        };
        let asset = Asset {
            id: Uuid::new_v4(),
            scan_id: self.scan_id,
            url: fetched.final_url.to_string(),
            kind,
            content_type: fetched.content_type.clone(),
            status: Some(fetched.status),
            size: fetched.body.len(),
            sha256: Some(sha256_hex(&fetched.body)),
            source: pending.source.clone(),
            fetched_at: chrono::Utc::now(),
        };
        if let Err(err) = self.store.insert_asset(&asset) {
            self.log("warn", format!("failed to store asset: {err}"));
        }
        self.events.scan(ScanEvent::Asset {
            scan_id: self.scan_id,
            asset: asset.clone(),
        });
        if is_static(kind) {
            return;
        }
        if let Some(text) = text {
            self.ingest_text(
                &fetched.final_url,
                kind,
                &text,
                Some(fetched.final_url.to_string()),
            );
        }
    }

    async fn finalize(mut self) -> Result<ScanRecord> {
        self.emit_stage(
            "decisions",
            "triaging endpoints and secrets".to_string(),
            0.72,
        );
        let mut endpoints: Vec<Endpoint> = self.endpoints.drain().map(|(_, v)| v).collect();
        endpoints.sort_by(|a, b| b.priority.total_cmp(&a.priority));

        for (idx, endpoint) in endpoints.iter_mut().enumerate() {
            if idx < self.options.max_model_endpoints {
                let state = json!({
                    "target": self.target.to_string(),
                    "endpoint": {
                        "method": endpoint.method,
                        "url": endpoint.url,
                        "path_template": endpoint.path_template,
                        "params": endpoint.params,
                        "source": endpoint.source_asset,
                        "context": endpoint.context,
                    },
                    "heuristic": {
                        "role": endpoint.role,
                        "sensitivity": endpoint.sensitivity,
                    }
                });
                if let Some(triage) = self
                    .decisions
                    .triage_endpoint(self.scan_id, &endpoint.id.to_string(), state)
                    .await
                {
                    apply_endpoint_triage(endpoint, triage);
                }
            }
            if let Err(err) = self.store.insert_endpoint(endpoint) {
                self.log("warn", format!("failed to store endpoint: {err}"));
            }
            self.events.scan(ScanEvent::Endpoint {
                scan_id: self.scan_id,
                endpoint: endpoint.clone(),
            });
        }

        let mut secrets: Vec<SecretFinding> = self.secrets.drain().map(|(_, v)| v).collect();
        secrets.sort_by(|a, b| b.risk.total_cmp(&a.risk));
        for (idx, secret) in secrets.iter_mut().enumerate() {
            if idx < self.options.max_model_secrets {
                let state = json!({
                    "target": self.target.to_string(),
                    "secret": {
                        "rule": secret.rule_id,
                        "provider": secret.provider,
                        "severity": secret.severity,
                        "masked_value": secret.masked_value,
                        "entropy": secret.entropy,
                        "context": secret.context,
                        "asset": secret.asset_url,
                    }
                });
                if let Some(triage) = self
                    .decisions
                    .triage_secret(self.scan_id, &secret.id.to_string(), state)
                    .await
                {
                    apply_secret_triage(secret, triage);
                }
            }
            if let Err(err) = self.store.insert_secret(secret) {
                self.log("warn", format!("failed to store secret: {err}"));
            }
            self.events.scan(ScanEvent::Secret {
                scan_id: self.scan_id,
                secret: secret.clone(),
            });
        }

        if self.options.verify {
            self.emit_stage("verify", "verifying endpoints".to_string(), 0.82);
            self.verify_endpoints().await;
        }

        let mut summary = self.store.count_for_scan(self.scan_id)?;
        summary.duration_ms = self.started.elapsed().as_millis() as u64;
        let status = ScanStatus::Completed;
        self.store.finish_scan(self.scan_id, status, &summary)?;

        let report_path = report::write_reports(&self.store, self.scan_id, &self.config).ok();
        self.events.scan(ScanEvent::Completed {
            scan_id: self.scan_id,
            summary: summary.clone(),
        });
        if self.options.notify {
            let errors = self
                .notifier
                .notify_scan(
                    self.target.as_str(),
                    &summary,
                    report_path.as_deref(),
                    "complete",
                )
                .await;
            for error in errors {
                self.log("warn", format!("notification failed: {error}"));
            }
        }
        info!(scan_id = %self.scan_id, "scan complete");
        self.store
            .load_scan(self.scan_id)?
            .context("scan disappeared after completion")
    }

    async fn verify_endpoints(&mut self) {
        let endpoints: Vec<Endpoint> = self
            .store
            .endpoints_for_scan(self.scan_id)
            .unwrap_or_default();
        let safe_methods = &self.config.recon.safe_methods;
        let mut verified = 0;
        for endpoint in endpoints {
            if verified >= self.options.max_verify {
                break;
            }
            let method = endpoint.method.to_ascii_uppercase();
            if endpoint.verify_safe == Some(false) {
                continue;
            }
            if !self.options.unsafe_verify && !safe_methods.iter().any(|m| m == &method) {
                continue;
            }
            let Ok(url) = Url::parse(&endpoint.url) else {
                continue;
            };
            if !self.scope.is_allowed(&url) {
                continue;
            }
            let method = if safe_methods.iter().any(|m| m == &method) {
                method
            } else {
                "GET".to_string()
            };
            let Ok(method) = Method::from_bytes(method.as_bytes()) else {
                continue;
            };
            verified += 1;
            let result = self
                .http
                .fetch(&url, method.clone(), Some(self.request_headers()), None)
                .await;
            let verification = match result {
                Ok(fetched) => {
                    build_verification(self.scan_id, endpoint.id, method.as_str(), &fetched)
                }
                Err(err) => Verification {
                    id: Uuid::new_v4(),
                    scan_id: self.scan_id,
                    endpoint_id: endpoint.id,
                    method: method.to_string(),
                    status: None,
                    content_type: None,
                    content_length: None,
                    body_hash: None,
                    body_snippet: None,
                    redirects: Vec::new(),
                    duration_ms: 0,
                    error: Some(err.to_string()),
                    classification: "error".into(),
                    checked_at: chrono::Utc::now(),
                },
            };
            if let Err(err) = self.store.insert_verification(&verification) {
                self.log("warn", format!("failed to store verification: {err}"));
            }
            self.events.scan(ScanEvent::Verification {
                scan_id: self.scan_id,
                verification,
            });
        }
    }

    fn emit_stage(&self, stage: &str, message: String, progress: f32) {
        self.events.scan(ScanEvent::Stage {
            scan_id: self.scan_id,
            stage: stage.to_string(),
            message,
            progress,
        });
    }

    fn log(&self, level: &str, message: String) {
        self.events.scan(ScanEvent::Log {
            scan_id: self.scan_id,
            level: level.to_string(),
            message,
        });
    }
}

fn apply_endpoint_triage(endpoint: &mut Endpoint, triage: EndpointTriage) {
    endpoint.role = triage.role;
    endpoint.sensitivity = triage.sensitivity;
    endpoint.auth_required = triage.auth_required;
    endpoint.verify_safe = triage.verify_safe;
    endpoint.priority = triage.priority;
    endpoint.confidence = triage.confidence;
    endpoint.provider = triage.provider;
}

fn apply_secret_triage(secret: &mut SecretFinding, triage: SecretTriage) {
    secret.is_real = triage.is_real;
    secret.risk = triage.risk;
    secret.validated = None;
    if secret.severity == "medium" && triage.risk >= 0.8 {
        secret.severity = "high".into();
    }
}

fn build_verification(
    scan_id: Uuid,
    endpoint_id: Uuid,
    method: &str,
    fetched: &Fetched,
) -> Verification {
    let classification = classify_status(fetched.status, fetched.content_type.as_deref());
    let body_snippet = if fetched.body.is_empty() {
        None
    } else {
        Some(redact_snippet(&fetched.text()))
    };
    Verification {
        id: Uuid::new_v4(),
        scan_id,
        endpoint_id,
        method: method.to_string(),
        status: Some(fetched.status),
        content_type: fetched.content_type.clone(),
        content_length: Some(fetched.body.len() as u64),
        body_hash: Some(sha256_hex(&fetched.body)),
        body_snippet,
        redirects: fetched.redirects.clone(),
        duration_ms: fetched.duration_ms,
        error: None,
        classification,
        checked_at: chrono::Utc::now(),
    }
}

fn classify_status(status: u16, content_type: Option<&str>) -> String {
    match status {
        200..=299 => {
            if content_type.unwrap_or("").contains("html") {
                "accessible_html".into()
            } else {
                "accessible".into()
            }
        }
        300..=399 => "redirect".into(),
        401 | 403 => "auth_required".into(),
        404 => "not_found".into(),
        405 => "method_not_allowed".into(),
        429 => "rate_limited".into(),
        500..=599 => "server_error".into(),
        _ => "other".into(),
    }
}

fn redact_snippet(text: &str) -> String {
    let mut out: String = text.chars().take(512).collect();
    for pattern in ["Bearer ", "sk-", "ghp_", "xoxb-", "AKIA"] {
        if let Some(idx) = out.find(pattern) {
            let end = (idx + pattern.len() + 12).min(out.len());
            out.replace_range(idx..end, "[redacted]");
        }
    }
    out
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use pscon::core::{
    AppEvent, BridgeEvent, Config, EventBus, ScanEvent, ScanSnapshot, Store, paths::config_path,
};
use pscon::engine::{ScanEngine, ScanOptions, report, setup_laya, start_bridge};
use tracing_subscriber::EnvFilter;
use url::Url;
use uuid::Uuid;

#[derive(Debug, Parser)]
#[command(
    name = "pscon",
    version,
    about = "Browser-assisted reconnaissance console for bug hunters"
)]
struct Cli {
    /// Path to a pscon config file.
    #[arg(long, global = true)]
    config: Option<PathBuf>,
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Scan a URL headlessly.
    Scan {
        url: String,
        #[arg(long)]
        depth: Option<usize>,
        #[arg(long)]
        max_pages: Option<usize>,
        #[arg(long)]
        max_assets: Option<usize>,
        #[arg(long)]
        max_model_endpoints: Option<usize>,
        #[arg(long)]
        max_verify: Option<usize>,
        #[arg(long)]
        no_verify: bool,
        #[arg(long)]
        unsafe_verify: bool,
        #[arg(long)]
        no_decisions: bool,
        #[arg(long)]
        no_notify: bool,
        #[arg(long)]
        scope: Option<PathBuf>,
        /// Extra HTTP header, e.g. "Authorization: Bearer ...".
        #[arg(long = "header")]
        headers: Vec<String>,
        #[arg(long)]
        cookie: Option<String>,
    },
    /// Open the interactive TUI.
    Tui {
        url: Option<String>,
        #[arg(long)]
        no_bridge: bool,
    },
    /// Run or manage the browser bridge.
    Bridge {
        #[command(subcommand)]
        command: BridgeCommands,
    },
    /// Import a JSner snapshot or a raw JSner export.
    Ingest {
        file: PathBuf,
        /// Target URL for raw JSner exports that do not contain one.
        #[arg(long)]
        target: Option<String>,
        #[arg(long)]
        verify: bool,
        #[arg(long)]
        decisions: bool,
    },
    /// Regenerate reports for a scan.
    Report { scan_id: String },
    /// Compare two scans.
    Diff {
        a: String,
        b: String,
        #[arg(long, default_value = "md")]
        format: String,
    },
    /// List scans.
    Scans,
    /// Decision-model commands.
    Decisions {
        #[command(subcommand)]
        command: DecisionCommands,
    },
    /// Notification commands.
    Notify {
        #[command(subcommand)]
        command: NotifyCommands,
    },
    /// Config commands.
    Config {
        #[command(subcommand)]
        command: ConfigCommands,
    },
}

#[derive(Debug, Subcommand)]
enum BridgeCommands {
    /// Start the local WebSocket/HTTP bridge.
    Serve,
    /// Print the current bridge token (for the extension popup).
    Token,
}

#[derive(Debug, Subcommand)]
enum DecisionCommands {
    /// Show configured providers and whether Laya is reachable.
    Test,
    /// Create a Python 3.12 venv and install laya[serve].
    SetupLaya,
    /// Evaluate a labels JSONL file (placeholder for the full eval harness).
    Eval { labels: PathBuf },
}

#[derive(Debug, Subcommand)]
enum NotifyCommands {
    /// Send a test notification.
    Test,
}

#[derive(Debug, Subcommand)]
enum ConfigCommands {
    /// Write a default config file.
    Init,
    /// Print the effective config.
    Show,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let command = cli.command.unwrap_or(Commands::Tui {
        url: None,
        no_bridge: false,
    });
    let is_tui = matches!(command, Commands::Tui { .. });
    if is_tui {
        // The TUI owns the terminal. Send tracing output to a file instead of
        // stderr so background scan warnings cannot corrupt the rendered frame.
        let log_path = pscon::core::data_dir().join("pscon.log");
        if let Some(parent) = log_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
        {
            tracing_subscriber::fmt()
                .with_env_filter(
                    EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn")),
                )
                .with_target(false)
                .with_writer(std::sync::Arc::new(file))
                .init();
        } else {
            tracing_subscriber::fmt()
                .with_env_filter(EnvFilter::new("off"))
                .init();
        }
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(
                EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn")),
            )
            .with_target(false)
            .init();
    }

    let mut config = match &cli.config {
        Some(path) => Config::load_from(path)?,
        None => Config::load()?,
    };
    pscon::core::paths::ensure_dirs()?;

    match command {
        Commands::Scan {
            url,
            depth,
            max_pages,
            max_assets,
            max_model_endpoints,
            max_verify,
            no_verify,
            unsafe_verify,
            no_decisions,
            no_notify,
            scope,
            headers,
            cookie,
        } => {
            let target = parse_target(&url)?;
            let mut options = ScanOptions::from_config(&config);
            if let Some(value) = depth {
                options.crawl_depth = value;
            }
            if let Some(value) = max_pages {
                options.max_pages = value;
            }
            if let Some(value) = max_assets {
                options.max_assets = value;
            }
            if let Some(value) = max_model_endpoints {
                options.max_model_endpoints = value;
            }
            if let Some(value) = max_verify {
                options.max_verify = value;
            }
            options.verify = !no_verify;
            options.unsafe_verify = unsafe_verify;
            options.decisions = !no_decisions;
            options.notify = !no_notify;
            if scope.is_some() {
                options.scope_file = scope;
            }
            options.headers = parse_headers(&headers)?;
            options.cookie = cookie;

            let events = EventBus::new(1024);
            spawn_event_printer(events.clone());
            let store = Store::open(&config.db_path())?;
            let engine = ScanEngine::new(config.clone(), store.clone(), events.clone());
            let scan = engine.run(target, options).await?;
            let report_path = report::report_path(&config, scan.id);
            println!("scan:   {}", scan.id);
            println!("status: {}", scan.status.as_str());
            println!(
                "found:  {} assets, {} endpoints, {} secrets, {} decisions",
                scan.summary.assets,
                scan.summary.endpoints,
                scan.summary.secrets,
                scan.summary.decisions
            );
            println!("report: {}", report_path.display());
        }
        Commands::Tui { url, no_bridge } => {
            let target = match url {
                Some(url) => Some(parse_target(&url)?),
                None => None,
            };
            let events = EventBus::new(2048);
            let store = Store::open(&config.db_path())?;
            let engine = Arc::new(ScanEngine::new(
                config.clone(),
                store.clone(),
                events.clone(),
            ));
            let bridge = if no_bridge {
                None
            } else {
                Some(start_bridge(&config, events.clone()).await?)
            };
            pscon::tui::run(engine, store, events, config, target, bridge).await?;
        }
        Commands::Bridge { command } => match command {
            BridgeCommands::Token => {
                let token = config.bridge_token().or_else(|| {
                    std::fs::read_to_string(pscon::core::paths::bridge_token_path())
                        .ok()
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                });
                match token {
                    Some(token) => println!("{token}"),
                    None => eprintln!(
                        "no bridge token yet; run `pscon bridge serve` or `pscon tui` first"
                    ),
                }
            }
            BridgeCommands::Serve => {
                let events = EventBus::new(1024);
                spawn_event_printer(events.clone());
                let mut receiver = events.subscribe();
                let store = Store::open(&config.db_path())?;
                let engine = Arc::new(ScanEngine::new(config.clone(), store, events.clone()));
                let bridge = start_bridge(&config, events.clone()).await?;
                println!("bridge listening on {}", bridge.addr);
                println!("websocket: ws://{}/ws?token={}", bridge.addr, bridge.token);
                println!("ingest:    http://{}/ingest", bridge.addr);
                println!("token:     {}", bridge.token);
                println!("Browser snapshots start scans automatically. Press Ctrl-C to stop.");
                let mut scans: tokio::task::JoinSet<Result<pscon::core::ScanRecord>> =
                    tokio::task::JoinSet::new();
                let mut pending = None;
                loop {
                    tokio::select! {
                        _ = tokio::signal::ctrl_c() => break,
                        event = receiver.recv() => match event {
                            Ok(AppEvent::Bridge(BridgeEvent::Snapshot { snapshot })) => {
                                // Match the TUI: keep the newest snapshot while a scan runs.
                                pending = Some(snapshot);
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                            _ => {},
                        },
                        Some(result) = scans.join_next(), if !scans.is_empty() => {
                            match result {
                                Ok(Ok(scan)) => println!("scan complete: {}", scan.id),
                                Ok(Err(error)) => events.scan(ScanEvent::Failed { scan_id: Uuid::nil(), error: error.to_string() }),
                                Err(error) => events.scan(ScanEvent::Failed { scan_id: Uuid::nil(), error: error.to_string() }),
                            }
                        }
                    }
                    if scans.is_empty()
                        && let Some(snapshot) = pending.take()
                    {
                        let engine = engine.clone();
                        let options = ScanOptions::from_config(&config);
                        scans.spawn(async move { engine.run_snapshot(snapshot, options).await });
                    }
                }
            }
        },
        Commands::Ingest {
            file,
            target,
            verify,
            decisions,
        } => {
            let text = std::fs::read_to_string(&file)
                .with_context(|| format!("reading {}", file.display()))?;
            let snapshot = parse_snapshot(&text, target.as_deref())?;
            let events = EventBus::new(1024);
            spawn_event_printer(events.clone());
            let store = Store::open(&config.db_path())?;
            let engine = ScanEngine::new(config.clone(), store, events);
            let mut options = ScanOptions::from_config(&config);
            options.verify = verify;
            options.decisions = decisions;
            options.notify = false;
            let scan = engine.run_snapshot(snapshot, options).await?;
            println!("scan:   {}", scan.id);
            println!(
                "report: {}",
                report::report_path(&config, scan.id).display()
            );
        }
        Commands::Report { scan_id } => {
            let id = parse_uuid(&scan_id)?;
            let store = Store::open(&config.db_path())?;
            let path = report::write_reports(&store, id, &config)?;
            println!("{}", path.display());
        }
        Commands::Diff { a, b, format } => {
            let a = parse_uuid(&a)?;
            let b = parse_uuid(&b)?;
            let store = Store::open(&config.db_path())?;
            print_diff(&store, a, b, &format)?;
        }
        Commands::Scans => {
            let store = Store::open(&config.db_path())?;
            for scan in store.list_scans(50)? {
                println!(
                    "{}  {:<10}  {:>5} ep  {:>5} sec  {}",
                    scan.id,
                    scan.status.as_str(),
                    scan.summary.endpoints,
                    scan.summary.secrets,
                    scan.target
                );
            }
        }
        Commands::Decisions { command } => match command {
            DecisionCommands::Test => {
                let store = Store::open(&config.db_path())?;
                let engine =
                    pscon::engine::DecisionEngine::new(config.decisions.clone(), store).await?;
                println!("providers: {:?}", engine.provider_names());
                println!("laya: {}", config.decisions.laya.base_url);
                println!(
                    "jev key: {}",
                    if std::env::var(&config.decisions.jev.api_key_env).is_ok() {
                        "set"
                    } else {
                        "missing"
                    }
                );
                println!(
                    "luna key: {}",
                    if std::env::var(&config.decisions.luna.api_key_env).is_ok() {
                        "set"
                    } else {
                        "missing"
                    }
                );
            }
            DecisionCommands::SetupLaya => {
                setup_laya(&config.decisions.laya).await?;
                println!(
                    "Laya venv ready at {}",
                    config.decisions.laya.venv.display()
                );
            }
            DecisionCommands::Eval { labels } => {
                let store = Store::open(&config.db_path())?;
                let engine =
                    pscon::engine::DecisionEngine::new(config.decisions.clone(), store).await?;
                let text = std::fs::read_to_string(&labels)
                    .with_context(|| format!("reading {}", labels.display()))?;
                let mut total = 0usize;
                let mut correct = 0usize;
                let mut confidence_sum = 0.0f32;
                let mut cost = 0.0f64;
                for (line_no, line) in text.lines().enumerate() {
                    let line = line.trim();
                    if line.is_empty() || line.starts_with('#') {
                        continue;
                    }
                    let value: serde_json::Value = serde_json::from_str(line)
                        .with_context(|| format!("labels line {}", line_no + 1))?;
                    let state = value
                        .get("state")
                        .cloned()
                        .unwrap_or(serde_json::Value::Null);
                    let gold = value
                        .get("gold")
                        .and_then(|g| {
                            g.as_str().map(str::to_string).or_else(|| {
                                g.get("role").and_then(|r| r.as_str()).map(str::to_string)
                            })
                        })
                        .unwrap_or_default();
                    let response = engine
                        .decide(
                            Uuid::new_v4(),
                            "eval",
                            &format!("line-{}", line_no + 1),
                            state,
                            &pscon::engine::endpoint_triage(),
                        )
                        .await;
                    if let Some(response) = response {
                        cost += response.cost_usd;
                        if let Some((choice, confidence)) = response
                            .answer("role")
                            .and_then(|answer| answer.as_choice())
                        {
                            total += 1;
                            confidence_sum += confidence;
                            if choice == gold {
                                correct += 1;
                            }
                        }
                    }
                }
                if total == 0 {
                    println!("no decisions produced; check provider configuration");
                } else {
                    println!("decisions: {total}");
                    println!("accuracy:  {:.1}%", correct as f32 / total as f32 * 100.0);
                    println!("mean conf: {:.2}", confidence_sum / total as f32);
                    println!("cost:      ${cost:.4}");
                }
            }
        },
        Commands::Notify { command } => match command {
            NotifyCommands::Test => {
                let notifier = pscon::core::Notifier::new(config.notify.clone());
                let errors = notifier.test().await;
                if errors.is_empty() {
                    println!("notification sent");
                } else {
                    for error in errors {
                        eprintln!("notification error: {error}");
                    }
                }
            }
        },
        Commands::Config { command } => match command {
            ConfigCommands::Init => {
                let path = cli.config.unwrap_or_else(config_path);
                Config::default().save_to(&path)?;
                println!("{}", path.display());
            }
            ConfigCommands::Show => {
                config = match &cli.config {
                    Some(path) => Config::load_from(path)?,
                    None => Config::load()?,
                };
                println!("{}", toml::to_string_pretty(&config)?);
            }
        },
    }

    Ok(())
}

fn parse_target(raw: &str) -> Result<Url> {
    if raw.starts_with("http://") || raw.starts_with("https://") {
        Ok(Url::parse(raw)?)
    } else {
        Ok(Url::parse(&format!("https://{raw}"))?)
    }
}

fn parse_uuid(raw: &str) -> Result<Uuid> {
    Uuid::parse_str(raw).with_context(|| format!("invalid scan id: {raw}"))
}

fn parse_headers(headers: &[String]) -> Result<Vec<(String, String)>> {
    let mut out = Vec::new();
    for header in headers {
        let (name, value) = header
            .split_once(':')
            .with_context(|| format!("invalid header (expected Name: Value): {header}"))?;
        out.push((name.trim().to_string(), value.trim().to_string()));
    }
    Ok(out)
}

fn spawn_event_printer(events: EventBus) {
    tokio::spawn(async move {
        let mut rx = events.subscribe();
        while let Ok(event) = rx.recv().await {
            if let AppEvent::Scan(event) = event {
                match event {
                    ScanEvent::Stage {
                        stage,
                        message,
                        progress,
                        ..
                    } => eprintln!("[{stage:>9}] {:>3.0}% {message}", progress * 100.0),
                    ScanEvent::Log { level, message, .. } => eprintln!("[{level}] {message}"),
                    ScanEvent::Completed { summary, .. } => eprintln!(
                        "[complete] {} assets, {} endpoints, {} secrets, {} decisions",
                        summary.assets, summary.endpoints, summary.secrets, summary.decisions
                    ),
                    ScanEvent::Failed { error, .. } => eprintln!("[failed] {error}"),
                    _ => {}
                }
            }
        }
    });
}

fn parse_snapshot(text: &str, target: Option<&str>) -> Result<ScanSnapshot> {
    if let Ok(snapshot) = serde_json::from_str::<ScanSnapshot>(text) {
        return Ok(snapshot);
    }
    let value: serde_json::Value = serde_json::from_str(text).context("parsing snapshot JSON")?;
    let target = target
        .map(parse_target)
        .transpose()?
        .or_else(|| {
            value
                .get("target")
                .and_then(|v| v.as_str())
                .and_then(|s| Url::parse(s).ok())
        })
        .context("raw JSner export needs --target")?;
    let mut network = Vec::new();
    for key in ["endpoints", "apis", "graphql", "config", "other"] {
        if let Some(items) = value.get(key).and_then(|v| v.as_array()) {
            for item in items {
                if let Some(url) = item.as_str()
                    && let Ok(url) = target.join(url)
                {
                    network.push(pscon::core::NetworkRequest {
                        url: url.to_string(),
                        method: "GET".into(),
                        status: None,
                        resource_type: Some(key.into()),
                        initiator: Some("jsner-export".into()),
                    });
                }
            }
        }
    }
    Ok(ScanSnapshot {
        target: target.to_string(),
        page_url: target.to_string(),
        title: None,
        html: None,
        resources: Vec::new(),
        network,
        inline_scripts: Vec::new(),
        storage_keys: Vec::new(),
        extension_version: Some("jsner-export".into()),
        captured_at: chrono::Utc::now(),
    })
}

fn print_diff(store: &Store, a: Uuid, b: Uuid, format: &str) -> Result<()> {
    let scan_a = store.load_scan(a)?.context("scan A not found")?;
    let scan_b = store.load_scan(b)?.context("scan B not found")?;
    let eps_a = store.endpoints_for_scan(a)?;
    let eps_b = store.endpoints_for_scan(b)?;
    let set_a: BTreeSet<(String, String)> = eps_a
        .iter()
        .map(|e| (e.method.clone(), e.normalized.clone()))
        .collect();
    let set_b: BTreeSet<(String, String)> = eps_b
        .iter()
        .map(|e| (e.method.clone(), e.normalized.clone()))
        .collect();
    let new: Vec<_> = set_b.difference(&set_a).cloned().collect();
    let removed: Vec<_> = set_a.difference(&set_b).cloned().collect();
    let map_a: BTreeMap<_, _> = eps_a
        .iter()
        .map(|e| ((e.method.clone(), e.normalized.clone()), e))
        .collect();
    let map_b: BTreeMap<_, _> = eps_b
        .iter()
        .map(|e| ((e.method.clone(), e.normalized.clone()), e))
        .collect();
    let mut changed = Vec::new();
    for key in set_a.intersection(&set_b) {
        if let (Some(a), Some(b)) = (map_a.get(key), map_b.get(key))
            && (a.role != b.role || (a.priority - b.priority).abs() > 0.15)
        {
            changed.push((
                key.clone(),
                a.role.clone(),
                b.role.clone(),
                a.priority,
                b.priority,
            ));
        }
    }
    if format == "json" {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "a": scan_a.id,
                "b": scan_b.id,
                "new": new,
                "removed": removed,
                "changed": changed,
            }))?
        );
    } else {
        println!("# pscon diff\n");
        println!(
            "A: {} ({})\nB: {} ({})\n",
            scan_a.target, scan_a.id, scan_b.target, scan_b.id
        );
        println!("## New endpoints ({})", new.len());
        for (method, url) in &new {
            println!("- `{method} {url}`");
        }
        println!("\n## Removed endpoints ({})", removed.len());
        for (method, url) in &removed {
            println!("- `{method} {url}`");
        }
        println!("\n## Changed endpoints ({})", changed.len());
        for (key, old_role, new_role, old_priority, new_priority) in &changed {
            println!(
                "- `{} {}` role {} -> {}, priority {:.2} -> {:.2}",
                key.0, key.1, old_role, new_role, old_priority, new_priority
            );
        }
    }
    Ok(())
}

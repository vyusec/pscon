use std::time::Duration;

use crate::core::config::LayaConfig;
use anyhow::{Context, Result, bail};
use tokio::process::Command;
use tracing::{info, warn};
use url::Url;

pub async fn ensure_running(config: &LayaConfig) -> Result<()> {
    let base = config.base_url.trim_end_matches('/');
    if health_ok(base).await {
        return Ok(());
    }
    if !config.autostart {
        bail!("Laya is not running at {base} and autostart is disabled");
    }

    let server = config.venv.join("bin").join("laya-serve");
    if !server.exists() {
        bail!(
            "Laya is not running at {base} and {} was not found. Run `pscon decisions setup-laya`.",
            server.display()
        );
    }

    let port = Url::parse(&config.base_url)
        .ok()
        .and_then(|u| u.port())
        .unwrap_or(8766);
    let log_path = crate::core::data_dir().join("laya.log");
    if let Some(parent) = log_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .with_context(|| format!("opening {}", log_path.display()))?;
    let log_err = log.try_clone()?;
    info!(port, "starting laya-serve");
    Command::new(&server)
        .env("LAYA_HOST", "127.0.0.1")
        .env("LAYA_PORT", port.to_string())
        .env("LAYA_PRELOAD", "1")
        .env("LAYA_LOG_LEVEL", "warning")
        .stdout(std::process::Stdio::from(log))
        .stderr(std::process::Stdio::from(log_err))
        .spawn()
        .with_context(|| format!("starting {}", server.display()))?;

    for _ in 0..60 {
        tokio::time::sleep(Duration::from_secs(2)).await;
        if health_ok(base).await {
            return Ok(());
        }
    }
    warn!("laya-serve did not become healthy in time");
    bail!(
        "laya-serve did not become healthy; see {}",
        log_path.display()
    )
}

async fn health_ok(base: &str) -> bool {
    reqwest::Client::new()
        .get(format!("{base}/health"))
        .timeout(Duration::from_secs(2))
        .send()
        .await
        .map(|r| r.status().is_success())
        .unwrap_or(false)
}

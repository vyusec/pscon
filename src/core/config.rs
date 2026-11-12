use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::core::paths::{config_path, data_dir, ensure_dirs};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Config {
    pub general: GeneralConfig,
    pub scope: ScopeConfig,
    pub recon: ReconConfig,
    pub bridge: BridgeConfig,
    pub decisions: DecisionsConfig,
    pub notify: NotifyConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GeneralConfig {
    pub data_dir: Option<PathBuf>,
    pub concurrency: usize,
    pub timeout_secs: u64,
    pub max_body_bytes: usize,
    pub user_agent: String,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            data_dir: None,
            concurrency: 16,
            timeout_secs: 20,
            max_body_bytes: 5_000_000,
            user_agent: format!(
                "pscon/{} (+https://github.com/vyusec/pscon)",
                env!("CARGO_PKG_VERSION")
            ),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ScopeConfig {
    pub file: Option<PathBuf>,
    pub allow_private_ips: bool,
    pub respect_robots: bool,
}

impl Default for ScopeConfig {
    fn default() -> Self {
        Self {
            file: None,
            allow_private_ips: true,
            respect_robots: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ReconConfig {
    pub crawl_depth: usize,
    pub max_pages: usize,
    pub max_assets: usize,
    pub fetch_source_maps: bool,
    pub scan_secrets: bool,
    pub verify: bool,
    pub safe_methods: Vec<String>,
    pub rate_limit_per_host_ms: u64,
    pub max_redirects: usize,
    pub include_third_party: bool,
}

impl Default for ReconConfig {
    fn default() -> Self {
        Self {
            crawl_depth: 2,
            max_pages: 200,
            max_assets: 1000,
            fetch_source_maps: true,
            scan_secrets: true,
            verify: true,
            safe_methods: vec!["GET".into(), "HEAD".into(), "OPTIONS".into()],
            rate_limit_per_host_ms: 120,
            max_redirects: 5,
            include_third_party: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BridgeConfig {
    pub bind: String,
    pub port: u16,
    pub token: Option<String>,
    pub allowed_origins: Vec<String>,
    pub auto_start_laya: bool,
}

impl Default for BridgeConfig {
    fn default() -> Self {
        Self {
            bind: "127.0.0.1".into(),
            port: 8787,
            token: None,
            allowed_origins: vec!["chrome-extension://*".into(), "moz-extension://*".into()],
            auto_start_laya: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DecisionsConfig {
    pub enabled: bool,
    pub provider_order: Vec<String>,
    pub confidence_threshold: f32,
    pub high_impact_threshold: f32,
    pub max_cost_usd: f64,
    pub max_calls: usize,
    pub cache_ttl_days: i64,
    pub redact_cloud: bool,
    pub laya: LayaConfig,
    pub jev: JevConfig,
    pub luna: LunaConfig,
}

impl Default for DecisionsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            provider_order: vec!["laya".into(), "jev".into(), "luna".into()],
            confidence_threshold: 0.75,
            high_impact_threshold: 0.9,
            max_cost_usd: 0.50,
            max_calls: 5000,
            cache_ttl_days: 30,
            redact_cloud: true,
            laya: LayaConfig::default(),
            jev: JevConfig::default(),
            luna: LunaConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LayaConfig {
    pub base_url: String,
    pub autostart: bool,
    pub venv: PathBuf,
    pub python: String,
    pub api_key_env: String,
}

impl Default for LayaConfig {
    fn default() -> Self {
        Self {
            base_url: "http://127.0.0.1:8766".into(),
            autostart: true,
            venv: data_dir().join("laya-venv"),
            python: "python3.12".into(),
            api_key_env: "LAYA_API_KEY".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct JevConfig {
    pub base_url: String,
    pub api_key_env: String,
    pub model: String,
}

impl Default for JevConfig {
    fn default() -> Self {
        Self {
            base_url: "https://jevtypesafeai.com/api/v1/decide".into(),
            api_key_env: "JEV_API_KEY".into(),
            model: "jev-latest".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LunaConfig {
    pub base_url: String,
    pub api_key_env: String,
    pub model: String,
}

impl Default for LunaConfig {
    fn default() -> Self {
        Self {
            base_url: "https://api.openai.com/v1/decisions".into(),
            api_key_env: "OPENAI_API_KEY".into(),
            model: "gpt-6-luna".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct NotifyConfig {
    pub desktop: bool,
    pub ntfy_topic: String,
    pub ntfy_server: String,
    pub ntfy_token_env: String,
    pub webhook_url: String,
    pub webhook_kind: String,
    pub telegram_bot_token_env: String,
    pub telegram_chat_id: String,
    pub on: Vec<String>,
}

impl Default for NotifyConfig {
    fn default() -> Self {
        Self {
            desktop: true,
            ntfy_topic: String::new(),
            ntfy_server: "https://ntfy.sh".into(),
            ntfy_token_env: "NTFY_TOKEN".into(),
            webhook_url: String::new(),
            webhook_kind: "generic".into(),
            telegram_bot_token_env: "TELEGRAM_BOT_TOKEN".into(),
            telegram_chat_id: String::new(),
            on: vec!["complete".into(), "critical".into(), "failed".into()],
        }
    }
}

impl Config {
    pub fn load() -> anyhow::Result<Self> {
        let path = config_path();
        if path.exists() {
            let text = std::fs::read_to_string(&path)?;
            let cfg: Config = toml::from_str(&text)?;
            Ok(cfg)
        } else {
            let cfg = Config::default();
            cfg.save()?;
            Ok(cfg)
        }
    }

    pub fn load_from(path: &Path) -> anyhow::Result<Self> {
        let text = std::fs::read_to_string(path)?;
        let cfg: Config = toml::from_str(&text)?;
        Ok(cfg)
    }

    pub fn save_to(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = toml::to_string_pretty(self)?;
        std::fs::write(path, text)?;
        Ok(())
    }

    pub fn save(&self) -> anyhow::Result<()> {
        ensure_dirs()?;
        let path = config_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = toml::to_string_pretty(self)?;
        std::fs::write(&path, text)?;
        Ok(())
    }

    pub fn init() -> anyhow::Result<PathBuf> {
        let path = config_path();
        if !path.exists() {
            Config::default().save()?;
        }
        Ok(path)
    }

    pub fn data_dir(&self) -> PathBuf {
        self.general.data_dir.clone().unwrap_or_else(data_dir)
    }

    pub fn db_path(&self) -> PathBuf {
        self.data_dir().join("pscon.db")
    }

    pub fn scans_dir(&self) -> PathBuf {
        self.data_dir().join("scans")
    }

    pub fn scope_file(&self) -> Option<&Path> {
        self.scope.file.as_deref()
    }

    pub fn bridge_token(&self) -> Option<String> {
        self.bridge
            .token
            .clone()
            .or_else(|| std::env::var("PSCON_BRIDGE_TOKEN").ok())
    }
}

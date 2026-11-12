use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanStatus {
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl ScanStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            ScanStatus::Running => "running",
            ScanStatus::Completed => "completed",
            ScanStatus::Failed => "failed",
            ScanStatus::Cancelled => "cancelled",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "completed" => ScanStatus::Completed,
            "failed" => ScanStatus::Failed,
            "cancelled" => ScanStatus::Cancelled,
            _ => ScanStatus::Running,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanRecord {
    pub id: Uuid,
    pub target: String,
    pub status: ScanStatus,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub summary: ScanSummary,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ScanSummary {
    pub assets: usize,
    pub endpoints: usize,
    pub secrets: usize,
    pub verified: usize,
    pub high_risk: usize,
    pub decisions: usize,
    pub duration_ms: u64,
    pub cost_usd: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetKind {
    Html,
    JavaScript,
    Css,
    Json,
    Xml,
    GraphQl,
    SourceMap,
    Wasm,
    Text,
    Image,
    Font,
    Media,
    Other,
}

impl AssetKind {
    pub fn as_str(self) -> &'static str {
        match self {
            AssetKind::Html => "html",
            AssetKind::JavaScript => "javascript",
            AssetKind::Css => "css",
            AssetKind::Json => "json",
            AssetKind::Xml => "xml",
            AssetKind::GraphQl => "graphql",
            AssetKind::SourceMap => "source_map",
            AssetKind::Wasm => "wasm",
            AssetKind::Text => "text",
            AssetKind::Image => "image",
            AssetKind::Font => "font",
            AssetKind::Media => "media",
            AssetKind::Other => "other",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "html" => AssetKind::Html,
            "javascript" => AssetKind::JavaScript,
            "css" => AssetKind::Css,
            "json" => AssetKind::Json,
            "xml" => AssetKind::Xml,
            "graphql" => AssetKind::GraphQl,
            "source_map" => AssetKind::SourceMap,
            "wasm" => AssetKind::Wasm,
            "text" => AssetKind::Text,
            "image" => AssetKind::Image,
            "font" => AssetKind::Font,
            "media" => AssetKind::Media,
            _ => AssetKind::Other,
        }
    }

    pub fn is_text(self) -> bool {
        matches!(
            self,
            AssetKind::Html
                | AssetKind::JavaScript
                | AssetKind::Css
                | AssetKind::Json
                | AssetKind::Xml
                | AssetKind::GraphQl
                | AssetKind::SourceMap
                | AssetKind::Text
        )
    }

    pub fn is_script(self) -> bool {
        matches!(self, AssetKind::JavaScript)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Asset {
    pub id: Uuid,
    pub scan_id: Uuid,
    pub url: String,
    pub kind: AssetKind,
    pub content_type: Option<String>,
    pub status: Option<u16>,
    pub size: usize,
    pub sha256: Option<String>,
    pub source: Option<String>,
    pub fetched_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Endpoint {
    pub id: Uuid,
    pub scan_id: Uuid,
    pub method: String,
    pub url: String,
    pub normalized: String,
    pub path_template: String,
    pub params: Vec<String>,
    pub source_asset: Option<String>,
    pub source_line: Option<u32>,
    pub context: Option<String>,
    pub role: String,
    pub sensitivity: f32,
    pub auth_required: Option<bool>,
    pub verify_safe: Option<bool>,
    pub priority: f32,
    pub confidence: f32,
    pub provider: String,
    pub discovered_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretFinding {
    pub id: Uuid,
    pub scan_id: Uuid,
    pub asset_url: String,
    pub line: u32,
    pub column: u32,
    pub rule_id: String,
    pub provider: String,
    pub description: String,
    pub severity: String,
    pub masked_value: String,
    pub entropy: f32,
    pub is_real: Option<bool>,
    pub risk: f32,
    pub validated: Option<bool>,
    pub context: Option<String>,
    pub discovered_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Verification {
    pub id: Uuid,
    pub scan_id: Uuid,
    pub endpoint_id: Uuid,
    pub method: String,
    pub status: Option<u16>,
    pub content_type: Option<String>,
    pub content_length: Option<u64>,
    pub body_hash: Option<String>,
    pub body_snippet: Option<String>,
    pub redirects: Vec<String>,
    pub duration_ms: u64,
    pub error: Option<String>,
    pub classification: String,
    pub checked_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionRecord {
    pub id: Uuid,
    pub scan_id: Uuid,
    pub subject_kind: String,
    pub subject_id: String,
    pub question: String,
    pub answer: serde_json::Value,
    pub confidence: f32,
    pub provider: String,
    pub model: String,
    pub latency_ms: u64,
    pub cost_usd: f64,
    pub cached: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ScanEvent {
    Started {
        scan_id: Uuid,
        target: String,
    },
    Stage {
        scan_id: Uuid,
        stage: String,
        message: String,
        progress: f32,
    },
    Asset {
        scan_id: Uuid,
        asset: Asset,
    },
    Endpoint {
        scan_id: Uuid,
        endpoint: Endpoint,
    },
    Secret {
        scan_id: Uuid,
        secret: SecretFinding,
    },
    Decision {
        scan_id: Uuid,
        decision: DecisionRecord,
    },
    Verification {
        scan_id: Uuid,
        verification: Verification,
    },
    Log {
        scan_id: Uuid,
        level: String,
        message: String,
    },
    Completed {
        scan_id: Uuid,
        summary: ScanSummary,
    },
    Failed {
        scan_id: Uuid,
        error: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BridgeEvent {
    Hello {
        extension_version: String,
        browser: String,
        tab_url: Option<String>,
    },
    Snapshot {
        snapshot: ScanSnapshot,
    },
    Resource {
        url: String,
        status: u16,
        content_type: Option<String>,
        body_base64: String,
    },
    ScanProgress {
        scan_id: String,
        stage: String,
        message: String,
        progress: f32,
    },
    Disconnected,
    Log {
        level: String,
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanSnapshot {
    pub target: String,
    pub page_url: String,
    pub title: Option<String>,
    pub html: Option<String>,
    pub resources: Vec<SnapshotResource>,
    pub network: Vec<NetworkRequest>,
    pub inline_scripts: Vec<String>,
    pub storage_keys: Vec<String>,
    pub extension_version: Option<String>,
    pub captured_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotResource {
    pub url: String,
    pub kind: String,
    pub method: Option<String>,
    pub status: Option<u16>,
    pub content_type: Option<String>,
    pub initiator: Option<String>,
    pub size: Option<usize>,
    pub inline: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkRequest {
    pub url: String,
    pub method: String,
    pub status: Option<u16>,
    pub resource_type: Option<String>,
    pub initiator: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanReport {
    pub schema_version: String,
    pub scan: ScanRecord,
    pub assets: Vec<Asset>,
    pub endpoints: Vec<Endpoint>,
    pub secrets: Vec<SecretFinding>,
    pub verifications: Vec<Verification>,
    pub decisions: Vec<DecisionRecord>,
    pub stats: BTreeMap<String, usize>,
}

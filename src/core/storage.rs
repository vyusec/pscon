use std::path::Path;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, params};
use uuid::Uuid;

use crate::core::model::{
    Asset, AssetKind, DecisionRecord, Endpoint, ScanRecord, ScanStatus, ScanSummary, SecretFinding,
    Verification,
};

#[derive(Debug, Clone)]
pub struct CachedDecision {
    pub provider: String,
    pub model: String,
    pub response_json: String,
    pub confidence: f32,
    pub cost_usd: f64,
}

#[derive(Clone)]
pub struct Store {
    conn: Arc<Mutex<Connection>>,
}

impl Store {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let store = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        store.migrate()?;
        Ok(store)
    }

    pub fn open_in_memory() -> anyhow::Result<Self> {
        let conn = Connection::open_in_memory()?;
        let store = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&self) -> anyhow::Result<()> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS scans (
                id TEXT PRIMARY KEY,
                target TEXT NOT NULL,
                status TEXT NOT NULL,
                started_at TEXT NOT NULL,
                finished_at TEXT,
                summary_json TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS assets (
                id TEXT PRIMARY KEY,
                scan_id TEXT NOT NULL,
                url TEXT NOT NULL,
                kind TEXT NOT NULL,
                content_type TEXT,
                status INTEGER,
                size INTEGER NOT NULL,
                sha256 TEXT,
                source TEXT,
                fetched_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_assets_scan ON assets(scan_id);
            CREATE INDEX IF NOT EXISTS idx_assets_url ON assets(scan_id, url);

            CREATE TABLE IF NOT EXISTS endpoints (
                id TEXT PRIMARY KEY,
                scan_id TEXT NOT NULL,
                method TEXT NOT NULL,
                url TEXT NOT NULL,
                normalized TEXT NOT NULL,
                path_template TEXT NOT NULL,
                params_json TEXT NOT NULL,
                source_asset TEXT,
                source_line INTEGER,
                context TEXT,
                role TEXT NOT NULL,
                sensitivity REAL NOT NULL,
                auth_required INTEGER,
                verify_safe INTEGER,
                priority REAL NOT NULL,
                confidence REAL NOT NULL,
                provider TEXT NOT NULL,
                discovered_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_endpoints_scan ON endpoints(scan_id);
            CREATE INDEX IF NOT EXISTS idx_endpoints_norm ON endpoints(scan_id, normalized);

            CREATE TABLE IF NOT EXISTS secrets (
                id TEXT PRIMARY KEY,
                scan_id TEXT NOT NULL,
                asset_url TEXT NOT NULL,
                line INTEGER NOT NULL,
                column INTEGER NOT NULL,
                rule_id TEXT NOT NULL,
                provider TEXT NOT NULL,
                description TEXT NOT NULL,
                severity TEXT NOT NULL,
                masked_value TEXT NOT NULL,
                entropy REAL NOT NULL,
                is_real INTEGER,
                risk REAL NOT NULL,
                validated INTEGER,
                context TEXT,
                discovered_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_secrets_scan ON secrets(scan_id);

            CREATE TABLE IF NOT EXISTS verifications (
                id TEXT PRIMARY KEY,
                scan_id TEXT NOT NULL,
                endpoint_id TEXT NOT NULL,
                method TEXT NOT NULL,
                status INTEGER,
                content_type TEXT,
                content_length INTEGER,
                body_hash TEXT,
                body_snippet TEXT,
                redirects_json TEXT NOT NULL,
                duration_ms INTEGER NOT NULL,
                error TEXT,
                classification TEXT NOT NULL,
                checked_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_verifications_scan ON verifications(scan_id);

            CREATE TABLE IF NOT EXISTS decisions (
                id TEXT PRIMARY KEY,
                scan_id TEXT NOT NULL,
                subject_kind TEXT NOT NULL,
                subject_id TEXT NOT NULL,
                question TEXT NOT NULL,
                answer_json TEXT NOT NULL,
                confidence REAL NOT NULL,
                provider TEXT NOT NULL,
                model TEXT NOT NULL,
                latency_ms INTEGER NOT NULL,
                cost_usd REAL NOT NULL,
                cached INTEGER NOT NULL,
                created_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_decisions_scan ON decisions(scan_id);

            CREATE TABLE IF NOT EXISTS decision_cache (
                cache_key TEXT PRIMARY KEY,
                provider TEXT NOT NULL,
                model TEXT NOT NULL,
                response_json TEXT NOT NULL,
                confidence REAL NOT NULL,
                cost_usd REAL NOT NULL,
                created_at TEXT NOT NULL
            );
            "#,
        )?;
        Ok(())
    }

    pub fn create_scan(&self, target: &str) -> anyhow::Result<ScanRecord> {
        let record = ScanRecord {
            id: Uuid::new_v4(),
            target: target.to_string(),
            status: ScanStatus::Running,
            started_at: Utc::now(),
            finished_at: None,
            summary: ScanSummary::default(),
        };
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute(
            "INSERT INTO scans (id, target, status, started_at, finished_at, summary_json)
             VALUES (?1, ?2, ?3, ?4, NULL, ?5)",
            params![
                record.id.to_string(),
                record.target,
                record.status.as_str(),
                record.started_at.to_rfc3339(),
                serde_json::to_string(&record.summary)?,
            ],
        )?;
        Ok(record)
    }

    pub fn finish_scan(
        &self,
        id: Uuid,
        status: ScanStatus,
        summary: &ScanSummary,
    ) -> anyhow::Result<()> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute(
            "UPDATE scans SET status = ?1, finished_at = ?2, summary_json = ?3 WHERE id = ?4",
            params![
                status.as_str(),
                Utc::now().to_rfc3339(),
                serde_json::to_string(summary)?,
                id.to_string(),
            ],
        )?;
        Ok(())
    }

    pub fn load_scan(&self, id: Uuid) -> anyhow::Result<Option<ScanRecord>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let row = conn
            .query_row(
                "SELECT id, target, status, started_at, finished_at, summary_json FROM scans WHERE id = ?1",
                params![id.to_string()],
                row_to_scan,
            )
            .optional()?;
        Ok(row)
    }

    pub fn latest_scan(&self) -> anyhow::Result<Option<ScanRecord>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let row = conn
            .query_row(
                "SELECT id, target, status, started_at, finished_at, summary_json FROM scans ORDER BY started_at DESC LIMIT 1",
                [],
                row_to_scan,
            )
            .optional()?;
        Ok(row)
    }

    pub fn list_scans(&self, limit: usize) -> anyhow::Result<Vec<ScanRecord>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT id, target, status, started_at, finished_at, summary_json FROM scans ORDER BY started_at DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit as i64], row_to_scan)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn insert_asset(&self, asset: &Asset) -> anyhow::Result<()> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute(
            "INSERT OR REPLACE INTO assets
             (id, scan_id, url, kind, content_type, status, size, sha256, source, fetched_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                asset.id.to_string(),
                asset.scan_id.to_string(),
                asset.url,
                asset.kind.as_str(),
                asset.content_type,
                asset.status.map(|s| s as i64),
                asset.size as i64,
                asset.sha256,
                asset.source,
                asset.fetched_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn insert_endpoint(&self, endpoint: &Endpoint) -> anyhow::Result<()> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute(
            "INSERT OR REPLACE INTO endpoints
             (id, scan_id, method, url, normalized, path_template, params_json, source_asset, source_line,
              context, role, sensitivity, auth_required, verify_safe, priority, confidence, provider, discovered_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
            params![
                endpoint.id.to_string(),
                endpoint.scan_id.to_string(),
                endpoint.method,
                endpoint.url,
                endpoint.normalized,
                endpoint.path_template,
                serde_json::to_string(&endpoint.params)?,
                endpoint.source_asset,
                endpoint.source_line.map(|v| v as i64),
                endpoint.context,
                endpoint.role,
                endpoint.sensitivity,
                endpoint.auth_required.map(|v| v as i64),
                endpoint.verify_safe.map(|v| v as i64),
                endpoint.priority,
                endpoint.confidence,
                endpoint.provider,
                endpoint.discovered_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn insert_secret(&self, secret: &SecretFinding) -> anyhow::Result<()> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute(
            "INSERT OR REPLACE INTO secrets
             (id, scan_id, asset_url, line, column, rule_id, provider, description, severity,
              masked_value, entropy, is_real, risk, validated, context, discovered_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
            params![
                secret.id.to_string(),
                secret.scan_id.to_string(),
                secret.asset_url,
                secret.line as i64,
                secret.column as i64,
                secret.rule_id,
                secret.provider,
                secret.description,
                secret.severity,
                secret.masked_value,
                secret.entropy,
                secret.is_real.map(|v| v as i64),
                secret.risk,
                secret.validated.map(|v| v as i64),
                secret.context,
                secret.discovered_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn insert_verification(&self, verification: &Verification) -> anyhow::Result<()> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute(
            "INSERT OR REPLACE INTO verifications
             (id, scan_id, endpoint_id, method, status, content_type, content_length, body_hash,
              body_snippet, redirects_json, duration_ms, error, classification, checked_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                verification.id.to_string(),
                verification.scan_id.to_string(),
                verification.endpoint_id.to_string(),
                verification.method,
                verification.status.map(|s| s as i64),
                verification.content_type,
                verification.content_length.map(|v| v as i64),
                verification.body_hash,
                verification.body_snippet,
                serde_json::to_string(&verification.redirects)?,
                verification.duration_ms as i64,
                verification.error,
                verification.classification,
                verification.checked_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn insert_decision(&self, decision: &DecisionRecord) -> anyhow::Result<()> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute(
            "INSERT OR REPLACE INTO decisions
             (id, scan_id, subject_kind, subject_id, question, answer_json, confidence, provider,
              model, latency_ms, cost_usd, cached, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                decision.id.to_string(),
                decision.scan_id.to_string(),
                decision.subject_kind,
                decision.subject_id,
                decision.question,
                serde_json::to_string(&decision.answer)?,
                decision.confidence,
                decision.provider,
                decision.model,
                decision.latency_ms as i64,
                decision.cost_usd,
                decision.cached as i64,
                decision.created_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn assets_for_scan(&self, scan_id: Uuid) -> anyhow::Result<Vec<Asset>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT id, scan_id, url, kind, content_type, status, size, sha256, source, fetched_at
             FROM assets WHERE scan_id = ?1 ORDER BY url",
        )?;
        let rows = stmt.query_map(params![scan_id.to_string()], row_to_asset)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn endpoints_for_scan(&self, scan_id: Uuid) -> anyhow::Result<Vec<Endpoint>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT id, scan_id, method, url, normalized, path_template, params_json, source_asset,
                    source_line, context, role, sensitivity, auth_required, verify_safe, priority,
                    confidence, provider, discovered_at
             FROM endpoints WHERE scan_id = ?1 ORDER BY priority DESC, url",
        )?;
        let rows = stmt.query_map(params![scan_id.to_string()], row_to_endpoint)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn secrets_for_scan(&self, scan_id: Uuid) -> anyhow::Result<Vec<SecretFinding>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT id, scan_id, asset_url, line, column, rule_id, provider, description, severity,
                    masked_value, entropy, is_real, risk, validated, context, discovered_at
             FROM secrets WHERE scan_id = ?1 ORDER BY risk DESC",
        )?;
        let rows = stmt.query_map(params![scan_id.to_string()], row_to_secret)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn verifications_for_scan(&self, scan_id: Uuid) -> anyhow::Result<Vec<Verification>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT id, scan_id, endpoint_id, method, status, content_type, content_length, body_hash,
                    body_snippet, redirects_json, duration_ms, error, classification, checked_at
             FROM verifications WHERE scan_id = ?1",
        )?;
        let rows = stmt.query_map(params![scan_id.to_string()], row_to_verification)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn decisions_for_scan(&self, scan_id: Uuid) -> anyhow::Result<Vec<DecisionRecord>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT id, scan_id, subject_kind, subject_id, question, answer_json, confidence, provider,
                    model, latency_ms, cost_usd, cached, created_at
             FROM decisions WHERE scan_id = ?1 ORDER BY created_at",
        )?;
        let rows = stmt.query_map(params![scan_id.to_string()], row_to_decision)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn get_cached_decision(&self, key: &str) -> anyhow::Result<Option<CachedDecision>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let row = conn
            .query_row(
                "SELECT provider, model, response_json, confidence, cost_usd FROM decision_cache WHERE cache_key = ?1",
                params![key],
                |row| {
                    Ok(CachedDecision {
                        provider: row.get(0)?,
                        model: row.get(1)?,
                        response_json: row.get(2)?,
                        confidence: row.get(3)?,
                        cost_usd: row.get(4)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
    }

    pub fn put_cached_decision(
        &self,
        key: &str,
        provider: &str,
        model: &str,
        response_json: &str,
        confidence: f32,
        cost_usd: f64,
    ) -> anyhow::Result<()> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        conn.execute(
            "INSERT OR REPLACE INTO decision_cache
             (cache_key, provider, model, response_json, confidence, cost_usd, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                key,
                provider,
                model,
                response_json,
                confidence,
                cost_usd,
                Utc::now().to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn count_for_scan(&self, scan_id: Uuid) -> anyhow::Result<ScanSummary> {
        let assets = self.count("assets", scan_id)?;
        let endpoints = self.count("endpoints", scan_id)?;
        let secrets = self.count("secrets", scan_id)?;
        let verified = self.count("verifications", scan_id)?;
        let decisions = self.count("decisions", scan_id)?;
        let high_risk: i64 = {
            let conn = self.conn.lock().expect("store mutex poisoned");
            conn.query_row(
                "SELECT COUNT(*) FROM endpoints WHERE scan_id = ?1 AND priority >= 0.8",
                params![scan_id.to_string()],
                |row| row.get(0),
            )?
        };
        Ok(ScanSummary {
            assets,
            endpoints,
            secrets,
            verified,
            high_risk: high_risk as usize,
            decisions,
            duration_ms: 0,
            cost_usd: 0.0,
        })
    }

    fn count(&self, table: &str, scan_id: Uuid) -> anyhow::Result<usize> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let sql = format!("SELECT COUNT(*) FROM {table} WHERE scan_id = ?1");
        let count: i64 = conn.query_row(&sql, params![scan_id.to_string()], |row| row.get(0))?;
        Ok(count as usize)
    }
}

fn row_to_scan(row: &rusqlite::Row<'_>) -> rusqlite::Result<ScanRecord> {
    let id: String = row.get(0)?;
    let target: String = row.get(1)?;
    let status: String = row.get(2)?;
    let started: String = row.get(3)?;
    let finished: Option<String> = row.get(4)?;
    let summary: String = row.get(5)?;
    Ok(ScanRecord {
        id: Uuid::parse_str(&id).map_err(to_sql_err)?,
        target,
        status: ScanStatus::parse(&status),
        started_at: DateTime::parse_from_rfc3339(&started)
            .map(|d| d.with_timezone(&Utc))
            .map_err(to_sql_err)?,
        finished_at: finished
            .as_deref()
            .map(|s| {
                DateTime::parse_from_rfc3339(s)
                    .map(|d| d.with_timezone(&Utc))
                    .map_err(to_sql_err)
            })
            .transpose()?,
        summary: serde_json::from_str(&summary).map_err(to_sql_err)?,
    })
}

fn row_to_asset(row: &rusqlite::Row<'_>) -> rusqlite::Result<Asset> {
    Ok(Asset {
        id: Uuid::parse_str(&row.get::<_, String>(0)?).map_err(to_sql_err)?,
        scan_id: Uuid::parse_str(&row.get::<_, String>(1)?).map_err(to_sql_err)?,
        url: row.get(2)?,
        kind: AssetKind::parse(&row.get::<_, String>(3)?),
        content_type: row.get(4)?,
        status: row.get::<_, Option<i64>>(5)?.map(|v| v as u16),
        size: row.get::<_, i64>(6)? as usize,
        sha256: row.get(7)?,
        source: row.get(8)?,
        fetched_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(9)?)
            .map(|d| d.with_timezone(&Utc))
            .map_err(to_sql_err)?,
    })
}

fn row_to_endpoint(row: &rusqlite::Row<'_>) -> rusqlite::Result<Endpoint> {
    Ok(Endpoint {
        id: Uuid::parse_str(&row.get::<_, String>(0)?).map_err(to_sql_err)?,
        scan_id: Uuid::parse_str(&row.get::<_, String>(1)?).map_err(to_sql_err)?,
        method: row.get(2)?,
        url: row.get(3)?,
        normalized: row.get(4)?,
        path_template: row.get(5)?,
        params: serde_json::from_str(&row.get::<_, String>(6)?).map_err(to_sql_err)?,
        source_asset: row.get(7)?,
        source_line: row.get::<_, Option<i64>>(8)?.map(|v| v as u32),
        context: row.get(9)?,
        role: row.get(10)?,
        sensitivity: row.get(11)?,
        auth_required: row.get::<_, Option<i64>>(12)?.map(|v| v != 0),
        verify_safe: row.get::<_, Option<i64>>(13)?.map(|v| v != 0),
        priority: row.get(14)?,
        confidence: row.get(15)?,
        provider: row.get(16)?,
        discovered_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(17)?)
            .map(|d| d.with_timezone(&Utc))
            .map_err(to_sql_err)?,
    })
}

fn row_to_secret(row: &rusqlite::Row<'_>) -> rusqlite::Result<SecretFinding> {
    Ok(SecretFinding {
        id: Uuid::parse_str(&row.get::<_, String>(0)?).map_err(to_sql_err)?,
        scan_id: Uuid::parse_str(&row.get::<_, String>(1)?).map_err(to_sql_err)?,
        asset_url: row.get(2)?,
        line: row.get::<_, i64>(3)? as u32,
        column: row.get::<_, i64>(4)? as u32,
        rule_id: row.get(5)?,
        provider: row.get(6)?,
        description: row.get(7)?,
        severity: row.get(8)?,
        masked_value: row.get(9)?,
        entropy: row.get(10)?,
        is_real: row.get::<_, Option<i64>>(11)?.map(|v| v != 0),
        risk: row.get(12)?,
        validated: row.get::<_, Option<i64>>(13)?.map(|v| v != 0),
        context: row.get(14)?,
        discovered_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(15)?)
            .map(|d| d.with_timezone(&Utc))
            .map_err(to_sql_err)?,
    })
}

fn row_to_verification(row: &rusqlite::Row<'_>) -> rusqlite::Result<Verification> {
    Ok(Verification {
        id: Uuid::parse_str(&row.get::<_, String>(0)?).map_err(to_sql_err)?,
        scan_id: Uuid::parse_str(&row.get::<_, String>(1)?).map_err(to_sql_err)?,
        endpoint_id: Uuid::parse_str(&row.get::<_, String>(2)?).map_err(to_sql_err)?,
        method: row.get(3)?,
        status: row.get::<_, Option<i64>>(4)?.map(|v| v as u16),
        content_type: row.get(5)?,
        content_length: row.get::<_, Option<i64>>(6)?.map(|v| v as u64),
        body_hash: row.get(7)?,
        body_snippet: row.get(8)?,
        redirects: serde_json::from_str(&row.get::<_, String>(9)?).map_err(to_sql_err)?,
        duration_ms: row.get::<_, i64>(10)? as u64,
        error: row.get(11)?,
        classification: row.get(12)?,
        checked_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(13)?)
            .map(|d| d.with_timezone(&Utc))
            .map_err(to_sql_err)?,
    })
}

fn row_to_decision(row: &rusqlite::Row<'_>) -> rusqlite::Result<DecisionRecord> {
    Ok(DecisionRecord {
        id: Uuid::parse_str(&row.get::<_, String>(0)?).map_err(to_sql_err)?,
        scan_id: Uuid::parse_str(&row.get::<_, String>(1)?).map_err(to_sql_err)?,
        subject_kind: row.get(2)?,
        subject_id: row.get(3)?,
        question: row.get(4)?,
        answer: serde_json::from_str(&row.get::<_, String>(5)?).map_err(to_sql_err)?,
        confidence: row.get(6)?,
        provider: row.get(7)?,
        model: row.get(8)?,
        latency_ms: row.get::<_, i64>(9)? as u64,
        cost_usd: row.get(10)?,
        cached: row.get::<_, i64>(11)? != 0,
        created_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(12)?)
            .map(|d| d.with_timezone(&Utc))
            .map_err(to_sql_err)?,
    })
}

fn to_sql_err(err: impl std::fmt::Display) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        0,
        rusqlite::types::Type::Text,
        Box::new(std::io::Error::other(err.to_string())),
    )
}

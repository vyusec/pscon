use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use crate::core::{Config, ScanReport, Store};
use anyhow::{Context, Result};
use serde_json::json;
use uuid::Uuid;

pub fn write_reports(store: &Store, scan_id: Uuid, config: &Config) -> Result<PathBuf> {
    let scan = store
        .load_scan(scan_id)?
        .context("scan not found for report")?;
    let assets = store.assets_for_scan(scan_id)?;
    let endpoints = store.endpoints_for_scan(scan_id)?;
    let secrets = store.secrets_for_scan(scan_id)?;
    let verifications = store.verifications_for_scan(scan_id)?;
    let decisions = store.decisions_for_scan(scan_id)?;

    let mut stats = BTreeMap::new();
    stats.insert("assets".into(), assets.len());
    stats.insert("endpoints".into(), endpoints.len());
    stats.insert("secrets".into(), secrets.len());
    stats.insert("verifications".into(), verifications.len());
    stats.insert("decisions".into(), decisions.len());
    stats.insert(
        "high_risk".into(),
        endpoints.iter().filter(|e| e.priority >= 0.8).count(),
    );

    let report = ScanReport {
        schema_version: "1.0".into(),
        scan: scan.clone(),
        assets,
        endpoints,
        secrets,
        verifications,
        decisions,
        stats,
    };

    let dir = config.scans_dir().join(scan_id.to_string());
    std::fs::create_dir_all(&dir)?;
    let json_path = dir.join("report.json");
    std::fs::write(&json_path, serde_json::to_string_pretty(&report)?)?;

    let md = render_markdown(&report);
    let md_path = dir.join("report.md");
    std::fs::write(&md_path, md)?;

    let html = render_html(&report);
    std::fs::write(dir.join("report.html"), html)?;

    let sarif = render_sarif(&report);
    std::fs::write(
        dir.join("report.sarif"),
        serde_json::to_string_pretty(&sarif)?,
    )?;

    Ok(md_path)
}

fn render_markdown(report: &ScanReport) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "# pscon report — {}", report.scan.target);
    let _ = writeln!(out);
    let _ = writeln!(out, "- **Scan ID:** `{}`", report.scan.id);
    let _ = writeln!(out, "- **Status:** {}", report.scan.status.as_str());
    let _ = writeln!(
        out,
        "- **Started:** {}",
        report.scan.started_at.to_rfc3339()
    );
    if let Some(finished) = report.scan.finished_at {
        let _ = writeln!(out, "- **Finished:** {}", finished.to_rfc3339());
    }
    let _ = writeln!(
        out,
        "- **Duration:** {} ms",
        report.scan.summary.duration_ms
    );
    let _ = writeln!(out);
    let _ = writeln!(out, "## Summary");
    let _ = writeln!(out);
    let _ = writeln!(out, "| Metric | Count |");
    let _ = writeln!(out, "|---|---:|");
    for (key, value) in &report.stats {
        let _ = writeln!(out, "| {key} | {value} |");
    }
    let _ = writeln!(out);

    let _ = writeln!(out, "## High-priority endpoints");
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "| Priority | Method | Endpoint | Role | Sensitivity | Auth | Status |"
    );
    let _ = writeln!(out, "|---:|---|---|---|---|---|---|");
    for ep in report
        .endpoints
        .iter()
        .filter(|e| e.priority >= 0.5)
        .take(100)
    {
        let verification = report
            .verifications
            .iter()
            .find(|v| v.endpoint_id == ep.id)
            .map(|v| format!("{} {}", v.status.unwrap_or(0), v.classification))
            .unwrap_or_else(|| "-".into());
        let _ = writeln!(
            out,
            "| {:.2} | {} | `{}` | {} | {:.2} | {} | {} |",
            ep.priority,
            ep.method,
            escape_md(&ep.path_template),
            ep.role,
            ep.sensitivity,
            ep.auth_required
                .map(|v| if v { "yes" } else { "no" })
                .unwrap_or("?"),
            verification
        );
    }
    let _ = writeln!(out);

    let _ = writeln!(out, "## Secrets");
    let _ = writeln!(out);
    if report.secrets.is_empty() {
        let _ = writeln!(out, "No secret candidates detected.");
    } else {
        let _ = writeln!(out, "| Risk | Rule | Provider | Masked value | Location |");
        let _ = writeln!(out, "|---:|---|---|---|---|");
        for secret in report.secrets.iter().take(100) {
            let _ = writeln!(
                out,
                "| {:.2} | {} | {} | `{}` | {}:{} |",
                secret.risk,
                secret.rule_id,
                secret.provider,
                escape_md(&secret.masked_value),
                escape_md(&secret.asset_url),
                secret.line
            );
        }
    }
    let _ = writeln!(out);

    let _ = writeln!(out, "## Decision models");
    let _ = writeln!(out);
    let mut providers: BTreeMap<String, usize> = BTreeMap::new();
    for decision in &report.decisions {
        *providers.entry(decision.provider.clone()).or_default() += 1;
    }
    if providers.is_empty() {
        let _ = writeln!(out, "No decision-model calls were made (heuristics only).");
    } else {
        for (provider, count) in providers {
            let _ = writeln!(out, "- **{provider}:** {count} decisions");
        }
    }
    let _ = writeln!(out);

    let _ = writeln!(out, "## Verification");
    let _ = writeln!(out);
    let mut classes: BTreeMap<String, usize> = BTreeMap::new();
    for verification in &report.verifications {
        *classes
            .entry(verification.classification.clone())
            .or_default() += 1;
    }
    if classes.is_empty() {
        let _ = writeln!(out, "No active verification was performed.");
    } else {
        for (class, count) in classes {
            let _ = writeln!(out, "- **{class}:** {count}");
        }
    }
    let _ = writeln!(out);

    let _ = writeln!(out, "## Assets");
    let _ = writeln!(out);
    let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
    for asset in &report.assets {
        *kinds.entry(asset.kind.as_str().to_string()).or_default() += 1;
    }
    for (kind, count) in kinds {
        let _ = writeln!(out, "- **{kind}:** {count}");
    }
    let _ = writeln!(out);

    let _ = writeln!(out, "---");
    let _ = writeln!(
        out,
        "Generated by pscon. Secret values are masked. Only test systems you are authorized to test."
    );
    out
}

fn render_html(report: &ScanReport) -> String {
    let mut out = String::new();
    out.push_str("<!doctype html><html><head><meta charset=\"utf-8\"><title>pscon report</title>");
    out.push_str("<style>body{font-family:system-ui,sans-serif;max-width:1200px;margin:2rem auto;padding:0 1rem;background:#0d1117;color:#c9d1d9}table{border-collapse:collapse;width:100%;margin:1rem 0}th,td{border:1px solid #30363d;padding:.4rem .6rem;text-align:left}th{background:#161b22}code{color:#7ee787}.high{color:#ff7b72}.med{color:#d29922}.low{color:#8b949e}</style></head><body>");
    out.push_str(&format!(
        "<h1>pscon report — {}</h1>",
        html_escape(&report.scan.target)
    ));
    out.push_str(&format!(
        "<p>Scan <code>{}</code> · status <b>{}</b> · {} assets · {} endpoints · {} secrets</p>",
        report.scan.id,
        report.scan.status.as_str(),
        report.stats.get("assets").copied().unwrap_or(0),
        report.stats.get("endpoints").copied().unwrap_or(0),
        report.stats.get("secrets").copied().unwrap_or(0),
    ));
    out.push_str("<h2>Endpoints</h2><table><tr><th>Priority</th><th>Method</th><th>Endpoint</th><th>Role</th><th>Sensitivity</th></tr>");
    for ep in report.endpoints.iter().take(500) {
        let class = if ep.priority >= 0.8 {
            "high"
        } else if ep.priority >= 0.5 {
            "med"
        } else {
            "low"
        };
        out.push_str(&format!(
            "<tr><td class=\"{class}\">{:.2}</td><td>{}</td><td><code>{}</code></td><td>{}</td><td>{:.2}</td></tr>",
            ep.priority,
            html_escape(&ep.method),
            html_escape(&ep.path_template),
            html_escape(&ep.role),
            ep.sensitivity
        ));
    }
    out.push_str("</table><h2>Secrets</h2><table><tr><th>Risk</th><th>Rule</th><th>Masked value</th><th>Location</th></tr>");
    for secret in &report.secrets {
        out.push_str(&format!(
            "<tr><td>{:.2}</td><td>{}</td><td><code>{}</code></td><td>{}:{}</td></tr>",
            secret.risk,
            html_escape(&secret.rule_id),
            html_escape(&secret.masked_value),
            html_escape(&secret.asset_url),
            secret.line
        ));
    }
    out.push_str("</table><p>Generated by pscon. Secret values are masked.</p></body></html>");
    out
}

fn render_sarif(report: &ScanReport) -> serde_json::Value {
    let mut results = Vec::new();
    for ep in report.endpoints.iter().filter(|e| e.priority >= 0.8) {
        results.push(json!({
            "ruleId": "pscon/high-priority-endpoint",
            "level": "warning",
            "message": { "text": format!("High-priority endpoint: {} {} ({})", ep.method, ep.path_template, ep.role) },
            "locations": [{ "physicalLocation": { "artifactLocation": { "uri": ep.url } } }],
        }));
    }
    for secret in &report.secrets {
        results.push(json!({
            "ruleId": format!("pscon/secret/{}", secret.rule_id),
            "level": if secret.risk >= 0.8 { "error" } else { "warning" },
            "message": { "text": format!("Potential secret: {} in {}", secret.description, secret.asset_url) },
            "locations": [{ "physicalLocation": {
                "artifactLocation": { "uri": secret.asset_url },
                "region": { "startLine": secret.line, "startColumn": secret.column }
            }}],
        }));
    }
    json!({
        "version": "2.1.0",
        "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
        "runs": [{
            "tool": { "driver": { "name": "pscon", "informationUri": "https://github.com/vyusec/pscon" } },
            "results": results,
        }]
    })
}

fn escape_md(value: &str) -> String {
    value.replace('|', "\\|").replace('\n', " ")
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

pub fn report_path(config: &Config, scan_id: Uuid) -> PathBuf {
    config
        .scans_dir()
        .join(scan_id.to_string())
        .join("report.md")
}

pub fn ensure_report_dir(config: &Config, scan_id: Uuid) -> Result<PathBuf> {
    let dir = config.scans_dir().join(scan_id.to_string());
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

pub fn write_text(path: &Path, content: &str) -> Result<()> {
    std::fs::write(path, content)?;
    Ok(())
}

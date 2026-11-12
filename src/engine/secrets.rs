use std::collections::HashMap;

use once_cell::sync::Lazy;
use regex::Regex;

#[derive(Debug, Clone)]
pub struct SecretCandidate {
    pub line: u32,
    pub column: u32,
    pub rule_id: String,
    pub provider: String,
    pub description: String,
    pub severity: String,
    pub masked_value: String,
    pub entropy: f32,
    pub context: String,
}

struct SecretRule {
    id: &'static str,
    provider: &'static str,
    description: &'static str,
    severity: &'static str,
    regex: Regex,
    group: usize,
    require_entropy: bool,
}

static RULES: Lazy<Vec<SecretRule>> = Lazy::new(|| {
    let mut rules = Vec::new();
    let mut add = |id, provider, description, severity, pattern: &str, group, require_entropy| {
        if let Ok(regex) = Regex::new(pattern) {
            rules.push(SecretRule {
                id,
                provider,
                description,
                severity,
                regex,
                group,
                require_entropy,
            });
        }
    };

    add(
        "aws_access_key",
        "aws",
        "AWS access key ID",
        "high",
        r"\b(AKIA[0-9A-Z]{16})\b",
        1,
        false,
    );
    add(
        "aws_secret_key",
        "aws",
        "AWS secret access key",
        "critical",
        r#"(?i)aws_secret_access_key\s*[:=]\s*['"]?([A-Za-z0-9/+=]{40})"#,
        1,
        true,
    );
    add(
        "github_pat",
        "github",
        "GitHub personal access token",
        "critical",
        r"\b(gh[pousr]_[A-Za-z0-9]{36,255})\b",
        1,
        false,
    );
    add(
        "github_fine_grained",
        "github",
        "GitHub fine-grained token",
        "critical",
        r"\b(github_pat_[A-Za-z0-9_]{20,255})\b",
        1,
        false,
    );
    add(
        "slack_token",
        "slack",
        "Slack token",
        "high",
        r"\b(xox[baprs]-[A-Za-z0-9-]{10,255})\b",
        1,
        false,
    );
    add(
        "stripe_secret",
        "stripe",
        "Stripe secret key",
        "critical",
        r"\b((?:sk|rk)_live_[A-Za-z0-9]{16,255})\b",
        1,
        false,
    );
    add(
        "google_api_key",
        "google",
        "Google API key",
        "high",
        r"\b(AIza[0-9A-Za-z_-]{35})\b",
        1,
        false,
    );
    add(
        "openai_key",
        "openai",
        "OpenAI API key",
        "critical",
        r"\b(sk-[A-Za-z0-9]{20,255})\b",
        1,
        false,
    );
    add(
        "sendgrid_key",
        "sendgrid",
        "SendGrid API key",
        "high",
        r"\b(SG\.[A-Za-z0-9_-]{22}\.[A-Za-z0-9_-]{43})\b",
        1,
        false,
    );
    add(
        "twilio_key",
        "twilio",
        "Twilio API key",
        "high",
        r"\b(SK[0-9a-fA-F]{32})\b",
        1,
        false,
    );
    add(
        "npm_token",
        "npm",
        "npm access token",
        "high",
        r"\b(npm_[A-Za-z0-9]{36})\b",
        1,
        false,
    );
    add(
        "heroku_key",
        "heroku",
        "Heroku API key",
        "high",
        r"\b(heroku_[A-Za-z0-9]{32,})\b",
        1,
        false,
    );
    add(
        "supabase_service_key",
        "supabase",
        "Supabase service role key",
        "critical",
        r"\b(sbp_[A-Za-z0-9]{40,})\b",
        1,
        false,
    );
    add(
        "jwt",
        "generic",
        "JSON Web Token",
        "medium",
        r"\b(eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,})\b",
        1,
        false,
    );
    add(
        "private_key",
        "generic",
        "Private key block",
        "critical",
        r"-----BEGIN (?:RSA |EC |OPENSSH |DSA |PGP )?PRIVATE KEY-----",
        0,
        false,
    );
    add(
        "database_url",
        "generic",
        "Database connection string with credentials",
        "critical",
        r#"(?i)\b((?:postgres(?:ql)?|mysql|mongodb(?:\+srv)?|redis|amqp)://[^\s"'<>]{8,})"#,
        1,
        true,
    );
    add(
        "bearer_token",
        "generic",
        "Bearer token",
        "medium",
        r#"(?i)\bbearer\s+([A-Za-z0-9\-._~+/]{20,}=*)"#,
        1,
        true,
    );
    add(
        "generic_secret_assignment",
        "generic",
        "High-entropy secret assignment",
        "medium",
        r#"(?i)(?:api[_-]?key|secret|token|password|passwd|authorization)\s*[:=]\s*['"]([A-Za-z0-9_\-./+=]{16,})['"]"#,
        1,
        true,
    );
    add(
        "discord_webhook",
        "discord",
        "Discord webhook URL",
        "high",
        r"(https://(?:canary\.|ptb\.)?discord(?:app)?\.com/api/webhooks/[0-9]+/[A-Za-z0-9_-]+)",
        1,
        false,
    );
    rules
});

pub fn scan(source: &str, _asset_url: &str) -> Vec<SecretCandidate> {
    let mut out = Vec::new();
    let mut seen: HashMap<(String, String), bool> = HashMap::new();
    for rule in RULES.iter() {
        for cap in rule.regex.captures_iter(source) {
            let Some(m) = cap.get(rule.group) else {
                continue;
            };
            let value = m.as_str();
            if value.len() < 8 {
                continue;
            }
            let entropy = shannon_entropy(value);
            if rule.require_entropy && entropy < 3.0 {
                continue;
            }
            let masked = mask(value);
            let key = (rule.id.to_string(), masked.clone());
            if seen.insert(key, true).is_some() {
                continue;
            }
            let line = source[..m.start()].bytes().filter(|b| *b == b'\n').count() as u32 + 1;
            let line_start = source[..m.start()].rfind('\n').map(|i| i + 1).unwrap_or(0);
            let column = (m.start() - line_start) as u32 + 1;
            let ctx_start = m.start().saturating_sub(60);
            let ctx_end = (m.end() + 60).min(source.len());
            let context = source
                .get(ctx_start..ctx_end)
                .unwrap_or_default()
                .replace('\n', " ")
                .trim()
                .to_string();
            out.push(SecretCandidate {
                line,
                column,
                rule_id: rule.id.to_string(),
                provider: rule.provider.to_string(),
                description: rule.description.to_string(),
                severity: rule.severity.to_string(),
                masked_value: masked,
                entropy,
                context,
            });
        }
    }
    out.sort_by(|a, b| {
        severity_rank(&b.severity)
            .cmp(&severity_rank(&a.severity))
            .then_with(|| b.entropy.total_cmp(&a.entropy))
    });
    out
}

pub fn shannon_entropy(value: &str) -> f32 {
    if value.is_empty() {
        return 0.0;
    }
    let mut counts = [0usize; 256];
    for byte in value.bytes() {
        counts[byte as usize] += 1;
    }
    let len = value.len() as f32;
    let mut entropy = 0.0;
    for count in counts {
        if count == 0 {
            continue;
        }
        let p = count as f32 / len;
        entropy -= p * p.log2();
    }
    entropy
}

fn mask(value: &str) -> String {
    if value.len() <= 8 {
        return "*".repeat(value.len());
    }
    let prefix: String = value.chars().take(4).collect();
    let suffix: String = value
        .chars()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("{prefix}…{suffix}")
}

fn severity_rank(severity: &str) -> u8 {
    match severity {
        "critical" => 4,
        "high" => 3,
        "medium" => 2,
        "low" => 1,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_and_masks_aws_key() {
        let findings = scan(
            "const key = 'AKIAIOSFODNN7EXAMPLE';",
            "https://example.com/app.js",
        );
        assert!(findings.iter().any(|f| f.rule_id == "aws_access_key"));
        let finding = findings
            .iter()
            .find(|f| f.rule_id == "aws_access_key")
            .unwrap();
        assert!(finding.masked_value.starts_with("AKIA"));
        assert!(!finding.masked_value.contains("IOSFODNN7"));
    }

    #[test]
    fn entropy_is_reasonable() {
        assert!(shannon_entropy("aaaaaaaa") < 1.0);
        assert!(shannon_entropy("aB3$xY9!qW2") > 2.5);
    }
}

use std::path::Path;

use serde_json::json;

use crate::core::config::NotifyConfig;
use crate::core::model::ScanSummary;

#[derive(Debug, Clone)]
pub struct Notifier {
    config: NotifyConfig,
}

impl Notifier {
    pub fn new(config: NotifyConfig) -> Self {
        Self { config }
    }

    pub fn wants(&self, event: &str) -> bool {
        self.config.on.iter().any(|e| e == event)
    }

    pub async fn notify_scan(
        &self,
        target: &str,
        summary: &ScanSummary,
        report_path: Option<&Path>,
        event: &str,
    ) -> Vec<String> {
        if !self.wants(event) {
            return Vec::new();
        }
        let title = match event {
            "failed" => format!("pscon scan failed: {target}"),
            _ => format!("pscon scan complete: {target}"),
        };
        let mut body = format!(
            "{} assets · {} endpoints · {} secrets · {} high risk · {} decisions",
            summary.assets,
            summary.endpoints,
            summary.secrets,
            summary.high_risk,
            summary.decisions
        );
        if let Some(path) = report_path {
            body.push_str(&format!("\nReport: {}", path.display()));
        }
        self.send(&title, &body).await
    }

    pub async fn send(&self, title: &str, body: &str) -> Vec<String> {
        let mut errors = Vec::new();

        if self.config.desktop {
            let title = title.to_string();
            let body = body.to_string();
            let result = tokio::task::spawn_blocking(move || {
                notify_rust::Notification::new()
                    .summary(&title)
                    .body(&body)
                    .appname("pscon")
                    .timeout(notify_rust::Timeout::Milliseconds(8000))
                    .show()
            })
            .await;
            match result {
                Ok(Ok(_)) => {}
                Ok(Err(e)) => errors.push(format!("desktop: {e}")),
                Err(e) => errors.push(format!("desktop task: {e}")),
            }
        }

        if !self.config.ntfy_topic.is_empty() {
            let url = format!(
                "{}/{}",
                self.config.ntfy_server.trim_end_matches('/'),
                self.config.ntfy_topic.trim_start_matches('/')
            );
            let mut req = reqwest::Client::new()
                .post(&url)
                .header("Title", title)
                .header("Priority", "default")
                .header("Tags", "mag");
            if let Ok(token) = std::env::var(&self.config.ntfy_token_env) {
                req = req.bearer_auth(token);
            }
            match req.body(body.to_string()).send().await {
                Ok(resp) if resp.status().is_success() => {}
                Ok(resp) => errors.push(format!("ntfy: HTTP {}", resp.status())),
                Err(e) => errors.push(format!("ntfy: {e}")),
            }
        }

        if !self.config.webhook_url.is_empty()
            && let Err(e) = self.send_webhook(title, body).await
        {
            errors.push(format!("webhook: {e}"));
        }

        errors
    }

    async fn send_webhook(&self, title: &str, body: &str) -> anyhow::Result<()> {
        let kind = self.config.webhook_kind.to_ascii_lowercase();
        let payload = match kind.as_str() {
            "discord" => json!({
                "content": format!("**{title}**\n{body}")
            }),
            "slack" => json!({
                "text": format!("*{title}*\n{body}")
            }),
            "telegram" => {
                let token = std::env::var(&self.config.telegram_bot_token_env)?;
                let chat = &self.config.telegram_chat_id;
                let url = format!("https://api.telegram.org/bot{token}/sendMessage");
                let resp = reqwest::Client::new()
                    .post(url)
                    .json(&json!({
                        "chat_id": chat,
                        "text": format!("{title}\n{body}")
                    }))
                    .send()
                    .await?;
                if !resp.status().is_success() {
                    anyhow::bail!("telegram HTTP {}", resp.status());
                }
                return Ok(());
            }
            _ => json!({
                "tool": "pscon",
                "title": title,
                "body": body,
                "timestamp": chrono::Utc::now().to_rfc3339(),
            }),
        };
        let resp = reqwest::Client::new()
            .post(&self.config.webhook_url)
            .json(&payload)
            .send()
            .await?;
        if !resp.status().is_success() {
            anyhow::bail!("HTTP {}", resp.status());
        }
        Ok(())
    }

    pub async fn test(&self) -> Vec<String> {
        self.send(
            "pscon notification test",
            "If you can read this, notifications are configured.",
        )
        .await
    }
}

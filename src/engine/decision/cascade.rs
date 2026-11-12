use std::sync::Arc;

use crate::core::config::{DecisionsConfig, LayaConfig};
use crate::core::{DecisionRecord, Store};
use anyhow::{Context, Result};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::sync::Mutex;
use tracing::{debug, warn};
use uuid::Uuid;

use super::answer::DecisionResponse;
use super::laya;
use super::packs::{EndpointTriage, SecretTriage, endpoint_triage, secret_triage};
use super::provider::{DecisionProvider, JevProvider, LayaProvider, LunaProvider};
use super::question::QuestionSet;

pub struct DecisionEngine {
    config: DecisionsConfig,
    providers: Vec<Arc<dyn DecisionProvider>>,
    store: Store,
    budget: Mutex<BudgetState>,
}

#[derive(Debug, Default)]
struct BudgetState {
    calls: usize,
    cost_usd: f64,
}

impl DecisionEngine {
    pub async fn new(config: DecisionsConfig, store: Store) -> Result<Self> {
        let mut providers: Vec<Arc<dyn DecisionProvider>> = Vec::new();
        let mut by_name: std::collections::HashMap<&str, Arc<dyn DecisionProvider>> =
            std::collections::HashMap::new();

        if config.enabled {
            let laya_ready = match laya::ensure_running(&config.laya).await {
                Ok(()) => true,
                Err(err) => {
                    warn!("Laya unavailable: {err}");
                    false
                }
            };
            if laya_ready {
                let laya_key = std::env::var(&config.laya.api_key_env).ok();
                let laya: Arc<dyn DecisionProvider> =
                    Arc::new(LayaProvider::new(&config.laya.base_url, laya_key));
                by_name.insert("laya", laya);
            }

            if let Ok(key) = std::env::var(&config.jev.api_key_env)
                && !key.trim().is_empty()
            {
                let jev: Arc<dyn DecisionProvider> = Arc::new(JevProvider::new(
                    &config.jev.base_url,
                    key,
                    &config.jev.model,
                ));
                by_name.insert("jev", jev);
            }
            if let Ok(key) = std::env::var(&config.luna.api_key_env)
                && !key.trim().is_empty()
            {
                let luna: Arc<dyn DecisionProvider> = Arc::new(LunaProvider::new(
                    &config.luna.base_url,
                    key,
                    &config.luna.model,
                ));
                by_name.insert("luna", luna);
            }

            for name in &config.provider_order {
                if let Some(provider) = by_name.remove(name.as_str()) {
                    providers.push(provider);
                }
            }
        }

        Ok(Self {
            config,
            providers,
            store,
            budget: Mutex::new(BudgetState::default()),
        })
    }

    pub fn provider_names(&self) -> Vec<&str> {
        self.providers.iter().map(|p| p.id()).collect()
    }

    pub async fn triage_endpoint(
        &self,
        scan_id: Uuid,
        endpoint_id: &str,
        state: Value,
    ) -> Option<EndpointTriage> {
        let response = self
            .decide(scan_id, "endpoint", endpoint_id, state, &endpoint_triage())
            .await?;
        Some(EndpointTriage::from_response(&response))
    }

    pub async fn triage_secret(
        &self,
        scan_id: Uuid,
        secret_id: &str,
        state: Value,
    ) -> Option<SecretTriage> {
        let response = self
            .decide(scan_id, "secret", secret_id, state, &secret_triage())
            .await?;
        Some(SecretTriage::from_response(&response))
    }

    pub async fn decide(
        &self,
        scan_id: Uuid,
        subject_kind: &str,
        subject_id: &str,
        state: Value,
        questions: &QuestionSet,
    ) -> Option<DecisionResponse> {
        if !self.config.enabled || self.providers.is_empty() {
            return None;
        }
        let key = cache_key(&state, questions);
        let mut best: Option<DecisionResponse> = None;

        for provider in &self.providers {
            let provider_key = format!("{}:{}", provider.id(), key);
            if let Ok(Some(cached)) = self.store.get_cached_decision(&provider_key)
                && let Ok(mut response) =
                    serde_json::from_str::<DecisionResponse>(&cached.response_json)
            {
                response.cached = true;
                if response.min_confidence() >= self.config.confidence_threshold {
                    self.persist(scan_id, subject_kind, subject_id, &response)
                        .await;
                    return Some(response);
                }
                if best
                    .as_ref()
                    .map(|b| response.min_confidence() > b.min_confidence())
                    .unwrap_or(true)
                {
                    best = Some(response);
                }
            }

            if !self.can_call().await {
                warn!("decision budget exhausted");
                break;
            }

            debug!(
                provider = provider.id(),
                subject = subject_id,
                "calling decision provider"
            );
            match provider.decide(state.clone(), questions).await {
                Ok(response) => {
                    self.record_call(response.cost_usd).await;
                    if let Ok(json) = serde_json::to_string(&response) {
                        let _ = self.store.put_cached_decision(
                            &provider_key,
                            &response.provider,
                            &response.model,
                            &json,
                            response.min_confidence(),
                            response.cost_usd,
                        );
                    }
                    self.persist(scan_id, subject_kind, subject_id, &response)
                        .await;
                    let confident = response.min_confidence() >= self.config.confidence_threshold;
                    if best
                        .as_ref()
                        .map(|b| response.min_confidence() > b.min_confidence())
                        .unwrap_or(true)
                    {
                        best = Some(response);
                    }
                    if confident {
                        break;
                    }
                }
                Err(err) => {
                    warn!(
                        provider = provider.id(),
                        "decision provider failed: {err:#}"
                    );
                }
            }
        }
        best
    }

    async fn persist(
        &self,
        scan_id: Uuid,
        subject_kind: &str,
        subject_id: &str,
        response: &DecisionResponse,
    ) {
        let record = DecisionRecord {
            id: Uuid::new_v4(),
            scan_id,
            subject_kind: subject_kind.to_string(),
            subject_id: subject_id.to_string(),
            question: response
                .answers
                .keys()
                .cloned()
                .collect::<Vec<_>>()
                .join(","),
            answer: serde_json::to_value(&response.answers).unwrap_or(Value::Null),
            confidence: response.min_confidence(),
            provider: response.provider.clone(),
            model: response.model.clone(),
            latency_ms: response.latency_ms,
            cost_usd: response.cost_usd,
            cached: response.cached,
            created_at: chrono::Utc::now(),
        };
        if let Err(err) = self.store.insert_decision(&record) {
            warn!("failed to persist decision: {err}");
        }
    }

    async fn can_call(&self) -> bool {
        let budget = self.budget.lock().await;
        budget.calls < self.config.max_calls && budget.cost_usd < self.config.max_cost_usd
    }

    async fn record_call(&self, cost: f64) {
        let mut budget = self.budget.lock().await;
        budget.calls += 1;
        budget.cost_usd += cost;
    }
}

pub fn cache_key(state: &Value, questions: &QuestionSet) -> String {
    let mut hasher = Sha256::new();
    hasher.update(serde_json::to_vec(state).unwrap_or_default());
    hasher.update(serde_json::to_vec(questions).unwrap_or_default());
    hex::encode(hasher.finalize())
}

pub async fn setup_laya(config: &LayaConfig) -> Result<()> {
    let venv = &config.venv;
    if !venv.exists() {
        let python = &config.python;
        let status = tokio::process::Command::new(python)
            .arg("-m")
            .arg("venv")
            .arg(venv)
            .status()
            .await
            .with_context(|| format!("creating venv with {python}"))?;
        if !status.success() {
            anyhow::bail!("failed to create Laya venv");
        }
    }
    let pip = venv.join("bin").join("pip");
    let status = tokio::process::Command::new(pip)
        .args(["install", "-U", "laya[serve]"])
        .status()
        .await
        .context("installing laya[serve]")?;
    if !status.success() {
        anyhow::bail!("failed to install laya[serve]");
    }
    Ok(())
}

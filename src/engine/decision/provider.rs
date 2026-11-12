use std::collections::BTreeMap;
use std::time::Instant;

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use serde_json::{Value, json};

use super::answer::{Answer, DecisionResponse};
use super::question::{Question, QuestionSet};

#[async_trait]
pub trait DecisionProvider: Send + Sync {
    fn id(&self) -> &str;
    fn model(&self) -> &str;
    async fn decide(&self, state: Value, questions: &QuestionSet) -> Result<DecisionResponse>;
}

pub struct LayaProvider {
    base_url: String,
    api_key: Option<String>,
    client: reqwest::Client,
}

impl LayaProvider {
    pub fn new(base_url: impl Into<String>, api_key: Option<String>) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            api_key,
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl DecisionProvider for LayaProvider {
    fn id(&self) -> &str {
        "laya"
    }

    fn model(&self) -> &str {
        "laya"
    }

    async fn decide(&self, state: Value, questions: &QuestionSet) -> Result<DecisionResponse> {
        let body = json!({
            "state": state,
            "questions": questions_to_jev(questions),
        });
        let mut req = self
            .client
            .post(format!("{}/v1/systemone", self.base_url))
            .json(&body);
        if let Some(key) = &self.api_key {
            req = req.bearer_auth(key);
        }
        let started = Instant::now();
        let resp = req.send().await.context("calling Laya")?;
        let status = resp.status();
        let text = resp.text().await.context("reading Laya response")?;
        if !status.is_success() {
            bail!("Laya HTTP {status}: {}", truncate(&text, 300));
        }
        let value: Value = serde_json::from_str(&text).context("parsing Laya response")?;
        let answers = parse_jev_answers(&value)?;
        let cost = value
            .pointer("/usage/cost_usd")
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        Ok(DecisionResponse {
            provider: "laya".into(),
            model: value
                .get("model")
                .and_then(Value::as_str)
                .unwrap_or("laya")
                .to_string(),
            answers,
            latency_ms: started.elapsed().as_millis() as u64,
            cost_usd: cost,
            cached: false,
            usage: value.get("usage").cloned().unwrap_or(Value::Null),
        })
    }
}

pub struct JevProvider {
    base_url: String,
    api_key: String,
    model: String,
    client: reqwest::Client,
}

impl JevProvider {
    pub fn new(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        model: impl Into<String>,
    ) -> Self {
        Self {
            base_url: base_url.into(),
            api_key: api_key.into(),
            model: model.into(),
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl DecisionProvider for JevProvider {
    fn id(&self) -> &str {
        "jev"
    }

    fn model(&self) -> &str {
        &self.model
    }

    async fn decide(&self, state: Value, questions: &QuestionSet) -> Result<DecisionResponse> {
        let body = json!({
            "model": self.model,
            "state": state,
            "questions": questions_to_jev(questions),
        });
        let started = Instant::now();
        let resp = self
            .client
            .post(&self.base_url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .context("calling Jev")?;
        let status = resp.status();
        let text = resp.text().await.context("reading Jev response")?;
        if !status.is_success() {
            bail!("Jev HTTP {status}: {}", truncate(&text, 300));
        }
        let value: Value = serde_json::from_str(&text).context("parsing Jev response")?;
        let answers = parse_jev_answers(&value)?;
        let cost = value
            .pointer("/usage/cost_usd")
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        Ok(DecisionResponse {
            provider: "jev".into(),
            model: value
                .get("model")
                .and_then(Value::as_str)
                .unwrap_or(&self.model)
                .to_string(),
            answers,
            latency_ms: started.elapsed().as_millis() as u64,
            cost_usd: cost,
            cached: false,
            usage: value.get("usage").cloned().unwrap_or(Value::Null),
        })
    }
}

pub struct LunaProvider {
    base_url: String,
    api_key: String,
    model: String,
    client: reqwest::Client,
}

impl LunaProvider {
    pub fn new(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        model: impl Into<String>,
    ) -> Self {
        Self {
            base_url: base_url.into(),
            api_key: api_key.into(),
            model: model.into(),
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl DecisionProvider for LunaProvider {
    fn id(&self) -> &str {
        "luna"
    }

    fn model(&self) -> &str {
        &self.model
    }

    async fn decide(&self, state: Value, questions: &QuestionSet) -> Result<DecisionResponse> {
        let body = json!({
            "model": self.model,
            "input": state,
            "questions": questions_to_luna(questions),
        });
        let started = Instant::now();
        let resp = self
            .client
            .post(&self.base_url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .context("calling GPT-6 Luna Decisions")?;
        let status = resp.status();
        let text = resp.text().await.context("reading Luna response")?;
        if !status.is_success() {
            bail!("Luna HTTP {status}: {}", truncate(&text, 300));
        }
        let value: Value = serde_json::from_str(&text).context("parsing Luna response")?;
        let answers = parse_luna_answers(&value)?;
        let cost = value
            .pointer("/usage/cost_usd")
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        Ok(DecisionResponse {
            provider: "luna".into(),
            model: value
                .get("model")
                .and_then(Value::as_str)
                .unwrap_or(&self.model)
                .to_string(),
            answers,
            latency_ms: started.elapsed().as_millis() as u64,
            cost_usd: cost,
            cached: false,
            usage: value.get("usage").cloned().unwrap_or(Value::Null),
        })
    }
}

fn questions_to_jev(questions: &QuestionSet) -> Value {
    let mut map = serde_json::Map::new();
    for (name, question) in questions {
        let value = match question {
            Question::Choice {
                instructions,
                criteria,
            } => {
                let criteria = criteria
                    .iter()
                    .map(|c| (c.value.clone(), Value::String(c.description.clone())))
                    .collect::<serde_json::Map<_, _>>();
                json!({
                    "type": "choice",
                    "instructions": instructions,
                    "criteria": criteria,
                })
            }
            Question::Score {
                instructions,
                criteria,
            } => json!({
                "type": "score",
                "instructions": instructions,
                "criteria": criteria,
            }),
            Question::Noul { instructions } => json!({
                "type": "noul",
                "instructions": instructions,
            }),
        };
        map.insert(name.clone(), value);
    }
    Value::Object(map)
}

fn questions_to_luna(questions: &QuestionSet) -> Value {
    let mut out = Vec::new();
    for (name, question) in questions {
        let value = match question {
            Question::Choice {
                instructions,
                criteria,
            } => json!({
                "type": "choice",
                "name": name,
                "instructions": instructions,
                "choices": criteria.iter().map(|c| json!({
                    "value": c.value,
                    "description": c.description,
                })).collect::<Vec<_>>(),
            }),
            Question::Score {
                instructions,
                criteria,
            } => json!({
                "type": "score",
                "name": name,
                "instructions": instructions,
                "levels": criteria.iter().enumerate().map(|(i, label)| json!({
                    "label": label,
                    "description": format!("level {i}: {label}"),
                })).collect::<Vec<_>>(),
            }),
            Question::Noul { instructions } => json!({
                "type": "predicate",
                "name": name,
                "instructions": instructions,
            }),
        };
        out.push(value);
    }
    Value::Array(out)
}

fn parse_jev_answers(value: &Value) -> Result<BTreeMap<String, Answer>> {
    let answers = value
        .get("answers")
        .and_then(Value::as_object)
        .context("response has no answers object")?;
    let mut out = BTreeMap::new();
    for (name, raw) in answers {
        let kind = raw.get("type").and_then(Value::as_str).unwrap_or("");
        let answer = match kind {
            "choice" => {
                let choice = raw
                    .get("choice")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let confidence = raw
                    .get("confidence")
                    .or_else(|| raw.get("answer_confidence"))
                    .and_then(Value::as_f64)
                    .unwrap_or(0.0) as f32;
                Answer::Choice {
                    choice,
                    confidence,
                    probabilities: probability_map(raw.get("probabilities")),
                }
            }
            "score" => {
                let score = raw.get("score").and_then(Value::as_f64).unwrap_or(0.0) as f32;
                let confidence = raw
                    .get("confidence")
                    .or_else(|| raw.get("answer_confidence"))
                    .and_then(Value::as_f64)
                    .unwrap_or(0.0) as f32;
                Answer::Score {
                    score,
                    confidence,
                    probabilities: probability_map(raw.get("probabilities")),
                }
            }
            "noul" | "predicate" => {
                let probability = raw
                    .get("noul")
                    .or_else(|| raw.get("probability"))
                    .and_then(Value::as_f64)
                    .unwrap_or(0.0) as f32;
                let confidence = raw
                    .get("confidence")
                    .and_then(Value::as_f64)
                    .map(|v| v as f32)
                    .unwrap_or_else(|| probability.max(1.0 - probability));
                Answer::Noul {
                    probability,
                    confidence,
                }
            }
            "refusal" => Answer::Refusal {
                reason: raw
                    .get("reason")
                    .and_then(Value::as_str)
                    .map(str::to_string),
            },
            _ => continue,
        };
        out.insert(name.clone(), answer);
    }
    Ok(out)
}

fn parse_luna_answers(value: &Value) -> Result<BTreeMap<String, Answer>> {
    let answers = value
        .get("answers")
        .and_then(Value::as_array)
        .context("response has no answers array")?;
    let mut out = BTreeMap::new();
    for raw in answers {
        let name = raw
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        if name.is_empty() {
            continue;
        }
        let kind = raw.get("type").and_then(Value::as_str).unwrap_or("");
        let answer = match kind {
            "choice" => {
                let choice = raw
                    .get("choice")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let confidence =
                    raw.get("confidence").and_then(Value::as_f64).unwrap_or(0.0) as f32;
                Answer::Choice {
                    choice,
                    confidence,
                    probabilities: probability_array(raw.get("probabilities")),
                }
            }
            "score" => {
                let score = raw.get("score").and_then(Value::as_f64).unwrap_or(0.0) as f32;
                let confidence =
                    raw.get("confidence").and_then(Value::as_f64).unwrap_or(0.0) as f32;
                Answer::Score {
                    score,
                    confidence,
                    probabilities: probability_array(raw.get("probabilities")),
                }
            }
            "predicate" => {
                let probability = raw
                    .get("probability")
                    .and_then(Value::as_f64)
                    .unwrap_or(0.0) as f32;
                Answer::Noul {
                    probability,
                    confidence: probability.max(1.0 - probability),
                }
            }
            "refusal" => Answer::Refusal {
                reason: raw
                    .get("reason")
                    .and_then(Value::as_str)
                    .map(str::to_string),
            },
            _ => continue,
        };
        out.insert(name, answer);
    }
    Ok(out)
}

fn probability_map(value: Option<&Value>) -> BTreeMap<String, f32> {
    let mut out = BTreeMap::new();
    if let Some(Value::Object(map)) = value {
        for (key, val) in map {
            if let Some(p) = val.as_f64() {
                out.insert(key.clone(), p as f32);
            }
        }
    }
    out
}

fn probability_array(value: Option<&Value>) -> BTreeMap<String, f32> {
    let mut out = BTreeMap::new();
    if let Some(Value::Array(items)) = value {
        for item in items {
            let key = item
                .get("value")
                .map(|v| {
                    v.as_str()
                        .map(str::to_string)
                        .unwrap_or_else(|| v.to_string())
                })
                .or_else(|| item.get("label").map(|v| v.to_string()))
                .unwrap_or_default();
            let p = item
                .get("probability")
                .and_then(Value::as_f64)
                .unwrap_or(0.0);
            if !key.is_empty() {
                out.insert(key, p as f32);
            }
        }
    }
    out
}

fn truncate(value: &str, max: usize) -> String {
    value.chars().take(max).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_jev_answers() {
        let value = json!({
            "model": "jev-1.13.0",
            "answers": {
                "role": {"type":"choice","choice":"admin","confidence":0.94,"probabilities":{"admin":0.94,"api":0.06}},
                "risk": {"type":"score","score":3.2,"confidence":0.8,"probabilities":{"3":0.8}},
                "is_real": {"type":"noul","noul":0.91}
            }
        });
        let answers = parse_jev_answers(&value).unwrap();
        assert_eq!(answers["role"].as_choice().unwrap().0, "admin");
        assert_eq!(answers["risk"].as_score().unwrap().0, 3.2);
        assert!((answers["is_real"].as_noul().unwrap().0 - 0.91).abs() < 0.001);
    }

    #[test]
    fn parses_luna_answers() {
        let value = json!({
            "model": "gpt-6-luna",
            "answers": [
                {"type":"choice","name":"role","choice":"api","confidence":0.88,"probabilities":[{"value":"api","probability":0.88}]},
                {"type":"predicate","name":"auth_required","probability":0.72},
                {"type":"score","name":"priority","score":2.5,"confidence":0.7,"probabilities":[{"value":2,"probability":0.7}]}
            ]
        });
        let answers = parse_luna_answers(&value).unwrap();
        assert_eq!(answers["role"].as_choice().unwrap().0, "api");
        assert!((answers["auth_required"].as_noul().unwrap().0 - 0.72).abs() < 0.001);
        assert_eq!(answers["priority"].as_score().unwrap().0, 2.5);
    }

    #[test]
    fn serializes_question_shapes() {
        let questions = crate::engine::decision::packs::endpoint_triage();
        let jev = questions_to_jev(&questions);
        assert!(jev.get("role").is_some());
        assert_eq!(jev["role"]["type"], "choice");
        let luna = questions_to_luna(&questions);
        assert!(
            luna.as_array()
                .unwrap()
                .iter()
                .any(|q| q["type"] == "predicate")
        );
    }
}

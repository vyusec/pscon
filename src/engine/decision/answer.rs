use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Answer {
    Choice {
        choice: String,
        confidence: f32,
        probabilities: BTreeMap<String, f32>,
    },
    Score {
        score: f32,
        confidence: f32,
        probabilities: BTreeMap<String, f32>,
    },
    Noul {
        probability: f32,
        confidence: f32,
    },
    Refusal {
        reason: Option<String>,
    },
}

impl Answer {
    pub fn confidence(&self) -> f32 {
        match self {
            Answer::Choice { confidence, .. } => *confidence,
            Answer::Score { confidence, .. } => *confidence,
            Answer::Noul { confidence, .. } => *confidence,
            Answer::Refusal { .. } => 0.0,
        }
    }

    pub fn as_choice(&self) -> Option<(&str, f32)> {
        match self {
            Answer::Choice {
                choice, confidence, ..
            } => Some((choice, *confidence)),
            _ => None,
        }
    }

    pub fn as_score(&self) -> Option<(f32, f32)> {
        match self {
            Answer::Score {
                score, confidence, ..
            } => Some((*score, *confidence)),
            _ => None,
        }
    }

    pub fn as_noul(&self) -> Option<(f32, f32)> {
        match self {
            Answer::Noul {
                probability,
                confidence,
            } => Some((*probability, *confidence)),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionResponse {
    pub provider: String,
    pub model: String,
    pub answers: BTreeMap<String, Answer>,
    pub latency_ms: u64,
    pub cost_usd: f64,
    pub cached: bool,
    pub usage: serde_json::Value,
}

impl DecisionResponse {
    pub fn min_confidence(&self) -> f32 {
        self.answers
            .values()
            .map(|a| a.confidence())
            .fold(1.0f32, f32::min)
    }

    pub fn answer(&self, name: &str) -> Option<&Answer> {
        self.answers.get(name)
    }
}

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Question {
    Choice {
        instructions: String,
        criteria: Vec<Criterion>,
    },
    Score {
        instructions: String,
        criteria: Vec<String>,
    },
    Noul {
        instructions: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Criterion {
    pub value: String,
    pub description: String,
}

pub type QuestionSet = BTreeMap<String, Question>;

pub fn choice(instructions: impl Into<String>, criteria: &[(&str, &str)]) -> Question {
    Question::Choice {
        instructions: instructions.into(),
        criteria: criteria
            .iter()
            .map(|(value, description)| Criterion {
                value: (*value).to_string(),
                description: (*description).to_string(),
            })
            .collect(),
    }
}

pub fn score(instructions: impl Into<String>, levels: &[&str]) -> Question {
    Question::Score {
        instructions: instructions.into(),
        criteria: levels.iter().map(|s| (*s).to_string()).collect(),
    }
}

pub fn noul(instructions: impl Into<String>) -> Question {
    Question::Noul {
        instructions: instructions.into(),
    }
}

pub fn question_set(items: Vec<(&str, Question)>) -> QuestionSet {
    items
        .into_iter()
        .map(|(name, q)| (name.to_string(), q))
        .collect()
}

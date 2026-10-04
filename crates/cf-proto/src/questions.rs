//! A question a harness's question tool puts to the chief through the
//! board (`POST /api/questions`), and the chief's answer as the API gives it
//! back.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One question in the board's shape. Its texts are what the harness gave,
/// passed through as they came; one it did not give is left out.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Question {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub question: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub header: Option<Value>,
    pub options: Vec<Choice>,
    /// Whether more than one option may be picked.
    pub multiple: bool,
}

/// An option a question offers.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Choice {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<Value>,
}

/// The chief's answer: the labels picked, one list per question.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Answer {
    #[serde(default)]
    choices: Option<Vec<Vec<String>>>,
}

impl Answer {
    /// The labels picked for the question at `at`; none when it has none.
    pub fn picks(&self, at: usize) -> &[String] {
        self.choices
            .as_ref()
            .and_then(|choices| choices.get(at))
            .map_or(&[], Vec::as_slice)
    }
}

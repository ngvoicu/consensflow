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

impl Question {
    /// A harness's question (`question`, `header`, `options` of `label` and
    /// `description`) in the board's shape, `multiple` as the harness says. None
    /// for one the JavaScript that read it could not (it threw): a question or
    /// an option that is null, or options that are no list.
    pub fn read(question: &Value, multiple: bool) -> Option<Self> {
        if question.is_null() {
            return None;
        }
        let options = match question.get("options") {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::Array(options)) => options
                .iter()
                .map(|option| {
                    (!option.is_null()).then(|| Choice {
                        label: option.get("label").cloned(),
                        description: option.get("description").cloned(),
                    })
                })
                .collect::<Option<Vec<_>>>()?,
            Some(_) => return None,
        };
        Some(Self {
            question: question.get("question").cloned(),
            header: question.get("header").cloned(),
            options,
            multiple,
        })
    }
}

/// An option a question offers.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Choice {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<Value>,
}

/// The chief's answer: its message, and the labels picked, one list per question.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Answer {
    #[serde(default)]
    id: Option<i64>,
    #[serde(default)]
    choices: Option<Vec<Vec<String>>>,
}

impl Answer {
    /// The answer's own message, which a door says it received: the API always
    /// gives it, and an answer read from anywhere else may not.
    pub fn id(&self) -> Option<i64> {
        self.id
    }

    /// The labels picked for the question at `at`; none when it has none.
    pub fn picks(&self, at: usize) -> &[String] {
        self.choices
            .as_ref()
            .and_then(|choices| choices.get(at))
            .map_or(&[], Vec::as_slice)
    }
}

/// What came of questions put to the board: the chief's answer; a refusal,
/// with what the window tells its model; or nothing in time, which leaves
/// the questions to the harness's own dialog (no board, a wait that ran out,
/// a board that could not be reached).
#[derive(Debug, Clone, PartialEq)]
pub enum Reply {
    Answered(Answer),
    Refused(String),
    Unanswered,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_a_question_as_the_board_takes_it_options_and_all() {
        let asked = Question::read(
            &json!({
                "question": "Which colour?", "header": "Colour",
                "options": [{ "label": "red", "description": "Warm" }, { "label": "blue" }, "green"],
            }),
            true,
        )
        .unwrap();
        assert_eq!(
            serde_json::to_value(asked).unwrap(),
            json!({
                "question": "Which colour?", "header": "Colour",
                "options": [{ "label": "red", "description": "Warm" }, { "label": "blue" }, {}],
                "multiple": true,
            })
        );
        let bare = Question::read(&json!({ "question": "Go on?" }), false).unwrap();
        assert_eq!(
            serde_json::to_value(bare).unwrap(),
            json!({ "question": "Go on?", "options": [], "multiple": false })
        );
    }

    #[test]
    fn a_question_that_is_no_object_reads_with_nothing_in_it() {
        let asked = Question::read(&json!("Which?"), false).unwrap();
        assert_eq!(
            serde_json::to_value(asked).unwrap(),
            json!({ "options": [], "multiple": false })
        );
    }

    #[test]
    fn a_question_the_javascript_could_not_read_is_none() {
        assert_eq!(Question::read(&json!(null), false), None);
        assert_eq!(Question::read(&json!({ "options": "red" }), false), None);
        assert_eq!(Question::read(&json!({ "options": {} }), false), None);
        assert_eq!(Question::read(&json!({ "options": [null] }), false), None);
        // No list at all is no options.
        let none = Question::read(&json!({ "options": null }), false).unwrap();
        assert!(none.options.is_empty());
    }

    #[test]
    fn the_answer_gives_the_labels_picked_for_each_question() {
        let answer: Answer =
            serde_json::from_value(json!({ "id": 7, "choices": [["a", "b"], []] })).unwrap();
        assert_eq!(answer.picks(0), ["a".to_string(), "b".to_string()]);
        assert!(answer.picks(1).is_empty() && answer.picks(2).is_empty());
        let free: Answer = serde_json::from_value(json!({ "id": 8, "choices": null })).unwrap();
        assert!(free.picks(0).is_empty());
    }
}

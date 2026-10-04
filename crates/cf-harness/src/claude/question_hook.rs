//! Claude Code's AskUserQuestion, answered from the board: the hook allows
//! the call with the chief's answers in its input, the way Claude Code
//! documents, or denies it with the board's reason.

use std::io::Read;
use std::time::Duration;

use cf_board::Board;
use serde_json::{json, Map, Value};

use crate::shared::question_hook::{answer, QuestionTool};

struct AskUserQuestion;

impl QuestionTool for AskUserQuestion {
    const TOOL: &'static str = "AskUserQuestion";

    fn answered(tool_input: &Map<String, Value>, answers: Vec<(String, String)>) -> Value {
        let mut input = tool_input.clone();
        let answers = answers
            .into_iter()
            .map(|(question, picks)| (question, Value::String(picks)));
        input.insert("answers".into(), Value::Object(answers.collect()));
        json!({
            "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "permissionDecision": "allow",
                "updatedInput": input,
            }
        })
    }

    fn refused(reason: String) -> Value {
        json!({
            "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "permissionDecision": "deny",
                "permissionDecisionReason": reason,
            }
        })
    }
}

/// What `cf hook claude` says to the PreToolUse event on `input`.
pub fn question_hook(input: &mut dyn Read, board: Option<&Board>, wait: Duration) -> Option<Value> {
    answer::<AskUserQuestion>(input, board, wait)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_answers_join_the_tool_input_in_place_and_by_question() {
        let input = json!({ "answers": { "old": "x" }, "questions": [], "metadata": 1 });
        let said = AskUserQuestion::answered(
            input.as_object().unwrap(),
            vec![
                ("Which?".into(), "a".into()),
                ("Twice?".into(), "b".into()),
                ("Which?".into(), "c".into()),
            ],
        );
        assert_eq!(
            said["hookSpecificOutput"]["updatedInput"].to_string(),
            r#"{"answers":{"Which?":"c","Twice?":"b"},"questions":[],"metadata":1}"#
        );
    }
}

//! Devin's ask_user_question, answered from the board. Devin draws its
//! dialog even over a pre-filled input (probed 2026-09-20), so the hook
//! refuses the call and hands the answer over as the refusal's reason, which
//! Devin reads and goes on with.

use std::io::Read;
use std::time::Duration;

use cf_board::Board;
use serde_json::{json, Map, Value};

use crate::shared::question_hook::{answer, QuestionTool};

struct AskUserQuestion;

impl QuestionTool for AskUserQuestion {
    const TOOL: &'static str = "ask_user_question";

    fn answered(_tool_input: &Map<String, Value>, answers: Vec<(String, String)>) -> Value {
        let answers: Vec<String> = answers
            .into_iter()
            .map(|(question, picks)| format!("{question} {picks}"))
            .collect();
        json!({
            "decision": "block",
            "reason": format!(
                "ConsensFlow answered from the board: {}. Continue with that answer; do not ask again.",
                answers.join(" · ")
            ),
        })
    }

    fn refused(reason: String) -> Value {
        json!({ "decision": "block", "reason": reason })
    }
}

/// What `cf hook devin` says to the PreToolUse event on `input`.
pub fn question_hook(input: &mut dyn Read, board: Option<&Board>, wait: Duration) -> Option<Value> {
    answer::<AskUserQuestion>(input, board, wait)
}

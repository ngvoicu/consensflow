//! Devin's ask_user_question, answered from the board. Devin draws its
//! dialog even over a pre-filled input (probed 2026-09-20), so the hook
//! refuses the call and hands the answer over as the refusal's reason, which
//! Devin reads and goes on with.

use std::io::Read;

use cf_proto::questions::{Question, Reply};
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

/// What `cf hook devin` prints for the PreToolUse event on `input`, its
/// questions put to the board through `ask`.
pub fn question_hook(
    input: &mut dyn Read,
    ask: impl FnOnce(&[Question]) -> Reply,
) -> Option<String> {
    answer::<AskUserQuestion>(input, ask)
}

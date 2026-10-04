//! A harness's question tool answered from the board: a PreToolUse hook on
//! the tool (Claude Code's AskUserQuestion, Devin's ask_user_question) puts
//! its questions to the chief, waits for the answer, and hands it back in
//! the form its harness reads. A question the board refuses is refused in
//! the window too, with the board's reason, so the model asks with `cf ask`:
//! nobody watches a member's window, and its own dialog would hold the task
//! for good. Anything else (no board, a wait that ran out, another tool, an
//! event it cannot read) ends silently, and the harness shows its own dialog.

use std::io::Read;
use std::time::Duration;

use cf_base::js;
use cf_base::json::from_slice_lossy;
use cf_board::door::{ask_the_board, refusal_reason, Choice, Question};
use cf_board::Board;
use serde_json::{Map, Value};

/// How one harness's question tool is named, and how its hook answers.
pub(crate) trait QuestionTool {
    /// The tool's name in the hook's event.
    const TOOL: &'static str;

    /// What the hook says when the board answered: each question's text with
    /// the labels the chief picked for it.
    fn answered(tool_input: &Map<String, Value>, answers: Vec<(String, String)>) -> Value;

    /// What the hook says when the board refused the question.
    fn refused(reason: String) -> Value;
}

/// What `T`'s hook says to the PreToolUse event on `input`, asking `board`
/// and waiting up to `wait`; nothing when it has nothing to say.
pub(crate) fn answer<T: QuestionTool>(
    input: &mut dyn Read,
    board: Option<&Board>,
    wait: Duration,
) -> Option<Value> {
    let board = board?;
    let mut bytes = Vec::new();
    input.read_to_end(&mut bytes).ok()?;
    let event = from_slice_lossy(&bytes).ok()?;
    if event.get("tool_name").and_then(Value::as_str) != Some(T::TOOL) {
        return None;
    }
    let tool_input = event.get("tool_input")?.as_object()?;
    let questions = tool_input.get("questions")?.as_array()?;
    let asked = questions
        .iter()
        .map(board_question)
        .collect::<Option<Vec<_>>>()?;
    match ask_the_board(board, &asked, wait) {
        Ok(Some(answer)) => {
            let answers = questions
                .iter()
                .enumerate()
                .map(|(at, question)| {
                    (
                        js::text(question.get("question")).into_owned(),
                        answer.picks(at).join(", "),
                    )
                })
                .collect();
            Some(T::answered(tool_input, answers))
        }
        Ok(None) => None,
        Err(cause) if cause.is_refusal() => Some(T::refused(refusal_reason(&cause))),
        Err(_) => None,
    }
}

/// One of the tool's questions in the board's shape; none for a question
/// the hook's JavaScript could not read (it threw, and so said nothing).
fn board_question(question: &Value) -> Option<Question> {
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
    Some(Question {
        question: question.get("question").cloned(),
        header: question.get("header").cloned(),
        options,
        multiple: question.get("multiSelect") == Some(&Value::Bool(true)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_a_question_as_the_board_takes_it_options_and_all() {
        let asked = board_question(&json!({
            "question": "Which colour?", "header": "Colour", "multiSelect": true,
            "options": [{ "label": "red", "description": "Warm" }, { "label": "blue" }, "green"],
        }))
        .unwrap();
        assert_eq!(
            serde_json::to_value(asked).unwrap(),
            json!({
                "question": "Which colour?", "header": "Colour",
                "options": [{ "label": "red", "description": "Warm" }, { "label": "blue" }, {}],
                "multiple": true,
            })
        );
        let bare = board_question(&json!({ "question": "Go on?" })).unwrap();
        assert_eq!(
            serde_json::to_value(bare).unwrap(),
            json!({ "question": "Go on?", "options": [], "multiple": false })
        );
    }

    #[test]
    fn a_question_it_cannot_read_is_none() {
        assert_eq!(board_question(&json!(null)), None);
        assert_eq!(board_question(&json!({ "options": "red" })), None);
        assert_eq!(board_question(&json!({ "options": [null] })), None);
    }
}

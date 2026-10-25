//! What the live tests of `crates/cf-e2e` ask of the engine's texts, through the test
//! binary `engine-ask` (`live::ask` there runs it), which has no logic of its own: the
//! words the engine gives a window, written by the engine's own code, where a live
//! test gives a window of its own what the daemon would.
//!
//! A question is a JSON object with an `op`, and its answer one JSON value:
//!
//! | `op` | the question | the answer |
//! |---|---|---|
//! | `cut_note` | `message` (its id), `tasks` (the number of the task a member had at work or waiting, one; none for the chief) | the note that tells a window the human's Resume brings back that its turn was cut, as the window is given it: its header, and the engine's words; an error for several tasks, as a window that held several comes back for none |
//! | `is_paste_of` | `text` (an item of a harness's record), `message` (an id) | whether the engine takes the item for the paste of that message, which proves it arrived (`delivery_text::is_paste_of`) |

use cf_proto::ledger::MessageView;
use serde_json::{json, Value};

use crate::delivery_text::{delivery_text, is_paste_of};
use crate::resume::Coming;

/// The answer to `question`, or why it cannot be answered.
pub fn answer(question: &Value) -> Result<Value, String> {
    match question.get("op").and_then(Value::as_str) {
        Some("cut_note") => cut_note(question),
        Some("is_paste_of") => paste_of(question),
        other => Err(format!(
            "the question asks for {}, which is neither cut_note nor is_paste_of",
            other.unwrap_or("nothing")
        )),
    }
}

/// Whether the engine takes the item `text` of a record for the paste of the message
/// `message`.
fn paste_of(question: &Value) -> Result<Value, String> {
    let text = question
        .get("text")
        .and_then(Value::as_str)
        .ok_or("the question names no text")?;
    let id = question
        .get("message")
        .and_then(Value::as_i64)
        .ok_or("the question names no message")?;
    Ok(json!(is_paste_of(text, id, None)))
}

/// The note a window is given as the first thing the human's Resume gives it, when
/// its turn was cut: a member's, about the tasks it had in hand, or the chief's where
/// there are none.
fn cut_note(question: &Value) -> Result<Value, String> {
    let id = question
        .get("message")
        .and_then(Value::as_i64)
        .ok_or("the question names no message")?;
    let tasks: Vec<i64> = question
        .get("tasks")
        .and_then(Value::as_array)
        .map(|tasks| tasks.iter().filter_map(Value::as_i64).collect())
        .unwrap_or_default();
    let coming = match tasks.as_slice() {
        [] => Coming::Chief,
        [task] => Coming::Member {
            task: *task,
            paid: None,
        },
        _ => {
            return Err(
                "a window that held several tasks comes back for none, and is told nothing"
                    .to_owned(),
            )
        }
    };
    // The note as the ledger writes it: from ConsensFlow, on the member's task.
    let note = MessageView {
        id,
        project_id: 0,
        recipient: String::new(),
        recipient_id: 0,
        recipient_role: String::new(),
        sender: None,
        kind: "note".to_owned(),
        task_number: coming.task(),
        reply_to: None,
        body: coming.words(),
        state: "delivering".to_owned(),
        attempts: 0,
        reason: None,
        receipt: Value::Null,
        questions: Value::Null,
        choices: Value::Null,
        urgent: false,
        created_at: String::new(),
        delivered_at: None,
    };
    Ok(json!(delivery_text(&note, &[])))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_member_is_told_its_turn_on_its_tasks_was_cut_and_the_chief_that_it_stopped() {
        let member = answer(&json!({ "op": "cut_note", "message": 2, "tasks": [1] })).unwrap();
        assert_eq!(
            member,
            "[ConsensFlow m-2 · T-1 · note from ConsensFlow]\nConsensFlow stopped while you were working on T-1. Go on where you stopped."
        );
        // A window that held several tasks comes back for none, and is told nothing.
        assert!(answer(&json!({ "op": "cut_note", "message": 2, "tasks": [1, 2] })).is_err());
        let chief = answer(&json!({ "op": "cut_note", "message": 7 })).unwrap();
        assert_eq!(
            chief,
            "[ConsensFlow m-7 · note from ConsensFlow]\nConsensFlow stopped in the middle of your turn, which was cut. Go on where you stopped."
        );
        assert!(answer(&json!({ "op": "cut_note" })).is_err());
        assert!(answer(&json!({ "op": "else" })).is_err());
    }

    #[test]
    fn an_item_is_a_paste_of_the_message_the_engine_takes_it_for() {
        let paste = |text: &str, message: i64| {
            answer(&json!({ "op": "is_paste_of", "text": text, "message": message })).unwrap()
        };
        let header = "[ConsensFlow m-4 · T-1 · result from @designer]";
        assert_eq!(paste(&format!("{header}\nDone."), 4), true);
        // Behind the placeholders of the pictures Claude attached.
        assert_eq!(paste(&format!("[Image #3]{header}\nDone."), 4), true);
        assert_eq!(paste(&format!("[Image #3]{header}\nDone."), 5), false);
        assert_eq!(paste(&format!("See {header}"), 4), false);
        assert!(answer(&json!({ "op": "is_paste_of", "message": 4 })).is_err());
        assert!(answer(&json!({ "op": "is_paste_of", "text": "x" })).is_err());
    }
}

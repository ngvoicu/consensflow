//! What the live tests of `crates/cf-e2e` ask of the engine's texts, through the test
//! binary `engine-ask` (`live::ask` there runs it), which has no logic of its own: the
//! words the engine gives a window, written by the engine's own code, where a live
//! test gives a window of its own what the daemon would.
//!
//! A question is a JSON object with an `op`, and its answer one JSON value:
//!
//! | `op` | the question | the answer |
//! |---|---|---|
//! | `cut_note` | `message` (its id), `tasks` (the numbers of the tasks a member had at work or waiting; none for the chief) | the note that tells a window the human's Resume brings back that its turn was cut, as the window is given it: its header, and the engine's words |
//! | `ours` | `lines` (a screen's last lines), `messages` (the messages pasted into the window, each its `id`, `kind`, `sender`, `taskNumber` and `body`, as the board gives them) | for each line, whether it is ConsensFlow's, by the rule a failed turn's note reads the screen by (`shown::ours`) |

use cf_proto::ledger::MessageView;
use serde_json::{json, Value};

use crate::delivery_text::delivery_text;
use crate::resume::Coming;
use crate::shown::{ours, Paste};

/// The answer to `question`, or why it cannot be answered.
pub fn answer(question: &Value) -> Result<Value, String> {
    match question.get("op").and_then(Value::as_str) {
        Some("cut_note") => cut_note(question),
        Some("ours") => ours_of(question),
        other => Err(format!(
            "the question asks for {}, which is neither cut_note nor ours",
            other.unwrap_or("nothing")
        )),
    }
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
    let coming = if tasks.is_empty() {
        Coming::Chief
    } else {
        Coming::Member(tasks)
    };
    // The note as the ledger writes it: from ConsensFlow, on the member's first task.
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

/// Which of a screen's `lines` are ConsensFlow's, of the `messages` pasted into
/// its window: each as the window was given it, the newest first.
fn ours_of(question: &Value) -> Result<Value, String> {
    let lines: Vec<String> = question
        .get("lines")
        .and_then(Value::as_array)
        .ok_or("the question gives no lines")?
        .iter()
        .map(|line| line.as_str().unwrap_or_default().to_owned())
        .collect();
    let mut messages: Vec<MessageView> = question
        .get("messages")
        .and_then(Value::as_array)
        .ok_or("the question gives no messages")?
        .iter()
        .map(pasted)
        .collect::<Result<_, _>>()?;
    messages.sort_by_key(|message| std::cmp::Reverse(message.id));
    let pastes: Vec<Paste> = messages
        .iter()
        .map(|message| Paste {
            id: message.id,
            text: delivery_text(message, &[]),
        })
        .collect();
    Ok(json!(ours(&lines, &pastes)))
}

/// A message as the board gives it, with what its paste reads by.
fn pasted(message: &Value) -> Result<MessageView, String> {
    let text = |name: &str| message.get(name).and_then(Value::as_str).map(str::to_owned);
    Ok(MessageView {
        id: message
            .get("id")
            .and_then(Value::as_i64)
            .ok_or("a message has no id")?,
        project_id: 0,
        recipient: String::new(),
        recipient_id: 0,
        recipient_role: String::new(),
        sender: text("sender"),
        kind: text("kind").ok_or("a message has no kind")?,
        task_number: message.get("taskNumber").and_then(Value::as_i64),
        reply_to: message.get("replyTo").and_then(Value::as_i64),
        body: text("body").unwrap_or_default(),
        state: "delivered".to_owned(),
        attempts: 0,
        reason: None,
        receipt: Value::Null,
        questions: Value::Null,
        choices: Value::Null,
        urgent: false,
        created_at: String::new(),
        delivered_at: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_member_is_told_its_turn_on_its_tasks_was_cut_and_the_chief_that_it_stopped() {
        let member = answer(&json!({ "op": "cut_note", "message": 2, "tasks": [1] })).unwrap();
        assert_eq!(
            member,
            "[ConsensFlow m-2 · T-1 · note from ConsensFlow]\nConsensFlow quit while you were working on T-1. Go on where you stopped."
        );
        let chief = answer(&json!({ "op": "cut_note", "message": 7 })).unwrap();
        assert_eq!(
            chief,
            "[ConsensFlow m-7 · note from ConsensFlow]\nConsensFlow stopped in the middle of your turn, which was cut. Go on where you stopped."
        );
        assert!(answer(&json!({ "op": "cut_note" })).is_err());
        assert!(answer(&json!({ "op": "else" })).is_err());
    }

    #[test]
    fn a_screen_is_told_line_by_line_whose_it_is() {
        let lines = json!([
            "[ConsensFlow m-4 · T-1 · task from @chief]",
            "Write the parser",
            "Error: provider unreachable"
        ]);
        let messages = json!([{
            "id": 4, "kind": "task", "sender": "chief", "taskNumber": 1, "body": "Write the parser"
        }]);
        let said = answer(&json!({ "op": "ours", "lines": lines, "messages": messages })).unwrap();
        assert_eq!(said, json!([true, true, false]));
    }
}

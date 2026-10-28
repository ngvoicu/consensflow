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
//! | `ours` | `lines` (a screen's last lines), `recipient` (the window's handle), `messages` (messages as the board gives them, each its `id`, `kind`, `sender`, `recipient`, `taskNumber` and `body`: those to `recipient` are what was pasted into the window) | for each line, whether it is ConsensFlow's, by the rule a failed turn's note reads the screen by (`shown::ours`) |
//! | `is_paste_of` | `text` (an item of a harness's record), `message` (an id) | whether the engine takes the item for the paste of that message, which proves it arrived (`delivery_text::is_paste_of`) |

use cf_proto::ledger::MessageView;
use serde_json::{json, Value};

use crate::delivery_text::{delivery_text, is_paste_of};
use crate::resume::Coming;
use crate::shown::{ours, Paste};

/// The answer to `question`, or why it cannot be answered.
pub fn answer(question: &Value) -> Result<Value, String> {
    match question.get("op").and_then(Value::as_str) {
        Some("cut_note") => cut_note(question),
        Some("ours") => ours_of(question),
        Some("is_paste_of") => paste_of(question),
        other => Err(format!(
            "the question asks for {}, which is none of cut_note, ours and is_paste_of",
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
        body: coming.words(None),
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
/// its window: each as the window was given it, the newest first. Only messages to
/// the window's `recipient` went into it, as the engine reads what was pasted
/// (`last_pastes`): a task's thread holds its worker's result too.
fn ours_of(question: &Value) -> Result<Value, String> {
    let recipient = question
        .get("recipient")
        .and_then(Value::as_str)
        .ok_or("the question names no recipient")?;
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
        .filter(|message| message.get("recipient").and_then(Value::as_str) == Some(recipient))
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
    fn a_screen_is_told_line_by_line_whose_it_is() {
        let lines = json!([
            "[ConsensFlow m-4 · T-1 · task from @chief]",
            "Write the parser",
            "Error: provider unreachable"
        ]);
        let messages = json!([{
            "id": 4, "kind": "task", "sender": "chief", "recipient": "zeus", "taskNumber": 1,
            "body": "Write the parser"
        }]);
        let said = answer(
            &json!({ "op": "ours", "recipient": "zeus", "lines": lines, "messages": messages }),
        )
        .unwrap();
        assert_eq!(said, json!([true, true, false]));
    }

    #[test]
    fn what_the_window_was_given_is_its_own_messages_not_the_threads_others() {
        // The task's thread holds the worker's result too, to the chief, newer than
        // its brief: it went into no window of the worker's (Calliope's re-check T-305).
        let messages = json!([
            { "id": 4, "kind": "task", "sender": "chief", "recipient": "zeus", "taskNumber": 1, "body": "Write the parser" },
            { "id": 5, "kind": "result", "sender": "zeus", "recipient": "chief", "taskNumber": 1, "body": "Done" }
        ]);
        let question = json!({
            "op": "ours",
            "recipient": "zeus",
            "lines": ["Write the parser", "Done"],
            "messages": messages
        });
        assert_eq!(answer(&question).unwrap(), json!([true, false]));
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

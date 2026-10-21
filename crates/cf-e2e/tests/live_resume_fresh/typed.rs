//! What the human types into a chief's window to have it hold a key, and asks it
//! for once its window is back, and what the harness's own record says of both:
//! whether the window took what was typed, when the turn that gave the key is
//! saved, and when the chief gives it back. Each is a line short enough for the
//! window's input box to show whole, as a line that wraps is not what the other
//! checks that drive a real Devin type.

use cf_e2e::live::{ask, Harness};

/// What the human asks the chief once its window is back: for the key, which the
/// question does not say.
pub const ASKED: &str = "What was the key? Reply with exactly the key, nothing else.";

/// The key the human gives `harness`'s chief to remember: one word, which no
/// harness's own screen holds.
pub fn key_of(harness: Harness) -> String {
    format!("KEY_{}", harness.name().to_ascii_uppercase())
}

/// What the human types into the chief's window to have it hold `key`.
pub fn the_key(key: &str) -> String {
    format!("Remember this key and reply noted: {key}")
}

/// Whether `record` holds a message of the human's with `mark` in it: the window
/// took what was typed, and the harness has it.
pub fn took(record: &ask::Record, mark: &str) -> bool {
    record
        .items
        .iter()
        .any(|item| item.role == "user" && item.text.contains(mark))
}

/// Whether `record` holds the turn that gave `key`: the human's message, and the
/// turn over, so that what the harness saved is the whole of it.
pub fn saved_turn(record: &ask::Record, key: &str) -> bool {
    record.settled && took(record, key)
}

/// The line of `record` in which the chief gives `key` back, after the question that
/// asks for it: what it said before is not the answer.
pub fn remembers(record: &ask::Record, key: &str) -> Option<String> {
    let asked = record
        .items
        .iter()
        .position(|item| item.role == "user" && item.text.contains(ASKED))?;
    record.items[asked..]
        .iter()
        .find(|item| item.role == "assistant" && item.text.contains(key))
        .map(|item| item.text.clone())
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;

    /// An item of a record, as the daemon reads it.
    fn said(role: &str, text: &str) -> ask::Item {
        ask::Item {
            id: format!("{role}-{text}"),
            role: role.to_owned(),
            text: text.to_owned(),
            complete: true,
        }
    }

    /// A record of these items, its turn over or not.
    fn record(items: Vec<ask::Item>, settled: bool) -> ask::Record {
        ask::Record {
            items,
            settled,
            settlement: if settled { "settled" } else { "in-flight" }.to_owned(),
            failed: Value::Null,
            quota: Value::Null,
            agents_pending: false,
            jobs_pending: 0,
            jobs_retired: 0,
        }
    }

    #[test]
    fn every_harness_is_given_a_key_of_its_own_to_remember_that_the_question_never_says() {
        let keys: Vec<String> = Harness::ALL.into_iter().map(key_of).collect();
        assert_eq!(
            keys,
            [
                "KEY_CLAUDE",
                "KEY_OPENCODE",
                "KEY_PI",
                "KEY_DEVIN",
                "KEY_CODEX"
            ]
        );
        assert_eq!(
            the_key("KEY_PI"),
            "Remember this key and reply noted: KEY_PI"
        );
        for key in keys {
            assert!(!ASKED.contains(&key), "{key}");
        }
    }

    #[test]
    fn what_is_typed_is_a_line_the_input_box_of_an_eighty_column_window_shows_whole() {
        // The prompt mark and a space come before it.
        for typed in [the_key("KEY_OPENCODE"), ASKED.to_owned()] {
            assert!(!typed.contains('\n'), "{typed}");
            assert!(typed.chars().count() + 2 < 80, "{typed}");
        }
    }

    #[test]
    fn the_window_took_what_was_typed_once_the_record_holds_a_message_of_the_humans_with_it() {
        let typed = said("user", &the_key("KEY_PI"));
        assert!(took(&record(vec![typed], false), "KEY_PI"));
        assert!(
            !took(&record(vec![said("assistant", "KEY_PI")], true), "KEY_PI"),
            "what the chief says is not what the human typed"
        );
        assert!(!took(&record(vec![], true), "KEY_PI"), "nothing yet");
    }

    #[test]
    fn a_turn_is_saved_once_the_record_holds_the_message_that_gave_the_key_and_its_turn_is_over() {
        let given = said("user", &the_key("KEY_PI"));
        assert!(saved_turn(&record(vec![given.clone()], true), "KEY_PI"));
        assert!(
            !saved_turn(&record(vec![given], false), "KEY_PI"),
            "the turn is still going on"
        );
        assert!(!saved_turn(&record(vec![], true), "KEY_PI"));
        assert!(
            !saved_turn(&record(vec![said("assistant", "KEY_PI")], true), "KEY_PI"),
            "what the chief says is not what the human typed"
        );
    }

    #[test]
    fn the_key_is_given_back_only_by_an_answer_after_the_question_that_asks_for_it() {
        let given = said("user", &the_key("KEY_PI"));
        let noted = said("assistant", "noted KEY_PI");
        let asked = said("user", ASKED);
        // Said before it was asked: no answer to it.
        assert_eq!(
            remembers(
                &record(vec![given.clone(), noted.clone(), asked.clone()], true),
                "KEY_PI"
            ),
            None
        );
        assert_eq!(
            remembers(
                &record(vec![given, noted, asked, said("assistant", "KEY_PI")], true),
                "KEY_PI"
            ),
            Some("KEY_PI".to_owned())
        );
        assert_eq!(
            remembers(&record(vec![], true), "KEY_PI"),
            None,
            "never asked"
        );
    }
}

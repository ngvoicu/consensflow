//! How a turn of a window is read from its harness's record, as the daemon reads it: a
//! message arrived once its header is in a user's item, and its turn ended once an
//! assistant's item follows it with the record at rest; how the wait for one ended
//! ([`Turn`]); and the record as lines of a report.

use std::time::Duration;

use cf_e2e::devin_store::Message;
use cf_e2e::live::ask::Record;
use cf_e2e::live::members::squash;

use crate::case::BYPASS;

/// How the wait for a turn ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Turn {
    /// The message showed in the window's record.
    pub arrived: bool,
    /// The turn it began ended: the record has the turn's end, and is at rest.
    pub ended: bool,
    /// The window ended before.
    pub closed: bool,
    pub took: Duration,
    /// Where the turn did not end, what the window showed.
    pub held: Option<String>,
    /// Where the window waited at a permission dialog that nobody answered (its terminal
    /// said so, or its screen showed the options): what it said and since when.
    pub dialog: Option<String>,
}

/// Whether `header`, the start of a message's header, is in the record, and whether
/// the turn it began has ended: the record is at rest, and the last item that is not
/// a note of the harness's own is an assistant's, after the message. A record at rest
/// whose last item is an assistant's is complete, cancelled or failed: the daemon reads
/// it as over, and a window that has said nothing of a turn yet reads at rest with the
/// message as its last item, which is no end. The way the daemon confirms that a
/// message arrived, and reads a window at rest.
pub fn told(record: Option<&Record>, header: &str) -> (bool, bool) {
    let Some(record) = record else {
        return (false, false);
    };
    let Some(at) = record
        .items
        .iter()
        .rposition(|item| item.role == "user" && item.text.contains(header))
    else {
        return (false, false);
    };
    let last = record
        .items
        .iter()
        .rposition(|item| item.role != "custom")
        .unwrap_or_default();
    let ended = record.settled && last > at && record.items[last].role == "assistant";
    (true, ended)
}

/// How Devin stored the user's message that holds `header` among `messages` of its
/// store, the first of them: with the header first, behind `/bypass` (the command that
/// switches a window to full permission, which a resumed window's first message goes
/// behind), or behind other words. None where no user's message holds it.
pub fn stored_shape(messages: &[Message], header: &str) -> Option<String> {
    let message = messages
        .iter()
        .find(|message| message.role == "user" && message.text.contains(header))?;
    let text = message.text.trim_start();
    Some(if text.starts_with(header) {
        "with its header first, /bypass not kept".to_owned()
    } else if text.starts_with(BYPASS) {
        format!("behind {BYPASS}, kept: {}", squash(text, 60))
    } else {
        format!("behind other words: {}", squash(text, 60))
    })
}

/// What fails a case whose first message is the product's own (the note, in its prompt
/// file) in how Devin stored it among `messages`: None where it is stored with its header
/// first, as the product gave it. The daemon's reader reads it as a paste whatever its
/// launch folder says only then; stored with `/bypass` kept, a start that swept its
/// launch folder leaves it no paste, and it is given again: the limit written beside the
/// Devin record reader's `behind_bypass`, which this check is the proof of.
pub fn stored_fault(messages: &[Message], header: &str) -> Option<String> {
    let Some(message) = messages
        .iter()
        .find(|message| message.role == "user" && message.text.contains(header))
    else {
        return Some("the daemon's reader has the note, Devin's store has it nowhere".to_owned());
    };
    let text = message.text.trim_start();
    if text.starts_with(header) {
        None
    } else if text.starts_with(BYPASS) {
        Some(format!(
            "Devin stored the product's first message with {BYPASS} kept: {}; a start that \
             sweeps its launch folder leaves it no paste, and it is given again (the limit \
             beside behind_bypass, crates/cf-harness/src/devin/record/answer.rs)",
            squash(text, 60)
        ))
    } else {
        Some(format!(
            "Devin stored the product's first message behind other words: {}",
            squash(text, 60)
        ))
    }
}

/// Whether the daemon's reader has the user's item that holds `header` with the header
/// first, as the daemon takes a paste of it (`is_paste_of`).
pub fn read_header_first(record: Option<&Record>, header: &str) -> bool {
    record.is_some_and(|record| {
        record
            .items
            .iter()
            .find(|item| item.role == "user" && item.text.contains(header))
            .is_some_and(|item| item.text.trim_start().starts_with(header))
    })
}

/// How the daemon's own reader reads the record, as a line.
pub fn reading(record: Option<&Record>) -> String {
    let Some(record) = record else {
        return "the daemon's reader: no record".to_owned();
    };
    let last = record.items.last().map_or_else(
        || "nothing".to_owned(),
        |item| {
            let said = squash(&item.text, 60);
            format!(
                "{}: {}",
                item.role,
                if said.is_empty() { "(no text)" } else { &said }
            )
        },
    );
    format!(
        "the daemon's reader: {}, {}, {} items, last {last}",
        record.settlement,
        if record.settled {
            "at rest"
        } else {
            "not at rest"
        },
        record.items.len()
    )
}

/// The last `last` items of the record, as a line: who each is from and what it says
/// (an item that says nothing, a tool's call, is there as that).
pub fn tail(record: Option<&Record>, last: usize) -> String {
    let Some(record) = record else {
        return "no record".to_owned();
    };
    let items = &record.items;
    let said: Vec<String> = items[items.len().saturating_sub(last)..]
        .iter()
        .map(|item| {
            let words = squash(&item.text, 100);
            format!(
                "{}: {}",
                item.role,
                if words.is_empty() {
                    "(no text)"
                } else {
                    &words
                }
            )
        })
        .collect();
    format!("{} items; the last: {}", items.len(), said.join(" | "))
}

#[cfg(test)]
mod tests {
    use cf_e2e::live::ask::Item;
    use serde_json::Value;

    use super::*;

    fn item(role: &str, text: &str, complete: bool) -> Item {
        Item {
            id: String::new(),
            role: role.to_owned(),
            text: text.to_owned(),
            complete,
        }
    }

    fn record(items: Vec<Item>, settled: bool) -> Record {
        Record {
            items,
            settled,
            settlement: if settled { "settled" } else { "in-flight" }.to_owned(),
            failed: Value::Null,
            quota: Value::Null,
        }
    }

    #[test]
    fn a_message_has_arrived_when_its_header_is_in_a_users_item_and_its_turn_has_ended_after_it() {
        let header = "[ConsensFlow m-10 ";
        let asked = item(
            "user",
            "[ConsensFlow m-10 · T-1 · note from @chief]\nrun it",
            true,
        );
        assert_eq!(told(None, header), (false, false));
        assert_eq!(told(Some(&record(vec![], true)), header), (false, false));
        // Not in the record yet, or in the words of another role: not arrived.
        let other = [item("assistant", "[ConsensFlow m-10 ", true)];
        assert_eq!(
            told(Some(&record(other.to_vec(), true)), header),
            (false, false)
        );
        // In the record with nothing after it, though the record reads at rest: begun.
        assert_eq!(
            told(Some(&record(vec![asked.clone()], true)), header),
            (true, false)
        );
        // A record not at rest, whatever it ends in: going on.
        let going = vec![asked.clone(), item("assistant", "", false)];
        assert_eq!(told(Some(&record(going, false)), header), (true, false));
        let ended = vec![asked.clone(), item("assistant", "done", true)];
        assert_eq!(
            told(Some(&record(ended.clone(), false)), header),
            (true, false)
        );
        assert_eq!(told(Some(&record(ended, true)), header), (true, true));
        // At rest with an assistant's words that end no turn: a turn cancelled or failed,
        // which the daemon reads as over. A tool's call still waiting for its answer is not.
        let cut = vec![asked.clone(), item("assistant", "", false)];
        assert_eq!(told(Some(&record(cut, true)), header), (true, true));
        let waiting = vec![
            asked.clone(),
            item("assistant", "", false),
            item("tool", "", true),
        ];
        assert_eq!(told(Some(&record(waiting, true)), header), (true, false));
        // The harness's own notes after the end of a turn, or after the message alone,
        // are no part of either.
        let noted = vec![
            asked.clone(),
            item("assistant", "done", true),
            item("custom", "a note", true),
        ];
        assert_eq!(told(Some(&record(noted, true)), header), (true, true));
        let only_notes = vec![asked.clone(), item("custom", "a note", true)];
        assert_eq!(told(Some(&record(only_notes, true)), header), (true, false));
        // The last asking is the one that counts: a turn before it ended, this one has not.
        let again = vec![asked.clone(), item("assistant", "done", true), asked];
        assert_eq!(told(Some(&record(again, true)), header), (true, false));
        // m-1 is not found in m-10's header.
        let first = vec![
            item("user", "[ConsensFlow m-10 · x]", true),
            item("assistant", "ok", true),
        ];
        assert_eq!(
            told(Some(&record(first, true)), "[ConsensFlow m-1 "),
            (false, false)
        );
    }

    #[test]
    fn how_devin_stored_a_message_is_its_header_first_behind_bypass_or_behind_other_words() {
        let header = "[ConsensFlow m-2 ";
        let stored = |text: &str| Message {
            id: "u-1".to_owned(),
            role: "user".to_owned(),
            text: text.to_owned(),
            raw: Value::Null,
        };
        let note = "[ConsensFlow m-2 · T-1 · note from ConsensFlow]\nGo on where you stopped.";
        assert_eq!(
            stored_shape(&[stored(note)], header).as_deref(),
            Some("with its header first, /bypass not kept")
        );
        assert!(
            stored_shape(&[stored(&format!("/bypass {note}"))], header).is_some_and(
                |shape| shape.starts_with("behind /bypass, kept: /bypass [ConsensFlow m-2")
            )
        );
        assert!(stored_shape(&[stored(&format!("Look: {note}"))], header)
            .is_some_and(|shape| shape.starts_with("behind other words")));
        assert_eq!(stored_shape(&[stored("hello")], header), None);
        // The reader's item, as the daemon takes a paste.
        let read = |text: &str| record(vec![item("user", text, true)], true);
        assert!(read_header_first(Some(&read(note)), header));
        assert!(!read_header_first(
            Some(&read(&format!("/bypass {note}"))),
            header
        ));
        assert!(!read_header_first(None, header));
    }

    #[test]
    fn the_products_first_message_stored_with_bypass_kept_fails_its_case_naming_the_limit() {
        let header = "[ConsensFlow m-2 ";
        let stored = |text: &str| Message {
            id: "u-1".to_owned(),
            role: "user".to_owned(),
            text: text.to_owned(),
            raw: Value::Null,
        };
        let note = "[ConsensFlow m-2 · T-1 · note from ConsensFlow]\nGo on where you stopped.";
        assert_eq!(stored_fault(&[stored(note)], header), None);
        assert_eq!(stored_fault(&[stored(&format!("\n{note}"))], header), None);
        let kept = stored_fault(&[stored(&format!("/bypass {note}"))], header)
            .expect("a first message stored behind /bypass fails its case");
        assert!(kept.contains("with /bypass kept: /bypass [ConsensFlow m-2"));
        assert!(kept.contains("behind_bypass, crates/cf-harness/src/devin/record/answer.rs"));
        assert!(stored_fault(&[stored(&format!("Look: {note}"))], header)
            .is_some_and(|fault| fault.contains("behind other words: Look: [ConsensFlow m-2")));
        // Not in the store, or only in another role's words: no proof, which fails too.
        let other = Message {
            role: "assistant".to_owned(),
            ..stored(note)
        };
        for messages in [vec![], vec![stored("hello")], vec![other]] {
            assert!(stored_fault(&messages, header).is_some_and(|fault| fault.ends_with("nowhere")));
        }
    }

    #[test]
    fn the_end_of_a_record_is_its_last_items_and_a_tools_call_that_says_nothing_is_one() {
        assert_eq!(tail(None, 3), "no record");
        let read = record(
            vec![
                item("user", "go", true),
                item("assistant", "", false),
                item("tool", "exit 0", true),
                item("assistant", "done  now", true),
            ],
            true,
        );
        assert_eq!(
            tail(Some(&read), 3),
            "4 items; the last: assistant: (no text) | tool: exit 0 | assistant: done now"
        );
        assert_eq!(tail(Some(&record(vec![], true)), 3), "0 items; the last: ");
    }

    #[test]
    fn the_daemons_reader_says_what_it_read_the_turn_as_and_the_last_item() {
        assert_eq!(reading(None), "the daemon's reader: no record");
        let read = record(
            vec![
                item("user", "hi", true),
                item("assistant", "hello  there", false),
            ],
            false,
        );
        assert_eq!(
            reading(Some(&read)),
            "the daemon's reader: in-flight, not at rest, 2 items, last assistant: hello there"
        );
        assert_eq!(
            reading(Some(&record(vec![], true))),
            "the daemon's reader: settled, at rest, 0 items, last nothing"
        );
        assert!(reading(Some(&record(vec![item("tool", "", true)], true)))
            .ends_with("last tool: (no text)"));
    }
}

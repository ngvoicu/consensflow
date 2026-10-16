//! How a turn of a window is read from its harness's record, as the daemon reads it: a
//! message arrived once its header is in a user's item, and its turn ended once an
//! assistant's item follows it with the record at rest; how the wait for one ended
//! ([`Turn`]); and the record as lines of a report.

use std::time::Duration;

use cf_e2e::live::ask::Record;
use cf_e2e::live::members::squash;

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

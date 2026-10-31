//! How the case reads a harness's record, as lines: what a resumed window did with
//! a turn it found cut is said by what the record holds and by how the daemon's own
//! reader reads it.

use cf_e2e::live::ask::{Item, Record};
use cf_e2e::live::members::squash;
use serde_json::Value;

/// An item of a record as a line: who it is from, and what it says.
pub fn line(item: &Item) -> String {
    let said = squash(&item.text, 140);
    format!(
        "{}: {}",
        item.role,
        if said.is_empty() { "(no text)" } else { &said }
    )
}

/// How the daemon's own reader reads the record, as a line.
pub fn reading(read: Option<&Record>) -> String {
    let Some(read) = read else {
        return "the daemon's reader: no record".to_owned();
    };
    let last = read
        .items
        .last()
        .map(|item| Value::String(line(item).chars().take(80).collect()).to_string());
    format!(
        "the daemon's reader: {}, {}, {} items, last {}",
        read.settlement,
        if read.settled {
            "at rest"
        } else {
            "not at rest"
        },
        read.items.len(),
        last.unwrap_or_else(|| "null".to_owned())
    )
}

/// The last `last` items of the record that say something, as a line.
pub fn tail(items: &[Item], last: usize) -> String {
    let said: Vec<&Item> = items.iter().filter(|item| !item.text.is_empty()).collect();
    format!(
        "{} items; the last: {}",
        items.len(),
        said[said.len().saturating_sub(last)..]
            .iter()
            .map(|item| line(item))
            .collect::<Vec<_>>()
            .join(" | ")
    )
}

/// What the record says of the new items, after the record had `before`: nothing
/// when it is as it was.
pub fn change(before: usize, after: &[Item]) -> String {
    if after.len() == before {
        format!("its record: unchanged, {before} items")
    } else {
        format!(
            "its record: {before} → {} items; the new ones: {}",
            after.len(),
            after[before.min(after.len())..]
                .iter()
                .map(line)
                .collect::<Vec<_>>()
                .join(" | ")
        )
    }
}

/// Whether the record holds the message `message` and nothing of an answer to
/// it: the message is the user's, and what follows it is no more than the notes the
/// harness writes itself (`custom` items). This is the turn that is at work and has
/// said nothing, for the harnesses that keep no status of it.
pub fn unanswered(items: &[Item], message: &str) -> bool {
    let Some(at) = items
        .iter()
        .rposition(|item| item.role == "user" && item.text.contains(message))
    else {
        return false;
    };
    items[at + 1..].iter().all(|item| item.role == "custom")
}

/// The reply that answers the message `message` with its own answer `answer`: the first
/// of the assistant's items after the message's user item whose last line is `answer`. A
/// reply may say more before its answer: Devin 3000.11.228 answered, in the one reply of
/// the turn the message began, the turn the resume found cut as well, its own answer the
/// last line (T-813, T-811's early case of 2026-10-27, which a reply that was the answer
/// alone missed). A reply to the cut turn written before the message is none.
pub fn answer_to<'a>(items: &'a [Item], message: &str, answer: &str) -> Option<&'a Item> {
    let at = items
        .iter()
        .rposition(|item| item.role == "user" && item.text.contains(message))?;
    items[at + 1..]
        .iter()
        .find(|item| item.role == "assistant" && last_line(&item.text) == Some(answer))
}

/// What `reply` said before its answer, its last line, as a line: none where the answer
/// was all it said.
pub fn before_answer(reply: &Item) -> Option<String> {
    let said = reply.text.trim_end();
    let before = said[..said.rfind('\n')?].trim();
    (!before.is_empty()).then(|| squash(before, 100))
}

/// The last line of `text` that says something, trimmed.
fn last_line(text: &str) -> Option<&str> {
    text.lines().map(str::trim).rfind(|line| !line.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(role: &str, text: &str) -> Item {
        Item {
            id: String::new(),
            role: role.to_owned(),
            text: text.to_owned(),
            complete: false,
        }
    }

    #[test]
    fn an_item_is_its_role_and_what_it_says_on_one_line_or_that_it_says_nothing() {
        assert_eq!(
            line(&item("user", "Run  this\n command")),
            "user: Run this command"
        );
        assert_eq!(line(&item("assistant", "")), "assistant: (no text)");
        assert_eq!(
            line(&item("assistant", &"x".repeat(300))).len(),
            "assistant: ".len() + 140
        );
    }

    #[test]
    fn the_daemons_reader_says_what_it_read_the_turn_as_and_the_last_item() {
        assert_eq!(reading(None), "the daemon's reader: no record");
        let read = Record {
            items: vec![item("user", "hi"), item("assistant", "hello")],
            settled: false,
            settlement: "in-flight".to_owned(),
            failed: Value::Null,
            quota: Value::Null,
            agents_pending: false,
            jobs_pending: 0,
            jobs_retired: 0,
            monitors_noticed: 0,
        };
        assert_eq!(
            reading(Some(&read)),
            "the daemon's reader: in-flight, not at rest, 2 items, last \"assistant: hello\""
        );
        let empty = Record {
            items: vec![],
            settled: true,
            settlement: "settled".to_owned(),
            ..read
        };
        assert_eq!(
            reading(Some(&empty)),
            "the daemon's reader: settled, at rest, 0 items, last null"
        );
    }

    #[test]
    fn the_tail_is_the_last_items_that_say_something_and_how_many_there_are() {
        let items = [
            item("user", "one"),
            item("custom", ""),
            item("assistant", "two"),
            item("user", "three"),
            item("assistant", "four"),
        ];
        assert_eq!(
            tail(&items, 3),
            "5 items; the last: assistant: two | user: three | assistant: four"
        );
        assert_eq!(tail(&[], 3), "0 items; the last: ");
    }

    #[test]
    fn a_record_that_grew_is_told_by_what_was_added_and_one_that_did_not_is_unchanged() {
        let items = [
            item("user", "a"),
            item("assistant", "b"),
            item("user", "AFTER"),
        ];
        assert_eq!(change(3, &items), "its record: unchanged, 3 items");
        assert_eq!(
            change(1, &items),
            "its record: 1 → 3 items; the new ones: assistant: b | user: AFTER"
        );
        // A record that shrank has no new ones to tell.
        assert_eq!(change(5, &items), "its record: 5 → 3 items; the new ones: ");
    }

    #[test]
    fn a_turn_has_said_nothing_when_the_message_is_in_the_record_with_no_answer_after_it() {
        let message = "Count from 1 to 40";
        assert!(!unanswered(&[], message));
        let asked = [item("user", "Count from 1 to 40, then COUNTED")];
        assert!(unanswered(&asked, message));
        // The harness's own notes are not an answer.
        let noted = [asked[0].clone(), item("custom", "model changed")];
        assert!(unanswered(&noted, message));
        let answered = [asked[0].clone(), item("assistant", "1")];
        assert!(!unanswered(&answered, message));
        // The newest asking is the one that counts.
        let again = [asked[0].clone(), item("assistant", "1"), asked[0].clone()];
        assert!(unanswered(&again, message));
        assert!(!unanswered(&[item("user", "something else")], message));
    }

    #[test]
    fn a_message_is_answered_by_a_reply_after_it_that_ends_in_its_answer_whatever_it_said_before() {
        let cut = item("user", "Count from 1 to 40, then COUNTED");
        let after = item("user", "Reply with exactly one line: AFTER");
        // T-811's early case on Devin 3000.11.228: one reply answered the cut turn and the
        // message, the message's answer last.
        let both = item("assistant", "1\n2\n40\nCOUNTED\nAFTER\n");
        let items = [cut.clone(), after.clone(), both.clone()];
        assert_eq!(answer_to(&items, "AFTER", "AFTER"), Some(&both));
        assert_eq!(
            before_answer(&both).as_deref(),
            Some("1 2 40 COUNTED"),
            "what it said first"
        );
        let alone = item("assistant", "AFTER");
        assert_eq!(
            answer_to(
                &[cut.clone(), after.clone(), alone.clone()],
                "AFTER",
                "AFTER"
            ),
            Some(&alone)
        );
        assert_eq!(before_answer(&alone), None);
        // No answer: the cut turn's words alone, a reply that only mentions the word, or a
        // reply written before the message.
        let counted = item("assistant", "1\n2\n40\nCOUNTED");
        assert_eq!(
            answer_to(&[cut.clone(), after.clone(), counted], "AFTER", "AFTER"),
            None
        );
        let mentions = item("assistant", "AFTER is the word you asked for.");
        assert_eq!(
            answer_to(&[after.clone(), mentions], "AFTER", "AFTER"),
            None
        );
        assert_eq!(answer_to(&[alone, after.clone()], "AFTER", "AFTER"), None);
        assert_eq!(answer_to(&[cut], "AFTER", "AFTER"), None);
    }
}

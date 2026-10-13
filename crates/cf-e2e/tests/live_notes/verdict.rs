//! What the chief's own record says of the notes it was sent: how many times the
//! start of each note's header is in a message of its record. A note that went in
//! once is the check's pass; one that is not there was never taken, and one that is
//! there twice was pasted again while the window still had the first (the field bug
//! of 2026-10-13: m-186, m-186, m-187, m-187). A note pasted twice into one prompt
//! is told twice too, though it is one message of the record.

use cf_e2e::live::ask::Item;
use cf_e2e::live::members::plural;
use cf_e2e::live::seconds;
use cf_e2e::live::trace::at_ms;
use serde_json::Value;

/// A note ConsensFlow sent the chief: its id, and what it is, in words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    pub id: i64,
    pub what: &'static str,
}

impl Note {
    /// The start of its header, as the daemon proves an arrival by it
    /// (`marker_of`): the space after the id keeps m-1 from being m-12.
    pub fn marker(&self) -> String {
        format!("[ConsensFlow m-{} ", self.id)
    }
}

/// What the record says of one note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seen {
    pub note: Note,
    /// How many times its header starts in what the chief was given.
    pub times: usize,
    /// In how many messages of the record.
    pub messages: usize,
}

impl Seen {
    /// Whether the note went in once, as it should have.
    pub fn held(&self) -> bool {
        self.times == 1
    }

    /// What the record says, in words.
    pub fn said(&self) -> String {
        let id = self.note.id;
        match (self.times, self.messages) {
            (0, _) => format!("m-{id} is in no message of the chief's record"),
            (1, _) => format!("m-{id} is in the chief's record once"),
            (times, 1) => {
                format!("m-{id} is in the chief's record {times} times, in one message: pasted into one prompt together")
            }
            (times, messages) => {
                format!("m-{id} is in the chief's record {times} times, in {messages} messages: pasted again")
            }
        }
    }
}

/// What the user's messages of `given` hold of each of `notes`, in the order of
/// `notes`. What a tool printed, or the chief wrote (it may quote a note), is not
/// what it was given.
pub fn seen(notes: &[Note], given: &[Item]) -> Vec<Seen> {
    notes
        .iter()
        .map(|note| {
            let marker = note.marker();
            let counts: Vec<usize> = given
                .iter()
                .filter(|item| item.role == "user")
                .map(|item| item.text.matches(&marker).count())
                .filter(|count| *count > 0)
                .collect();
            Seen {
                note: note.clone(),
                times: counts.iter().sum(),
                messages: counts.len(),
            }
        })
        .collect()
}

/// When the daemon first pasted any of `notes`, from what it wrote (a
/// `delivery.begun` of one), in milliseconds since the epoch: none if it pasted none.
pub fn first_paste(events: &[Value], notes: &[Note]) -> Option<i64> {
    events
        .iter()
        .filter(|event| event["kind"] == "delivery.begun")
        .filter(|event| notes.iter().any(|note| event["data"]["message"] == note.id))
        .filter_map(at_ms)
        .min()
}

/// The notes the daemon pressed Enter again for, from what it wrote (a
/// `delivery.enter_again` of each time): a paste its window had not sent, or one that
/// a turn ignored the Enter of.
pub fn enter_again(events: &[Value], notes: &[Note]) -> Vec<String> {
    events
        .iter()
        .filter(|event| event["kind"] == "delivery.enter_again")
        .filter_map(|event| {
            let id = event["message"].as_i64()?;
            notes
                .iter()
                .any(|note| note.id == id)
                .then(|| format!("m-{id}"))
        })
        .collect()
}

/// How late the chief's record was with each of `notes`, by what the daemon wrote
/// (`events`): how often it pasted the note, and how long after the first paste the
/// record showed it (`delivery.begun`, `delivery.confirmed`). It is told and not
/// judged: a record that is late is what the daemon now waits for, and how late it
/// was says whether the chief's harness held the note in its turn.
pub fn lateness(events: &[Value], notes: &[Note]) -> Vec<String> {
    let times = |kind: &str, id: i64| -> Vec<i64> {
        events
            .iter()
            .filter(|event| event["kind"] == kind && event["data"]["message"] == id)
            .filter_map(at_ms)
            .collect()
    };
    notes
        .iter()
        .map(|note| {
            let (pasted, shown) = (
                times("delivery.begun", note.id),
                times("delivery.confirmed", note.id),
            );
            let (id, what) = (note.id, note.what);
            let pastes = plural(pasted.len(), "paste");
            match (pasted.first(), shown.first()) {
                (Some(first), Some(shown)) => format!(
                    "m-{id} ({what}): {pastes}; its record showed it {} s after the first",
                    seconds(shown - first)
                ),
                (Some(_), None) => {
                    format!("m-{id} ({what}): {pastes}; its record never showed it")
                }
                (None, _) => format!("m-{id} ({what}): never pasted"),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn item(role: &str, text: &str) -> Item {
        Item {
            id: format!("{role}-{}", text.len()),
            role: role.to_owned(),
            text: text.to_owned(),
            complete: true,
        }
    }

    fn notes() -> Vec<Note> {
        vec![
            Note {
                id: 186,
                what: "the cancel's note",
            },
            Note {
                id: 187,
                what: "the closed window's note",
            },
        ]
    }

    #[test]
    fn a_note_in_one_message_of_the_record_went_in_once() {
        let given = [
            item(
                "user",
                "[ConsensFlow m-186 · T-17 · note from ConsensFlow]\ncancelled",
            ),
            item("assistant", "NOTED"),
            item(
                "user",
                "[ConsensFlow m-187 · T-17 · note from ConsensFlow]\nclosed",
            ),
        ];
        let found = seen(&notes(), &given);
        assert!(found.iter().all(Seen::held), "{found:?}");
        assert_eq!(found[0].said(), "m-186 is in the chief's record once");
    }

    #[test]
    fn a_note_pasted_again_is_in_two_messages_and_told_so_and_its_neighbour_is_not_touched() {
        let given = [
            item(
                "user",
                "[ConsensFlow m-186 | T-17 | note from ConsensFlow]\ncancelled",
            ),
            item(
                "user",
                "[ConsensFlow m-186 | T-17 | note from ConsensFlow]\ncancelled",
            ),
            item(
                "user",
                "[ConsensFlow m-187 | T-17 | note from ConsensFlow]\nclosed",
            ),
        ];
        let found = seen(&notes(), &given);
        assert_eq!(
            (found[0].times, found[0].messages, found[0].held()),
            (2, 2, false)
        );
        assert_eq!(
            found[0].said(),
            "m-186 is in the chief's record 2 times, in 2 messages: pasted again"
        );
        assert!(found[1].held());
    }

    #[test]
    fn two_pastes_in_one_prompt_are_two_times_in_one_message() {
        let both = "[ConsensFlow m-187 · note]\nclosed[ConsensFlow m-187 · note]\nclosed";
        let found = seen(&notes(), &[item("user", both)]);
        assert_eq!((found[1].times, found[1].messages), (2, 1));
        assert_eq!(
            found[1].said(),
            "m-187 is in the chief's record 2 times, in one message: pasted into one prompt together"
        );
    }

    #[test]
    fn a_note_that_never_went_in_is_told_so_and_what_is_not_the_users_is_not_given() {
        // The chief quoted it, and a tool printed it: neither is what the window was given.
        let given = [
            item("assistant", "[ConsensFlow m-186 · T-17 · note]"),
            item("tool", "[ConsensFlow m-187 · T-17 · note]"),
        ];
        let found = seen(&notes(), &given);
        assert!(found.iter().all(|seen| !seen.held() && seen.times == 0));
        assert_eq!(
            found[0].said(),
            "m-186 is in no message of the chief's record"
        );
    }

    #[test]
    fn a_note_is_found_by_its_id_whole_so_m_18_is_not_m_186() {
        let given = [item("user", "[ConsensFlow m-1860 · note]")];
        let found = seen(&notes(), &given);
        assert_eq!(found[0].times, 0);
        assert_eq!(found[0].note.marker(), "[ConsensFlow m-186 ");
    }

    #[test]
    fn the_first_paste_is_the_earliest_a_note_was_pasted_and_none_where_no_note_was() {
        let event = |kind: &str, id: i64, at: &str| json!({ "at": at, "kind": kind, "data": { "message": id } });
        let events = [
            event("delivery.begun", 99, "2026-10-13T09:59:00.000Z"),
            event("delivery.begun", 187, "2026-10-13T10:00:02.000Z"),
            event("delivery.begun", 186, "2026-10-13T10:00:00.000Z"),
            event("delivery.confirmed", 186, "2026-10-13T09:58:00.000Z"),
            event("delivery.begun", 186, "2026-10-13T10:01:00.000Z"),
        ];
        let first = first_paste(&events, &notes()).unwrap();
        assert_eq!(first, at_ms(&events[2]).unwrap());
        assert_eq!(first_paste(&events[..1], &notes()), None);
        assert_eq!(first_paste(&events, &[]), None);
    }

    #[test]
    fn the_notes_pressed_enter_again_for_are_those_of_the_window_events_of_the_run() {
        let events = [
            json!({ "kind": "delivery.enter_again", "message": 187 }),
            json!({ "kind": "delivery.enter_again", "message": 99 }),
            json!({ "kind": "delivery.enter_again", "message": 187 }),
            json!({ "kind": "delivery.held", "message": 186 }),
        ];
        assert_eq!(enter_again(&events, &notes()), ["m-187", "m-187"]);
        assert!(enter_again(&events[3..], &notes()).is_empty());
    }

    #[test]
    fn how_late_a_record_was_is_told_from_the_first_paste_to_the_confirmation_with_how_often_it_was_pasted(
    ) {
        let event = |kind: &str, id: i64, at: &str| json!({ "at": at, "kind": kind, "data": { "message": id } });
        let events = [
            event("delivery.begun", 186, "2026-10-13T10:00:00.000Z"),
            event("delivery.confirmed", 186, "2026-10-13T10:00:01.500Z"),
            event("delivery.begun", 187, "2026-10-13T10:00:02.000Z"),
            event("delivery.retried", 187, "2026-10-13T10:01:02.000Z"),
            event("delivery.begun", 187, "2026-10-13T10:01:30.000Z"),
            event("delivery.confirmed", 187, "2026-10-13T10:01:31.000Z"),
            // Another message's, and one that says no time.
            event("delivery.begun", 99, "2026-10-13T10:00:00.000Z"),
            json!({ "kind": "delivery.begun", "data": { "message": 186 } }),
        ];
        assert_eq!(
            lateness(&events, &notes()),
            [
                "m-186 (the cancel's note): 1 paste; its record showed it 1.5 s after the first",
                "m-187 (the closed window's note): 2 pastes; its record showed it 89.0 s after the first",
            ]
        );
        // A record that never showed it, and a note never pasted.
        let begun = [event("delivery.begun", 186, "2026-10-13T10:00:00.000Z")];
        assert_eq!(
            lateness(&begun, &notes()),
            [
                "m-186 (the cancel's note): 1 paste; its record never showed it",
                "m-187 (the closed window's note): never pasted",
            ]
        );
    }
}

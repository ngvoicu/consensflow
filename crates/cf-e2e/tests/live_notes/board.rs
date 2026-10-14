//! What the check reads off the board and the chief's own record: the notes
//! ConsensFlow sent the chief of a cancel, whether the record confirmed them, the
//! state of the chief's window, and the conversation the chief's harness kept.

use cf_e2e::live::ask::{self, Record};
use cf_e2e::live::members::{session_of, Members};
use cf_e2e::live::Harness;
use cf_e2e::Result;
use serde_json::Value;

use crate::verdict::Note;

/// The notes ConsensFlow sent the chief of the cancel of T-`number`, oldest first:
/// its own, and the closing of the member's window with its job running.
pub fn told(members: &Members, number: i64) -> Result<Vec<Note>> {
    Ok(notes_among(&members.inbox(Some("chief"))?, number))
}

/// The notes of the cancel of T-`number` among the messages of `inbox`: those
/// ConsensFlow wrote itself (no window did), by their words.
fn notes_among(inbox: &[Value], number: i64) -> Vec<Note> {
    let cancelled = format!("cancelled T-{number} ");
    let closed = format!("T-{number}'s window closed with background work");
    let mut found: Vec<Note> = inbox
        .iter()
        .filter(|message| message["kind"] == "note" && message["sender"].is_null())
        .filter_map(|message| {
            let body = message["body"].as_str()?;
            let what = if body.contains(&cancelled) {
                "the cancel's note"
            } else if body.starts_with(&closed) {
                "the closed window's note"
            } else {
                return None;
            };
            Some(Note {
                id: message["id"].as_i64()?,
                what,
            })
        })
        .collect();
    found.sort_by_key(|note| note.id);
    found
}

/// Whether the message `id` of the chief's inbox was confirmed by its record.
pub fn delivered(members: &Members, id: i64) -> Result<bool> {
    Ok(members
        .inbox(Some("chief"))?
        .iter()
        .any(|message| message["id"] == id && message["state"] == "delivered"))
}

/// What the state of the chief's window is, as the board says it.
pub fn chief_state(members: &Members) -> Result<String> {
    Ok(members.lane("chief")?["activity"]["state"]
        .as_str()
        .unwrap_or_default()
        .to_owned())
}

/// What the chief's own harness recorded of its conversation, as the daemon reads
/// it: Claude Code's conversation is named by its window's launch, Devin names its
/// own, and its store says which is the newest of the run's folder.
pub fn record_of(chief: Harness, members: &Members) -> Result<Option<Record>> {
    let env = members.rig().environment();
    let session = if chief == Harness::Devin {
        ask::newest_session(chief.kind(), members.folder(), 0, env)?
    } else {
        let pane = format!("p{}-chief", members.project().id());
        members
            .rig()
            .open_frames()
            .into_iter()
            .rev()
            .find(|frame| frame["id"] == pane.as_str())
            .and_then(|frame| session_of(&frame))
    };
    match session {
        Some(session) => ask::records(chief.kind(), &session, env),
        None => Ok(None),
    }
}

/// The notes the daemon pasted again, from what it wrote: a `delivery.retried` of
/// each, and why.
pub fn retried(events: &[Value], notes: &[Note]) -> Vec<String> {
    events
        .iter()
        .filter(|event| event["kind"] == "delivery.retried")
        .filter_map(|event| {
            let id = event["data"]["message"].as_i64()?;
            notes.iter().any(|note| note.id == id).then(|| {
                format!(
                    "m-{id}: {}",
                    event["data"]["reason"].as_str().unwrap_or("no reason")
                )
            })
        })
        .collect()
}

/// The notes in words, as the case says them.
pub fn named(notes: &[Note]) -> String {
    notes
        .iter()
        .map(|note| format!("m-{} ({})", note.id, note.what))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn the_notes_the_check_looks_for_are_those_of_the_cancel_and_of_the_closed_window_by_their_words(
    ) {
        let note = |id: i64, body: &str, sender: Value| json!({ "id": id, "kind": "note", "sender": sender, "body": body, "state": "queued" });
        let inbox = vec![
            note(
                9,
                "T-3's window closed with background work still running: node job",
                Value::Null,
            ),
            note(
                7,
                "@human cancelled T-3 (the parser): @worker's window was stopped.",
                Value::Null,
            ),
            // Another task's, one a window wrote, and what is no note.
            note(
                5,
                "@human cancelled T-4 (the lexer): @worker's window was stopped.",
                Value::Null,
            ),
            note(6, "@human cancelled T-3 (copied)", json!("zeus")),
            json!({ "id": 8, "kind": "result", "sender": null, "body": "cancelled T-3 (x)" }),
        ];
        let ids = |number: i64| -> Vec<(i64, &'static str)> {
            notes_among(&inbox, number)
                .into_iter()
                .map(|note| (note.id, note.what))
                .collect()
        };
        assert_eq!(
            ids(3),
            [(7, "the cancel's note"), (9, "the closed window's note")]
        );
        assert_eq!(ids(4), [(5, "the cancel's note")]);
        assert!(ids(5).is_empty());
    }

    #[test]
    fn the_notes_pasted_again_are_those_the_daemon_retried_and_say_why() {
        let events = vec![
            json!({ "kind": "delivery.retried", "data": { "message": 186, "reason": "the harness record never showed it" } }),
            json!({ "kind": "delivery.retried", "data": { "message": 99, "reason": "other" } }),
            json!({ "kind": "delivery.begun", "data": { "message": 187 } }),
        ];
        let notes = [Note {
            id: 186,
            what: "the cancel's note",
        }];
        assert_eq!(
            retried(&events, &notes),
            ["m-186: the harness record never showed it"]
        );
        assert!(retried(&events, &[]).is_empty());
    }

    #[test]
    fn the_notes_are_named_by_id_and_what_they_are() {
        let notes = [
            Note {
                id: 186,
                what: "the cancel's note",
            },
            Note {
                id: 187,
                what: "the closed window's note",
            },
        ];
        assert_eq!(
            named(&notes),
            "m-186 (the cancel's note), m-187 (the closed window's note)"
        );
        assert_eq!(named(&[]), "");
    }
}

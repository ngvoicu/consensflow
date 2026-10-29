//! A message Claude Code takes into a turn under way. Pasted while its agent works, a
//! message is queued (`enqueue`), and at the next tool's result Claude Code takes it out of
//! the queue (`remove`, `"reason": "absorbed_mid_turn"`) and into the turn: an attachment,
//! `queued_command` of `"commandMode": "prompt"`, from a human (`origin.kind`), shown to the
//! model as "the user sent a new message while you were working". No user's record is
//! written of it. The reader makes of it the user's item, so the window's record shows the
//! message that arrived (an arrival is proven by the user's item that is its paste), and
//! it begins no turn and ends none: the turn it went into goes on (T-793, CMT-02; the
//! chief's m-4182 and m-4697 were pasted twice, 2026-10-26).
//!
//! The records are the field case's (Claude Code 2.1.310, the chief's transcript, lines
//! 58177 to 58195), with what nothing reads left out and the message's words made up.

use super::*;
use crate::shared::record::reading::{Item, Role};

/// The message as Claude Code holds it: the paste, in the wrapper it put it in.
const PASTE: &str = "<pasted_content id=\"aead\">\n[ConsensFlow m-4697 · T-708 · question from @khepri-velvet-reef]\nMay I clean up my test folder?\n\nAnswer with this command, run in your shell: cf answer m-4697 \"…\"\n</pasted_content id=\"aead\">";

/// The human's turn the paste went into, typed.
fn typed(uuid: &str, parent: Value, words: &str, at: &str) -> Value {
    having(
        user(uuid, parent, json!(words)),
        json!({ "promptSource": "typed", "timestamp": at }),
    )
}

/// An operation on the queue, of the paste, as 2.1.310 writes it.
fn queued(operation: &str, at: &str) -> Value {
    having(
        queue(operation, json!(PASTE)),
        json!({ "timestamp": at, "sessionId": SESSION }),
    )
}

/// The attachment that takes the message `prompt` into the turn, the child of `parent`.
fn absorbed_as(uuid: &str, parent: &str, prompt: Value) -> Value {
    of_session(json!({
        "type": "attachment", "uuid": uuid, "parentUuid": parent,
        "timestamp": "2026-10-26T17:07:26.266Z",
        "attachment": {
            "type": "queued_command",
            "prompt": prompt,
            "source_uuid": "585b5bdb-337d-47ce-b8b9-b317e5d1f860",
            "delivery_id": "04189ab5-f03e-4d77-a3be-fec9cf234a02",
            "commandMode": "prompt",
            "origin": { "kind": "human" },
            "timestamp": "2026-10-26T17:07:26.266Z",
            "humanTurn": true,
        },
        "rendered": [{ "content": "<system-reminder>\nThe user sent a new message while you were working:\n…" }],
        "renderedRole": "system",
    }))
}

/// The attachment of the paste.
pub(super) fn absorbed() -> Value {
    absorbed_as("q1", "r1", json!(PASTE))
}

/// The field case: the human's turn, the paste queued while it works, the tool's result
/// the paste is taken in at, the attachment, and the answer that ends the turn.
pub(super) fn field_case() -> Vec<Value> {
    let call = having(
        assistant(
            "a1",
            "u1",
            json!("m1"),
            json!([tool_use(json!("t1"))]),
            json!("tool_use"),
        ),
        json!({ "timestamp": "2026-10-26T17:07:29.582Z" }),
    );
    let result = having(
        user(
            "r1",
            json!("a1"),
            json!([tool_result(json!("t1"), json!("19:07"))]),
        ),
        json!({ "timestamp": "2026-10-26T17:07:29.731Z" }),
    );
    let removed = having(
        queued("remove", "2026-10-26T17:07:29.736Z"),
        json!({
            "reason": "absorbed_mid_turn",
            "commandUuid": "585b5bdb-337d-47ce-b8b9-b317e5d1f860",
            "deliveryId": "04189ab5-f03e-4d77-a3be-fec9cf234a02",
        }),
    );
    let answered = having(
        assistant(
            "a2",
            "q1",
            json!("m2"),
            json!([text(json!("Yes, clean it up."))]),
            json!("end_turn"),
        ),
        json!({ "timestamp": "2026-10-26T17:07:40.000Z" }),
    );
    vec![
        typed(
            "u1",
            Value::Null,
            "check B's progress",
            "2026-10-26T17:07:25.560Z",
        ),
        queued("enqueue", "2026-10-26T17:07:26.267Z"),
        call,
        removed,
        result,
        absorbed(),
        answered,
        duration("d1", "a2"),
    ]
}

/// The place of the attachment in the field case.
pub(super) const AT: usize = 5;

/// What one look at `records` reads: its items, and what it says of the turn.
fn read(records: &[Value]) -> (Vec<Item>, Settlement, bool) {
    let mut stage = Stage::new();
    stage.write(records);
    match &*stage.reader.look(&Options::default(), 0) {
        Reading::Known(record) => (record.items.clone(), record.settlement, record.in_flight),
        Reading::Unknown(reason) | Reading::Missing(reason) => panic!("unknown: {reason}"),
    }
}

/// The user's items of a reading, by id and text.
fn users(items: &[Item]) -> Vec<(String, String)> {
    items
        .iter()
        .filter(|item| item.role == Role::User)
        .map(|item| (item.id.to_string(), item.text.to_string()))
        .collect()
}

#[test]
fn a_message_claude_code_takes_into_a_turn_under_way_is_the_user_s_item_in_its_place() {
    let records = field_case();
    let (items, ..) = read(&records);
    assert_eq!(
        users(&items),
        [
            ("u1".to_owned(), "check B's progress".to_owned()),
            ("q1".to_owned(), PASTE.to_owned()),
        ]
    );
    // After the tool's result it was taken in at, and before the answer that follows it;
    // its time is the record's.
    let ids: Vec<&str> = items.iter().map(|item| &*item.id).collect();
    assert_eq!(ids, ["u1", "m1", "t1", "q1", "m2"]);
    let message = &items[3];
    assert_eq!(message.at, Some(json!("2026-10-26T17:07:26.266Z")));
    assert!(message.complete && !message.commentary && !message.compaction);
    // From the moment it is written, before any answer.
    let (items, settlement, _) = read(&records[..=AT]);
    assert_eq!(users(&items).last().unwrap().0, "q1");
    assert_eq!(settlement, Settlement::InFlight);
}

#[test]
fn the_message_taken_in_begins_no_turn_and_ends_none() {
    // Each look, from the first record on, says of the turn what it says without the
    // attachment: in flight while the turn works, settled once it ended.
    let records = field_case();
    let mut without = records.clone();
    without.remove(AT);
    for end in 1..=records.len() {
        let with = read(&records[..end]);
        let kept = if end > AT { end - 1 } else { end };
        let other = read(&without[..kept]);
        assert_eq!((with.1, with.2), (other.1, other.2), "{end} records");
        let mut items = with.0;
        items.retain(|item| &*item.id != "q1");
        assert_eq!(items, other.0, "{end} records");
    }
    assert_eq!(read(&records).1, Settlement::Settled);
}

#[test]
fn a_message_with_pictures_is_its_words_as_a_user_s_message_with_pictures_is() {
    // A paste that names a picture by its path: Claude Code attaches the picture, and its
    // text says where (2.1.306, `imagePasteIds`).
    let words = "[Image #79]\n\n<pasted_content id=\"aead\">\n[ConsensFlow m-12 · note from @human]\nSee the picture.\n</pasted_content id=\"aead\">";
    let prompt = json!([
        text(json!(words)),
        { "type": "image", "source": { "type": "base64", "media_type": "image/png", "data": "iVBORw0KGgo=" } },
    ]);
    let mut records = field_case();
    records[AT] = in_attachment(
        absorbed_as("q1", "r1", prompt),
        "imagePasteIds",
        Some(json!([79])),
    );
    let (items, ..) = read(&records);
    assert_eq!(users(&items)[1], ("q1".to_owned(), words.to_owned()));
}

#[test]
fn what_another_session_or_claude_code_itself_brings_into_a_turn_is_no_user_s_item() {
    let records = field_case();
    let mut without = records.clone();
    without.remove(AT);
    let unread = read(&without);
    let not_a_human_s = [
        // A message of another agent of the session's, or a subagent's hand-back.
        in_attachment(
            having(absorbed(), json!({ "isMeta": true })),
            "origin",
            Some(json!({ "kind": "peer", "from": "fix-ledger" })),
        ),
        // Claude Code's own word to go on.
        in_attachment(
            absorbed(),
            "origin",
            Some(json!({ "kind": "auto-continuation" })),
        ),
        // One that says nothing of where it came from.
        in_attachment(absorbed(), "origin", None),
        // A notification of a job's end, which ends the job and is no message.
        in_attachment(
            in_attachment(absorbed(), "origin", None),
            "commandMode",
            Some(json!("task-notification")),
        ),
        // Of a subagent's conversation, or of another session.
        having(absorbed(), json!({ "isSidechain": true })),
        having(absorbed(), json!({ "sessionId": "other" })),
        // With no words, or no id to be an item by.
        in_attachment(absorbed(), "prompt", Some(json!(""))),
        lacking(absorbed(), "uuid"),
        having(absorbed(), json!({ "uuid": "" })),
    ];
    for record in not_a_human_s {
        let mut records = records.clone();
        records[AT] = record.clone();
        let (items, settlement, in_flight) = read(&records);
        assert_eq!(users(&items), users(&unread.0), "{record}");
        assert_eq!((settlement, in_flight), (unread.1, unread.2), "{record}");
    }
}

#[test]
fn a_message_taken_in_that_an_interrupt_puts_back_and_that_is_sent_again_is_both_turns_items() {
    // Interrupted a moment after it took the message in, Claude Code queues it again,
    // and it is sent as a turn of its own (2.1.278 to 2.1.311: `requeueIfInterruptedWithinMs`;
    // the attachment stays in the conversation, the interrupt its child).
    let mut records = field_case();
    records.truncate(AT + 1);
    let interrupted = user(
        "i1",
        json!("q1"),
        json!([text(json!("[Request interrupted by user]"))]),
    );
    let again = typed("u2", json!("i1"), PASTE, "2026-10-26T17:07:33.000Z");
    let answered = assistant(
        "a2",
        "u2",
        json!("m2"),
        json!([text(json!("Yes."))]),
        json!("end_turn"),
    );
    records.push(interrupted);
    let (_, settlement, in_flight) = read(&records);
    assert_eq!((settlement, in_flight), (Settlement::Settled, false));
    records.extend([again, answered, duration("d1", "a2")]);
    let (items, settlement, _) = read(&records);
    let pasted: Vec<String> = users(&items)
        .into_iter()
        .filter(|(_, words)| words == PASTE)
        .map(|(id, _)| id)
        .collect();
    assert_eq!(pasted, ["q1", "u2"]);
    assert_eq!(settlement, Settlement::Settled);
}

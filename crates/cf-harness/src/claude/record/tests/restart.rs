//! A conversation Claude Code goes on with in a new process (`--resume`): the
//! queue of messages is the process's own, held in memory, so what the process
//! before had queued and not begun went with it, and no record says so. The
//! process that resumes says it is one: it closes the turn the old one was cut
//! in with a message of its own (`<synthetic>`, `No response requested.`), and a
//! `SessionStart` hook, where the human's settings have one, runs as
//! `SessionStart:resume`. Either empties the queue.
//!
//! The field report (poker-lab, ConsensFlow 3.0.4, Claude Code 2.1.302, T-442):
//! the worker's window was closed while a notification of a job ConsensFlow had
//! ended was queued, and the chief's reopen resumed it. The reopen's turn ended,
//! and the task stayed working for good: the queued notification was read as
//! still to come. `restart-left-a-queued-notification.jsonl` is that record's
//! shape with made-up words (`tests/fixtures/claude/README.md`).

use super::*;
use crate::shared::record::reading::Role;

/// The records of T-442's shape, for the session of these tests.
fn t442() -> Vec<Value> {
    include_str!("../../../../tests/fixtures/claude/restart-left-a-queued-notification.jsonl")
        .replace("$SESSION", SESSION)
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

/// The place of the first record of `records` that `found` finds.
fn place(records: &[Value], found: impl Fn(&Value) -> bool) -> usize {
    records.iter().position(found).expect("the record is there")
}

/// Whether `record` is an enqueue of a notification of the job `job` that says
/// `status`.
fn queued_notice(record: &Value, job: &str, status: &str) -> bool {
    record["operation"] == "enqueue"
        && record["content"].as_str().is_some_and(|content| {
            content.contains(&format!("<task-id>{job}</task-id>"))
                && content.contains(&format!("<status>{status}</status>"))
        })
}

/// What a look at `records` reads.
fn record_of(records: &[Value]) -> Record {
    let mut stage = Stage::new();
    stage.write(records);
    match &*stage.reader.look(&Options::default(), 0) {
        Reading::Known(record) => record.clone(),
        Reading::Unknown(reason) => panic!("unknown: {reason}"),
    }
}

/// What the reopen's turn wrote: its last assistant's message.
const ANSWER: &str =
    "With the cache warm the import took 12 minutes; the slowest steps are index, write and parse.";

#[test]
fn a_notification_the_old_process_had_queued_holds_no_turn_of_the_process_that_resumed() {
    let records = t442();
    // The notification of the job ConsensFlow ended, queued at 18:30:09 and gone
    // with the process at the close: the record without its enqueue was read as
    // settled before a restart was read at all.
    let stale = place(&records, |record| {
        queued_notice(record, "bqehk4ozo", "failed")
    });
    let mut without = records.clone();
    without.remove(stale);
    assert_eq!(settlement_of(&without), Settlement::Settled);

    let read = record_of(&records);
    assert_eq!(
        read.settlement,
        Settlement::Settled,
        "the reopen's turn ended"
    );
    let last = read.items.last().unwrap();
    assert_eq!(
        (last.role, &*last.text, last.complete),
        (Role::Assistant, ANSWER, true)
    );
}

#[test]
fn the_record_says_the_turn_as_it_was_written_through_the_restart() {
    let records = t442();
    let at = |found: &dyn Fn(&Value) -> bool| place(&records, found);
    let uuid = |uuid: &'static str| move |record: &Value| record["uuid"] == uuid;
    // The notification turn of 18:30:06 ended; the one of 18:30:09 was cut by
    // the window's close; the restart closed it and began the reopen's, which
    // ended at 19:25:10.
    let ended = at(&uuid("be5ee0e5-3200-4b65-9e03-05b25e29b0e3"));
    let cut = at(&uuid("0f115950-6cac-4f6e-a5c4-65d51bf1d2be"));
    let reopened = at(&uuid("e0e24d7e-bb33-46e1-ba30-3a8e813f4d9c"));
    let batches: Vec<&[Value]> = vec![
        &records[..=ended],
        &records[ended + 1..=cut],
        &records[cut + 1..=reopened],
        &records[reopened + 1..],
    ];
    let said: Vec<String> = looks(&batches)
        .iter()
        .map(|look| look.rsplit("\"settlement\":").next().unwrap().to_owned())
        .collect();
    assert_eq!(
        said,
        [
            "\"settled\"}",
            "\"in-flight\"}",
            "\"in-flight\"}",
            "\"settled\"}"
        ]
    );
}

/// A message queued, the turn that answered it, and `then`: whether the turn is
/// settled after.
fn queued_then(then: Vec<Value>) -> Settlement {
    let mut records = vec![
        hello(),
        queue("enqueue", json!("waiting")),
        answer("a1", "u1"),
        duration("d1", "a1"),
    ];
    records.extend(then);
    settlement_of(&records)
}

/// The attachment a `SessionStart` hook's run writes, as `name` names it.
pub(super) fn session_start(uuid: &str, parent: &str, name: &str) -> Value {
    of_session(json!({
        "type": "attachment", "uuid": uuid, "parentUuid": parent,
        "attachment": {
            "type": "hook_success", "hookName": name, "hookEvent": "SessionStart",
            "toolUseID": "hook-1", "exitCode": 0, "durationMs": 5,
            "content": "", "stdout": "{}\n", "command": "true",
        },
    }))
}

/// Claude Code's close of the turn a restart found cut: `uuid`, a child of `parent`.
pub(super) fn closed(uuid: &str, parent: &str) -> Value {
    let close = assistant(
        uuid,
        parent,
        json!("7f3c9a2e-close"),
        json!([text(json!("No response requested."))]),
        json!("stop_sequence"),
    );
    having(
        in_message(close, "model", Some(json!("<synthetic>"))),
        json!({ "isApiErrorMessage": false }),
    )
}

#[test]
fn a_resume_empties_the_queue_and_nothing_else_of_claudes_own_does() {
    assert_eq!(queued_then(vec![]), Settlement::InFlight);
    let resumed = |name: &str| queued_then(vec![session_start("h1", "d1", name)]);
    assert_eq!(resumed("SessionStart:resume"), Settlement::Settled);
    // A session that starts afresh, after a `/clear` or a compaction, is the
    // same process, with the same queue.
    for name in [
        "SessionStart:startup",
        "SessionStart:clear",
        "SessionStart:compact",
        "SessionStart",
    ] {
        assert_eq!(resumed(name), Settlement::InFlight, "{name}");
    }
    // Another conversation's, or a sidechain's, says nothing of this one.
    for other in [
        json!({ "sessionId": "other" }),
        json!({ "isSidechain": true }),
    ] {
        let start = having(
            session_start("h1", "d1", "SessionStart:resume"),
            other.clone(),
        );
        assert_eq!(queued_then(vec![start]), Settlement::InFlight, "{other}");
    }
    let named_elsewhere = in_attachment(
        session_start("h1", "d1", "SessionStart:resume"),
        "hookEvent",
        Some(json!("Stop")),
    );
    assert_eq!(queued_then(vec![named_elsewhere]), Settlement::InFlight);

    // The close of a cut turn says a restart too, and the turn that follows
    // ends as any.
    let next = |close: Value| {
        let again = assistant(
            "a3",
            "u3",
            json!("m3"),
            json!([text(json!("Done"))]),
            json!("end_turn"),
        );
        queued_then(vec![
            user("u2", json!("d1"), json!("Cut")),
            close,
            user("u3", json!("c1"), json!("Again")),
            again,
            duration("d3", "a3"),
        ])
    };
    assert_eq!(next(closed("c1", "u2")), Settlement::Settled);
    // A model's own words, or the API's refusal, that say the same are no close.
    let said_by_the_model = in_message(closed("c1", "u2"), "model", Some(json!("claude-opus-5-5")));
    let refused = having(closed("c1", "u2"), json!({ "isApiErrorMessage": true }));
    let other_words = in_message(
        closed("c1", "u2"),
        "content",
        Some(json!([text(json!("No response needed."))])),
    );
    for close in [said_by_the_model, refused, other_words] {
        assert_eq!(next(close.clone()), Settlement::InFlight, "{close}");
    }
}

#[test]
fn the_close_of_a_cut_turn_is_no_part_of_any_answer() {
    let read = record_of(&t442());
    let close = read
        .items
        .iter()
        .find(|item| &*item.text == "No response requested.")
        .expect("the close is an item");
    assert_eq!((close.role, close.commentary), (Role::Assistant, true));
    // Every other message is the agent's own words.
    assert!(read
        .items
        .iter()
        .filter(|item| item.id != close.id)
        .all(|item| !item.commentary));
    // The same words the model wrote, or the API's refusal says, are not Claude's close.
    let words = |close: Value| {
        record_of(&[hello(), close])
            .items
            .iter()
            .any(|item| item.commentary)
    };
    let said_by_the_model = in_message(closed("c1", "u1"), "model", Some(json!("claude-opus-5-5")));
    assert!(!words(said_by_the_model));
    assert!(!words(having(
        closed("c1", "u1"),
        json!({ "isApiErrorMessage": true })
    )));
    assert!(words(closed("c1", "u1")));
}

//! A conversation Claude Code goes on with in a new process (`--resume`): the
//! queue of messages is the process's own, held in memory, so what the process
//! before had queued and not begun went with it, and no record says so.
//! ConsensFlow knows where the new process began, as it closed the window the
//! old one ran in before it started the new one: a look is told the transcript's
//! length then (`Options::relaunched_at`), and the queue is empty from there on,
//! what the new process queued after it kept. The record's own signs of a resume
//! (Claude's close of a cut turn, a `SessionStart:resume` hook's run) say
//! nothing of it: Claude writes them for a `/resume` in the window too, which
//! keeps its queue, and writes the hook's run after what the new process queued
//! first.
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

/// A look's options that say a process began where the transcript of the
/// records before `from` ends: ConsensFlow opened the window again then.
fn relaunched(records: &[Value], from: usize) -> Options {
    Options {
        relaunched_at: Some(Stage::lines(&records[..from]).len() as u64),
        ..Options::default()
    }
}

/// What a look told `options` reads of `records`.
fn record_told(records: &[Value], options: &Options) -> Record {
    let mut stage = Stage::new();
    stage.write(records);
    known(&stage.look_told(options))
}

/// What a look at `records` reads.
fn record_of(records: &[Value]) -> Record {
    record_told(records, &Options::default())
}

fn known(reading: &Reading) -> Record {
    match reading {
        Reading::Known(record) => record.clone(),
        Reading::Unknown(reason) => panic!("unknown: {reason}"),
    }
}

/// What a reader says of the turn after each record of `records` is written,
/// one at a time and one look each, a letter a look: `S` settled, `F` in
/// flight, `U` unknown. Its looks are told, once the records before
/// `relaunch` are written, that a process began there: the first of them is
/// the look the window takes as it is opened again.
fn after_each(records: &[Value], relaunch: Option<usize>) -> String {
    let mut stage = Stage::new();
    let told = relaunch.map(|from| relaunched(records, from));
    (1..=records.len())
        .map(|written| {
            if written == 1 {
                stage.write(&records[..1]);
            } else {
                stage.append(&records[written - 1..written]);
            }
            let options = match (&told, relaunch) {
                (Some(told), Some(from)) if written >= from => told.clone(),
                _ => Options::default(),
            };
            match known(&stage.look_told(&options)).settlement {
                Settlement::Settled => 'S',
                Settlement::InFlight => 'F',
                Settlement::Unknown => 'U',
            }
        })
        .collect()
}

/// What the reopen's turn wrote: its last assistant's message.
const ANSWER: &str =
    "With the cache warm the import took 12 minutes; the slowest steps are index, write and parse.";

#[test]
fn a_notification_the_closed_process_had_queued_holds_no_turn_after_the_relaunch() {
    let records = t442();
    // The notification of the job ConsensFlow ended, queued at 18:30:09 and gone
    // with the process at the close: the record without its enqueue is read as
    // settled with nothing told.
    let stale = place(&records, |record| {
        queued_notice(record, "bqehk4ozo", "failed")
    });
    let mut without = records.clone();
    without.remove(stale);
    assert_eq!(settlement_of(&without), Settlement::Settled);
    // With it, the record alone says the message is still to come: Claude's
    // close of the cut turn and its resume hook's run are no word on it.
    assert_eq!(settlement_of(&records), Settlement::InFlight);

    // The reopen started a process at 18:30:59: what it wrote first is the job's
    // notification again (stopped), queued at 18:31:00.
    let relaunch = place(&records, |record| {
        queued_notice(record, "bqehk4ozo", "stopped")
    });
    let read = record_told(&records, &relaunched(&records, relaunch));
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
    // m-2081's turn is in flight from its message on, look after look, and
    // ends where its record says so, not before.
    let marker = place(&records, |record| {
        record["message"]["content"]
            .as_str()
            .is_some_and(|text| text.starts_with("[ConsensFlow m-2081 "))
    });
    // Its end: the stop hooks' summary at 19:25:10, before the turn's duration.
    let end = records
        .iter()
        .rposition(|record| record["subtype"] == "stop_hook_summary")
        .unwrap();
    let said = after_each(&records, Some(relaunch));
    assert!(
        said[marker..end].chars().all(|said| said == 'F'),
        "{}",
        &said[marker..end]
    );
    assert!(
        said[end..].chars().all(|said| said == 'S'),
        "{}",
        &said[end..]
    );
}

/// A turn that ended at its stop hooks' summary (`s1`), as Claude Code writes
/// one with `-p`, and what its process wrote of itself as it went: `bookkeeping`.
fn ended_then(bookkeeping: &[&str]) -> Vec<Value> {
    let mut records = vec![
        hello(),
        answer("a1", "u1"),
        attachment("t1", "a1"),
        summary("s1", "t1"),
    ];
    records.extend(
        bookkeeping
            .iter()
            .map(|kind| json!({ "type": kind, "sessionId": SESSION })),
    );
    records
}

/// What a process ConsensFlow started on the conversation writes as it takes the
/// message it was given, a hook of the human's run as it resumed, in the order
/// Claude Code 2.1.301 wrote them: the message queued and taken, the hook's
/// run, the prompt; then its answer and its end.
fn taken_after_a_resume(message: &str) -> Vec<Value> {
    let prompt = having(
        user("u2", json!("h1"), json!(message)),
        json!({ "promptSource": "sdk" }),
    );
    let reply = assistant(
        "a2",
        "t2",
        json!("m2"),
        json!([text(json!("PELICAN"))]),
        json!("end_turn"),
    );
    vec![
        queue("enqueue", json!(message)),
        queue("dequeue", Value::Null),
        session_start("h1", "s1", "SessionStart:resume"),
        prompt,
        attachment("t2", "u2"),
        reply,
        summary("s2", "a2"),
    ]
}

#[test]
fn what_the_new_process_queued_before_its_resume_hook_ran_holds_its_turn_look_after_look() {
    // Two records of 2.1.301 on this Mac (`3317ebf8…`, lines 513–519, and
    // `384926cd…`, lines 52–60): the new process queued its message and took it
    // before the hook's run was written. Its turn is in flight from the message
    // on, at every look, until its end.
    let orders = [
        ("3317ebf8", vec!["last-prompt", "cost-state"]),
        ("384926cd", vec!["last-prompt", "atis-latch", "cost-state"]),
    ];
    for (record, bookkeeping) in orders {
        let mut records = ended_then(&bookkeeping);
        let relaunch = records.len();
        records.extend(taken_after_a_resume("What word did I ask you to remember?"));
        let before = "FFFS".to_owned() + &"S".repeat(bookkeeping.len());
        let expected = before + "FFFFFFS";
        assert_eq!(
            after_each(&records, None),
            expected,
            "{record}, nothing told"
        );
        assert_eq!(
            after_each(&records, Some(relaunch)),
            expected,
            "{record}, its relaunch told"
        );
        // One look that reads past where the new process began, as the window's
        // first can, its message taken and its prompt not yet written.
        let taken = &records[..relaunch + 2];
        assert_eq!(
            record_told(taken, &relaunched(&records, relaunch)).settlement,
            Settlement::InFlight,
            "{record}, read at once"
        );
    }
}

#[test]
fn a_resume_in_the_window_keeps_the_queue() {
    // `/resume` in a window runs the hook as a new process does, in the process
    // that holds the queue.
    let records = vec![
        hello(),
        queue("enqueue", json!("waiting")),
        session_start("h1", "u1", "SessionStart:resume"),
        answer("a1", "h1"),
        summary("s1", "a1"),
        queue("dequeue", Value::Null),
        user("u2", json!("s1"), json!("waiting")),
        assistant(
            "a2",
            "u2",
            json!("m2"),
            json!([text(json!("Done"))]),
            json!("end_turn"),
        ),
        summary("s2", "a2"),
    ];
    assert_eq!(after_each(&records, None), "FFFFFFFFS");
}

#[test]
fn a_process_that_closed_at_rest_with_a_notification_queued_holds_no_turn_after_the_relaunch() {
    // The turn ended, a job's notification was queued, and the window closed
    // before Claude took it. A process started on the conversation then writes
    // no sign of the old one: no turn was cut, and ConsensFlow's settings run
    // no hook as it resumes.
    let notification = "<task-notification>\n<task-id>bjob00001</task-id>\n<status>failed</status>\n</task-notification>";
    let records = vec![
        hello(),
        answer("a1", "u1"),
        summary("s1", "a1"),
        queue("enqueue", json!(notification)),
        user("u2", json!("s1"), json!("Go on")),
        assistant(
            "a2",
            "u2",
            json!("m2"),
            json!([text(json!("Done"))]),
            json!("end_turn"),
        ),
        summary("s2", "a2"),
    ];
    assert_eq!(after_each(&records, None), "FFSFFFF", "the record alone");
    assert_eq!(after_each(&records, Some(4)), "FFSSFFS");
}

#[test]
fn a_relaunch_told_after_the_new_process_was_read_takes_the_record_from_its_start() {
    // A look of the engine's own, told nothing, read what the new process wrote
    // before the window's first look told where it began.
    let mut records = vec![
        hello(),
        answer("a1", "u1"),
        summary("s1", "a1"),
        queue("enqueue", json!("stale")),
    ];
    let relaunch = relaunched(&records, records.len());
    records.extend([
        queue("enqueue", json!("Go on")),
        queue("dequeue", Value::Null),
    ]);
    let mut stage = Stage::new();
    stage.write(&records);
    let first = known(&stage.look_told(&Options::default()));
    assert_eq!(first.settlement, Settlement::InFlight);
    // The message the new process took is still to begin its turn.
    let told = stage.look_told(&relaunch);
    assert_eq!(known(&told).settlement, Settlement::InFlight);
    let answered = [
        user("u2", json!("s1"), json!("Go on")),
        assistant(
            "a2",
            "u2",
            json!("m2"),
            json!([text(json!("Done"))]),
            json!("end_turn"),
        ),
        summary("s2", "a2"),
    ];
    stage.append(&answered);
    let after = stage.look_told(&relaunch);
    assert_eq!(known(&after).settlement, Settlement::Settled);
    // As a reader told from its first look reads it.
    records.extend(answered);
    assert_eq!(
        show(&after),
        show(&Reading::Known(record_told(&records, &relaunch)))
    );
}

#[test]
fn the_latest_relaunch_holds_and_a_look_that_tells_an_earlier_one_or_none_changes_nothing() {
    let reply = |uuid: &str, parent: &str, id: &str| {
        assistant(
            uuid,
            parent,
            json!(id),
            json!([text(json!("Done"))]),
            json!("end_turn"),
        )
    };
    let mut records = vec![
        hello(),
        answer("a1", "u1"),
        summary("s1", "a1"),
        queue("enqueue", json!("first stale")),
    ];
    let first = relaunched(&records, records.len());
    records.extend([
        user("u2", json!("s1"), json!("Again")),
        reply("a2", "u2", "m2"),
        summary("s2", "a2"),
        queue("enqueue", json!("second stale")),
    ]);
    let second = relaunched(&records, records.len());
    records.extend([
        user("u3", json!("s2"), json!("Once more")),
        reply("a3", "u3", "m3"),
        summary("s3", "a3"),
    ]);
    let mut stage = Stage::new();
    stage.write(&records);
    let settled = |reading: Arc<Reading>| known(&reading).settlement;
    assert_eq!(settled(stage.look_told(&first)), Settlement::InFlight);
    assert_eq!(settled(stage.look_told(&second)), Settlement::Settled);
    assert_eq!(settled(stage.look_told(&first)), Settlement::Settled);
    assert_eq!(
        settled(stage.look_told(&Options::default())),
        Settlement::Settled
    );
}

#[test]
fn the_agents_the_closed_process_left_running_went_with_it() {
    // Its last turn ended with an agent running, as its duration says.
    let records = vec![
        hello(),
        answer("a1", "u1"),
        having(
            duration("d1", "a1"),
            json!({ "pendingBackgroundAgentCount": 1 }),
        ),
    ];
    let pending = |options: &Options| {
        let read = record_told(&records, options);
        (read.settlement, read.agents_pending)
    };
    assert_eq!(
        pending(&Options::default()),
        (Settlement::Settled, true),
        "the record alone"
    );
    assert_eq!(
        pending(&relaunched(&records, records.len())),
        (Settlement::Settled, false),
        "a process started after it"
    );
    // A turn of the new process that leaves agents says so as any.
    let mut again = records.clone();
    again.extend([
        user("u2", json!("d1"), json!("Again")),
        assistant(
            "a2",
            "u2",
            json!("m2"),
            json!([text(json!("Started one"))]),
            json!("end_turn"),
        ),
        having(
            duration("d2", "a2"),
            json!({ "pendingBackgroundAgentCount": 1 }),
        ),
    ]);
    let read = record_told(&again, &relaunched(&again, records.len()));
    assert!(read.agents_pending);
}

#[test]
fn a_record_the_old_process_wrote_past_the_relaunch_ends_none_of_its_turns_again() {
    // An old program that outlived its window writes its turn's duration, with an
    // agent running, after the length was taken (both reviews of round 2; where
    // the host could not prove the program gone, no length is taken at all).
    let records = vec![
        hello(),
        answer("a1", "u1"),
        summary("s1", "a1"),
        queue("enqueue", json!("stale")),
        having(
            duration("d1", "s1"),
            json!({ "pendingBackgroundAgentCount": 1 }),
        ),
    ];
    let relaunch = relaunched(&records, 4);
    let read = record_told(&records, &relaunch);
    assert!(!read.agents_pending, "no agent of the new process runs");
    // Read as it was written, the old turn's duration after the relaunch's look.
    let mut stage = Stage::new();
    stage.write(&records[..4]);
    assert!(!known(&stage.look_told(&relaunch)).agents_pending);
    stage.append(&records[4..]);
    assert!(!known(&stage.look_told(&relaunch)).agents_pending);
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

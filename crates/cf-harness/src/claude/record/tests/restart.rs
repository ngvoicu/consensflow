//! A conversation Claude Code goes on with in a new process (`--resume`): the
//! queue of messages is the process's own, held in memory, so what the process
//! before had queued and not begun went with it, and no record says so.
//! ConsensFlow knows where the new process began, as it closed the window the
//! old one ran in before it started the new one: a look is told where the transcript
//! ended then (`Options::relaunched`), and the queue is empty from there on,
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
use crate::records::Boundary;
use crate::shared::record::jsonl::EDGE_BYTES;
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
        relaunched: Some(boundary(&Stage::lines(&records[..from]))),
        ..Options::default()
    }
}

/// What a look told `options` reads of `records`.
fn record_told(records: &[Value], options: &Options) -> Record {
    let mut stage = Stage::new();
    stage.write(records);
    known(&stage.look_told(options))
}

/// Where a transcript written as `text` ends: its length, and the bytes before it that
/// the reader checks.
pub(super) fn boundary(text: &str) -> Boundary {
    let bytes = text.as_bytes();
    let edge = bytes.len().saturating_sub(EDGE_BYTES as usize);
    Boundary {
        at: bytes.len() as u64,
        edge: Arc::from(&bytes[edge..]),
    }
}

/// What a look at `records` reads.
fn record_of(records: &[Value]) -> Record {
    record_told(records, &Options::default())
}

fn known(reading: &Reading) -> Record {
    match reading {
        Reading::Known(record) => record.clone(),
        Reading::Unknown(reason) | Reading::Missing(reason) => panic!("unknown: {reason}"),
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
        (read.settlement, read.pending.agents > 0)
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
    assert!(read.pending.agents > 0);
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
    assert!(read.pending.agents == 0, "no agent of the new process runs");
    // Read as it was written, the old turn's duration after the relaunch's look.
    let mut stage = Stage::new();
    stage.write(&records[..4]);
    assert!(known(&stage.look_told(&relaunch)).pending.agents == 0);
    stage.append(&records[4..]);
    assert!(known(&stage.look_told(&relaunch)).pending.agents == 0);
}

#[test]
fn an_answer_the_old_process_never_ran_its_stop_hooks_for_holds_no_turn_after_the_relaunch() {
    // Sol's review of round 4: the old process went after its answer, before its
    // stop hooks' summary; the new one's answer and duration came before the
    // record of its prompt, an order Claude Code writes
    // (`tests/engine/fixtures/completion/README.md`).
    let records = vec![
        hello(),
        answer("a1", "u1"),
        assistant(
            "a2",
            "u2",
            json!("m2"),
            json!([text(json!("Done"))]),
            json!("end_turn"),
        ),
        duration("d2", "a2"),
        user("u2", json!("a1"), json!("Again")),
    ];
    let read = record_told(&records, &relaunched(&records, 2));
    assert_eq!(read.settlement, Settlement::Settled);
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

/// The live run's record (`monitor-wait.jsonl`) to the end of the turn its monitor's
/// first event woke: a command and a monitor pending, as the window closed on them.
fn left_a_monitor() -> Vec<Value> {
    let records = super::jobs::monitor_wait();
    let first_event = place(&records, |record| {
        record["message"]["content"]
            .as_array()
            .is_some_and(|parts| parts.iter().any(|part| part["text"] == "NOTED"))
    });
    let end = first_event
        + 1
        + place(&records[first_event..], |record| {
            record["subtype"] == "turn_duration"
        });
    records[..end].to_vec()
}

/// What the new process does: a prompt, and its answer, which ends its turn.
fn went_on(after: &str) -> Vec<Value> {
    vec![
        user("u90", json!(after), json!("Go on")),
        assistant(
            "a90",
            "u90",
            json!("m90"),
            json!([text(json!("Gone on"))]),
            json!("end_turn"),
        ),
        summary("s90", "a90"),
    ]
}

/// The last record's uuid.
fn last_uuid(records: &[Value]) -> String {
    records.last().unwrap()["uuid"].as_str().unwrap().to_owned()
}

#[test]
fn the_jobs_the_closed_process_left_are_retired_at_the_relaunch_and_not_ended() {
    // T-442's restart with its monitor (T-8's answer, 2026-10-21): the window was closed
    // with a command and a monitor running, and opened again on the conversation once the
    // host proved its program gone. Their notifications went with the process: the new
    // process's turns are held by none of them, and the reading says they were retired.
    let before = left_a_monitor();
    let mut records = before.clone();
    records.extend(went_on(&last_uuid(&before)));
    let read = record_of(&records);
    assert_eq!(
        (read.settlement, read.pending.jobs, read.pending.retired),
        (Settlement::Settled, 2, 0),
        "the record alone: both run"
    );
    let read = record_told(&records, &relaunched(&records, before.len()));
    assert_eq!(
        (read.settlement, read.pending.jobs, read.pending.retired),
        (Settlement::Settled, 0, 2),
        "a process started after it"
    );
    // A job the new process starts runs, as any; a word of the end of a retired one, as
    // the new process's summary of what the old left says, settles it.
    let mut again = records.clone();
    let started = having(
        user(
            "u91",
            json!("s90"),
            json!([tool_result(
                json!("t91"),
                json!("Command running in background")
            )]),
        ),
        json!({ "toolUseResult": { "backgroundTaskId": "bnew0001" } }),
    );
    let said = "<task-notification>\n<task-id>b615qv2gd</task-id>\n<status>stopped</status>\n</task-notification>";
    again.extend([
        assistant(
            "a91",
            "s90",
            json!("m91"),
            json!([tool_use(json!("t91"))]),
            json!("tool_use"),
        ),
        started,
        having(
            user("w91", json!("u91"), json!(said)),
            json!({ "promptSource": "system", "turnOrigin": "task_notification" }),
        ),
        assistant(
            "a92",
            "w91",
            json!("m92"),
            json!([text(json!("Noted"))]),
            json!("end_turn"),
        ),
        summary("s92", "a92"),
    ]);
    let read = record_told(&again, &relaunched(&again, before.len()));
    assert_eq!(
        (read.settlement, read.pending.jobs, read.pending.retired),
        (Settlement::Settled, 1, 1)
    );
}

#[test]
fn a_resume_in_the_window_retires_no_job() {
    // `/resume` typed in the window keeps the process and its jobs: Claude Code writes
    // the close of a cut turn and the resume hook's run all the same, and ConsensFlow,
    // which opened no window, tells the reader of no relaunch.
    let before = left_a_monitor();
    let last = last_uuid(&before);
    let mut records = before;
    records.extend([
        closed("c90", &last),
        session_start("h90", "c90", "SessionStart:resume"),
    ]);
    records.extend(went_on("h90"));
    let read = record_of(&records);
    assert_eq!(
        (read.settlement, read.pending.jobs, read.pending.retired),
        (Settlement::Settled, 2, 0)
    );
}

/// T-526's shape (the consensflow board, Claude Code 2.1.310, 2026-10-25), with made-up
/// words: the close ended the worker's background job, whose notification the closing
/// process queued and took, beginning no turn. The process the reopen started told the
/// job's end again, and took the reopen's paste, each with an operation of its own, and
/// its turn ended at its answer. Told nothing, the reader keeps the closed process's taken
/// notification as a turn still to begin (four hours on the board, until the chief's
/// Finish); told where the new process began, the turn is over at its answer.
#[test]
fn a_notification_the_closing_process_took_and_never_began_holds_no_turn_after_the_relaunch() {
    let notice = "<task-notification>\n<task-id>bland0001</task-id>\n<status>failed</status>\n<summary>Background command \"Land it\" failed with exit code 143</summary>\n</task-notification>";
    let reopen = "[ConsensFlow m-2 · T-1 · task from @chief]\nLand it again";
    let mut records = vec![
        hello(),
        answer("a1", "u1"),
        duration("d1", "a1"),
        // The closing process: the notification queued and taken.
        queue("enqueue", json!(notice)),
        queue("dequeue", Value::Null),
    ];
    let relaunch = records.len();
    records.extend([
        queue("enqueue", json!(notice)),
        queue("dequeue", Value::Null),
        queue("enqueue", json!("")),
        queue("dequeue", Value::Null),
        having(
            user("u2", json!("d1"), json!(notice)),
            json!({ "promptSource": "system", "origin": { "kind": "task-notification" } }),
        ),
        having(
            user("u3", json!("u2"), json!(reopen)),
            json!({ "promptSource": "queued" }),
        ),
        assistant(
            "a3",
            "u3",
            json!("m3"),
            json!([text(json!("LANDED"))]),
            json!("end_turn"),
        ),
        summary("s3", "a3"),
        duration("d3", "s3"),
    ]);
    assert_eq!(
        settlement_of(&records),
        Settlement::InFlight,
        "told nothing"
    );
    let mut untaken = records.clone();
    untaken.remove(relaunch - 1);
    untaken.remove(relaunch - 2);
    assert_eq!(
        settlement_of(&untaken),
        Settlement::Settled,
        "the taken notification is what holds it"
    );
    let read = record_told(&records, &relaunched(&records, relaunch));
    assert_eq!(
        read.settlement,
        Settlement::Settled,
        "the reopen's turn ended"
    );
    let last = read.items.last().unwrap();
    assert_eq!(
        (last.role, &*last.text, last.complete),
        (Role::Assistant, "LANDED", true)
    );
}

/// What a process writes of itself as it goes that a rewrite of the transcript can leave
/// out: `count` lines of bookkeeping, long enough that the transcript without them is
/// shorter than it was with them.
fn bookkeeping(count: usize) -> Vec<Value> {
    (0..count)
        .map(|at| json!({ "type": "last-prompt", "sessionId": SESSION, "lastPrompt": format!("{at}{}", "x".repeat(600)) }))
        .collect()
}

/// A reader's state told `options` on each look of `stages`, each stage the whole
/// transcript then: written anew where it says so, else added to it.
fn settlements(stages: &[(bool, Vec<Value>)], options: &Options) -> Vec<Settlement> {
    let mut stage = Stage::new();
    stages
        .iter()
        .map(|(anew, records)| {
            if *anew {
                stage.write(records);
            } else {
                stage.append(records);
            }
            known(&stage.look_told(options)).settlement
        })
        .collect()
}

#[test]
fn a_transcript_rewritten_shorter_than_the_relaunch_keeps_the_new_process_answer() {
    // Both reviews of the relaunch boundary (P2, rounds 2 and 4): the transcript is
    // written anew, shorter than it was where the new process began, and still holds
    // what the new process wrote: its answer, whose stop hooks' summary comes after the
    // rewrite. The boundary is told as a length alone, and so is read where the file is
    // that long no more: at its end, which drops the new answer as the old process's,
    // and the summary then ends no turn.
    let mut old = vec![hello(), answer("a1", "u1"), summary("s1", "a1")];
    old.extend(bookkeeping(8));
    let options = relaunched(&old, old.len());
    let new = vec![
        user("u2", json!("s1"), json!("Again")),
        assistant(
            "a2",
            "u2",
            json!("m2"),
            json!([text(json!("Done"))]),
            json!("end_turn"),
        ),
    ];
    let mut rewritten = old[..3].to_vec();
    rewritten.extend(new.clone());
    assert!(Stage::lines(&rewritten).len() < Stage::lines(&old).len());
    let read = settlements(
        &[
            (true, old.clone()),
            (false, new),
            (true, rewritten),
            (false, vec![summary("s2", "a2")]),
        ],
        &options,
    );
    assert_eq!(
        read,
        [
            Settlement::Settled,
            Settlement::InFlight,
            Settlement::InFlight,
            Settlement::Settled
        ]
    );
}

#[test]
fn a_relaunch_told_after_a_rewrite_shorter_than_the_one_before_is_where_the_new_process_began() {
    // Sol's review of round 2 (P2): after a rewrite below the boundary, the next process
    // ConsensFlow starts begins at a length below the last one told, which a length
    // compared with the last as a number takes for an earlier launch's. The process
    // before it had queued a notification as it went; the new one queued its message
    // and has not yet taken it, and its window is busy.
    let mut first = vec![hello(), answer("a1", "u1"), summary("s1", "a1")];
    first.extend(bookkeeping(8));
    let earlier = relaunched(&first, first.len());
    let mut rewritten = first[..3].to_vec();
    rewritten.extend([
        user("u2", json!("s1"), json!("Again")),
        assistant(
            "a2",
            "u2",
            json!("m2"),
            json!([text(json!("Done"))]),
            json!("end_turn"),
        ),
        summary("s2", "a2"),
        queue("enqueue", json!("stale")),
    ]);
    let later = relaunched(&rewritten, rewritten.len());
    let mut records = rewritten.clone();
    records.push(queue("enqueue", json!("Go on")));
    assert!(Stage::lines(&records).len() < Stage::lines(&first).len());
    let mut stage = Stage::new();
    stage.write(&first);
    assert_eq!(
        known(&stage.look_told(&earlier)).settlement,
        Settlement::Settled
    );
    stage.write(&records);
    assert_eq!(
        known(&stage.look_told(&later)).settlement,
        Settlement::InFlight,
        "the new process's message is still to be taken"
    );
    stage.append(&[
        queue("dequeue", Value::Null),
        user("u3", json!("s2"), json!("Go on")),
        assistant(
            "a3",
            "u3",
            json!("m3"),
            json!([text(json!("Gone on"))]),
            json!("end_turn"),
        ),
        summary("s3", "a3"),
    ]);
    assert_eq!(
        known(&stage.look_told(&later)).settlement,
        Settlement::Settled,
        "the old process's notification went with it"
    );
}

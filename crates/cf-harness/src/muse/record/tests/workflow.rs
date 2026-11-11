//! A workflow's children, on a record made here (`tests/fixtures/muse/made/workflow-children.jsonl`):
//! one workflow of five `workflow-agent` children, in the order of event kinds, statuses
//! and timings of what T-1458 left on the owner's board (T-1467, T-1471), every id and
//! word made. In the real record each child ended with `result_ready` and its lifecycle's
//! `completed` and `terminal`, none with the `subagent_result` or the ended status the
//! reader waited for, and the board held "working in the background (5 agents)" for 25
//! minutes after the last one. Also the guard for a kind the reader does not know.

use std::sync::{Arc, Mutex};

use serde_json::{json, Value};

use super::*;
use crate::shared::record::reading::Pending;
use crate::shared::record::torn::say_with;

const MADE: &str = include_str!("../../../../tests/fixtures/muse/made/workflow-children.jsonl");

fn pending(jobs: usize, agents: usize, retired: usize) -> Pending {
    Pending {
        jobs,
        agents,
        retired,
        noticed: 0,
    }
}

/// What a fresh reader reads of the lines of `log` that `keep` keeps, up to and including
/// the first line that holds `last`.
fn read_through(log: &str, last: &str, keep: impl Fn(&str) -> bool) -> Record {
    let mut text = String::new();
    for line in log.lines().filter(|line| keep(line)) {
        text.push_str(line);
        text.push('\n');
        if line.contains(last) {
            break;
        }
    }
    known(&fresh_reading(&session_of(log), &text)).clone()
}

fn through(last: &str) -> Record {
    read_through(MADE, last, |_| true)
}

#[test]
fn the_made_record_reads_the_same_read_on_a_record_at_a_time_as_read_fresh() {
    let session = session_of(MADE);
    let stage = Stage::new(&session);
    let mut reading_on = stage.reader();
    let mut so_far = String::new();
    for line in MADE.lines() {
        so_far.push_str(line);
        so_far.push('\n');
        stage.append(line);
        assert_eq!(
            look(&mut reading_on),
            fresh_reading(&session, &so_far),
            "{line:.60}"
        );
    }
}

#[test]
fn five_workflow_children_that_end_with_result_ready_are_five_agents_until_each_ends() {
    // The turn that started the workflow is over (its `terminal`), the five children
    // run, and the workflow's task runs on in the background.
    let running = through("\"control_status\":\"running\"");
    assert_eq!(running.pending.agents, 1);
    let all = through("\"child-e\",\"attempt\":1,\"status\":\"started\"");
    assert_eq!(all.settlement, Settlement::Settled, "the turn is over");
    assert_eq!(all.pending, pending(1, 5, 0));
    // Each ends at its `result_ready`, in the real order: b, c, a, e, d.
    for (ended, left) in ["child-b", "child-c", "child-a", "child-e", "child-d"]
        .into_iter()
        .zip([4, 3, 2, 1, 0])
    {
        let record = through(&format!(
            "\"kind\":\"result_ready\",\"subagent_id\":\"{ended}\""
        ));
        assert_eq!(record.pending.agents, left, "after {ended}'s result_ready");
    }
}

#[test]
fn a_workflow_whose_children_all_ended_leaves_nothing_pending_once_its_task_ends() {
    // The last child ended (its lifecycle's `terminal`): the workflow's task still runs.
    let last = through("\"child-d\",\"attempt\":1,\"status\":\"terminal\"");
    assert_eq!(last.pending, pending(1, 0, 0));
    // Its task completes (the end the agent is told of), and the item is queued
    // and drained into the run it wakes, which answers and ends.
    let told = through("\"kind\":\"inbox_item_queued\"");
    assert_eq!(told.pending, pending(1, 0, 0), "its agent not told yet");
    let woken = through("\"kind\":\"inbox_item_drained\"");
    assert_eq!(woken.pending, Pending::default());
    let done = through("\"run_id\":\"run-2\",\"event\":{\"kind\":\"terminal\"");
    assert_eq!(done.settlement, Settlement::Settled);
    assert_eq!(done.pending, Pending::default());
    // A quit 25 minutes later (`session.end`) retires nothing: nothing was left.
    let quit = through("\"payload_type\":\"session.end\"");
    assert_eq!(quit.pending, Pending::default());
}

/// The line of the workflow's task ending, the first of the made record to hold it.
const TASK_ENDS: &str = "\"task_id\":\"task-workflow\",\"event\":{\"kind\":\"completed\"";

fn run_line(sequence: u64, event: Value) -> String {
    super::run(sequence, "run-1", event).to_string()
}

/// The made record with the member's answer before the workflow's end: run-1's `terminal`
/// (the third line of what it said, right after the launch) is taken out and put, with an
/// answer, just before the task's end, as a plain background task is answered before it ends
/// (`live/README.md`, the background command's turn).
fn answered_early() -> Vec<String> {
    let mut lines: Vec<String> = MADE
        .lines()
        .filter(|line| !line.contains("\"run_id\":\"run-1\",\"event\":{\"kind\":\"terminal\""))
        .map(str::to_owned)
        .collect();
    let at = lines
        .iter()
        .position(|line| line.contains(TASK_ENDS))
        .unwrap();
    lines.insert(
        at,
        run_line(9001, json!({ "kind": "terminal", "terminal": "completed" })),
    );
    lines.insert(
        at,
        run_line(
            9000,
            json!({ "kind": "assistant_message_committed", "message_id": "early", "text": "Made early answer." }),
        ),
    );
    lines
}

/// What a fresh reader reads of `lines`, up to and including the first that holds `last`.
fn lines_through(lines: &[String], last: &str) -> Record {
    let mut text = String::new();
    for line in lines {
        text.push_str(line);
        text.push('\n');
        if line.contains(last) {
            break;
        }
    }
    known(&fresh_reading(&session_of(MADE), &text)).clone()
}

#[test]
fn a_workflow_ended_before_its_notice_is_queued_is_pending_until_the_notice_is_taken_in() {
    let lines = answered_early();
    // The member answered; the workflow's task completes (record 74) and Muse has not yet
    // queued the notice for the agent (75): the turn is over and the workflow is not.
    let at_end = lines_through(&lines, TASK_ENDS);
    assert_eq!(at_end.settlement, Settlement::Settled, "answered");
    assert_eq!(at_end.pending, pending(1, 0, 0), "the notice is owed");
    // Queued (75): still pending, once; the run it wakes starts (76), and takes it in (77).
    assert_eq!(
        lines_through(&lines, "\"kind\":\"inbox_item_queued\"").pending,
        pending(1, 0, 0)
    );
    assert_eq!(
        lines_through(
            &lines,
            "\"run_id\":\"run-2\",\"event\":{\"kind\":\"started\""
        )
        .pending,
        pending(1, 0, 0)
    );
    assert_eq!(
        lines_through(&lines, "\"kind\":\"inbox_item_drained\"").pending,
        Pending::default()
    );
    // The whole record, to its end, is at zero.
    let whole = lines_through(&lines, "\"payload_type\":\"session.end\"");
    assert_eq!(whole.pending, Pending::default());
}

#[test]
fn a_notice_owed_and_never_queued_ends_with_the_turn_that_was_awake_or_the_session() {
    // The agent was awake when the workflow ended (its turn open): Muse tells it another
    // way, and the notice is owed no more when that turn ends.
    let mut lines: Vec<String> = MADE
        .lines()
        .filter(|line| !line.contains("\"run_id\":\"run-1\",\"event\":{\"kind\":\"terminal\""))
        .take_while(|line| !line.contains("\"kind\":\"inbox_item_queued\""))
        .map(str::to_owned)
        .collect();
    let awake = lines_through(&lines, TASK_ENDS);
    assert_eq!(awake.settlement, Settlement::InFlight);
    assert_eq!(awake.pending, pending(1, 0, 0));
    lines.push(run_line(
        9001,
        json!({ "kind": "terminal", "terminal": "completed" }),
    ));
    let over = known(&fresh_reading(
        &session_of(MADE),
        &(lines.join("\n") + "\n"),
    ))
    .clone();
    assert_eq!(over.settlement, Settlement::Settled);
    assert_eq!(
        over.pending,
        Pending::default(),
        "no notice came, none is waited for"
    );
    // The agent at rest, and the session ends with the notice never queued: it is retired.
    let mut at_rest: Vec<String> = answered_early()
        .into_iter()
        .take_while(|line| !line.contains("\"kind\":\"inbox_item_queued\""))
        .collect();
    at_rest.push(MADE.lines().last().unwrap().to_owned());
    let quit = known(&fresh_reading(
        &session_of(MADE),
        &(at_rest.join("\n") + "\n"),
    ))
    .clone();
    assert_eq!(
        quit.pending,
        pending(0, 0, 1),
        "retired, no word of its end coming"
    );
}

#[test]
fn a_workflow_task_that_fails_is_cancelled_or_times_out_owes_its_notice_and_ends_no_child() {
    // Nothing recorded says what Muse does with a child still running when its workflow's
    // task ends otherwise than completed (its strings say `workflow run cancelled before a
    // terminal child outcome was recorded`, a `generated_child_cancelled`): the child stays
    // counted until its own records end it. Here child-d's end records are gone.
    for end in ["failed", "cancelled", "timed_out"] {
        let lines: Vec<String> = MADE
            .lines()
            .filter(|line| {
                !(line.contains("\"subagent_id\":\"child-d\"") && line.contains("\"result_ready\"")
                    || line.contains("\"child_id\":\"child-d\"")
                        && (line.contains("\"status\":\"completed\"")
                            || line.contains("\"status\":\"terminal\"")))
            })
            .map(|line| {
                line.replace(
                    TASK_ENDS,
                    &format!("\"task_id\":\"task-workflow\",\"event\":{{\"kind\":\"{end}\""),
                )
            })
            .collect();
        let ended = lines_through(&lines, &format!("\"event\":{{\"kind\":\"{end}\""));
        assert_eq!(
            ended.pending,
            pending(1, 1, 0),
            "{end}: child-d still counted, notice owed"
        );
    }
}

#[test]
fn a_child_with_no_end_in_the_record_stays_counted_when_its_workflow_s_task_ends() {
    let lines: Vec<String> = MADE
        .lines()
        .filter(|line| {
            !(line.contains("\"subagent_id\":\"child-d\"") && line.contains("\"result_ready\"")
                || line.contains("\"child_id\":\"child-d\"")
                    && (line.contains("\"status\":\"completed\"")
                        || line.contains("\"status\":\"terminal\"")))
        })
        .map(str::to_owned)
        .collect();
    assert_eq!(lines_through(&lines, TASK_ENDS).pending.agents, 1);
}

#[test]
fn a_later_attempt_of_a_child_that_ended_counts_again_made_from_the_binary_s_strings() {
    // No live record has a second attempt: the lifecycle's `attempt` is 1 in every one. The
    // binary names `workflow_lifecycle_retries` and an `attempt_ref`; this is made on the
    // reading that a retry says `scheduled` and `started` again with a higher `attempt`.
    let mut lines: Vec<String> = MADE.lines().map(str::to_owned).collect();
    let at = lines
        .iter()
        .position(|line| {
            line.contains("\"child_id\":\"child-b\"") && line.contains("\"status\":\"terminal\"")
        })
        .unwrap();
    let again = |sequence: u64, status: &str| {
        run_line(
            sequence,
            json!({ "kind": "workflow_child_lifecycle", "workflow_run_id": "wf-1",
            "child_id": "child-b", "attempt": 2, "status": status, "task_id": "task-child-b" }),
        )
    };
    lines.insert(at + 1, again(9001, "scheduled"));
    lines.insert(at + 2, again(9002, "started"));
    assert_eq!(
        lines_through(&lines, "\"status\":\"terminal\"")
            .pending
            .agents,
        4,
        "b ended"
    );
    assert_eq!(
        lines_through(&lines, "\"attempt\":2,\"status\":\"started\"")
            .pending
            .agents,
        5,
        "b again"
    );
    // The retry reports and ends again; the rest of the record is as it was.
    lines.insert(at + 3, again(9003, "terminal"));
    let ended = lines_through(&lines, "\"attempt\":2,\"status\":\"terminal\"");
    assert_eq!(ended.pending.agents, 4, "b ended again");
    let whole = lines_through(&lines, "\"payload_type\":\"session.end\"");
    assert_eq!(whole.pending, Pending::default());
}

#[test]
fn a_workflow_child_is_told_ended_by_its_lifecycle_alone() {
    // Neither its `result_ready` nor the end of its task in the record: its lifecycle says.
    let record = read_through(
        MADE,
        "\"child-d\",\"attempt\":1,\"status\":\"terminal\"",
        |line| {
            !(line.contains("\"result_ready\"") || line.contains("\"task_id\":\"task-workflow\""))
        },
    );
    assert_eq!(record.pending.agents, 0);
}

/// The lines the reader said, on this thread, of what it read.
fn said_while(read: impl FnOnce()) -> Vec<String> {
    let lines = Arc::new(Mutex::new(Vec::new()));
    let heard = Arc::clone(&lines);
    say_with(Box::new(move |line| {
        heard.lock().unwrap().push(line.to_owned())
    }));
    read();
    let said = lines.lock().unwrap().clone();
    said
}

/// A made child record of the kind `kind`.
fn child_record(sequence: u64, kind: &str, child: &str, more: Value) -> Value {
    let mut record = json!({ "kind": kind, "subagent_id": child });
    for (name, value) in more.as_object().cloned().unwrap_or_default() {
        record[name] = value;
    }
    json!({
        "schema_version": 1, "id": format!("g{sequence}"), "sequence": sequence,
        "stream": { "kind": "session", "id": "s1" },
        "payload_type": format!("subagent.control.{kind}"),
        "payload": { "kind": "subagent_control", "record": record },
    })
}

#[test]
fn a_kind_of_child_record_the_reader_does_not_know_is_said_once_a_kind_not_once_a_look() {
    let records = [
        intent(1, "t1", "Ask a child."),
        super::run(2, "t1", json!({ "kind": "started" })),
        child_record(3, "spawn_accepted", "c1", json!({})),
        child_record(4, "recovery_pending_review", "c1", json!({})),
        child_record(
            5,
            "status_updated",
            "c1",
            json!({ "control_status": "wedged" }),
        ),
        child_record(6, "recovery_pending_review", "c1", json!({})),
        super::run(
            7,
            "t1",
            json!({ "kind": "terminal", "terminal": "completed" }),
        ),
    ];
    let said = said_while(|| {
        // The same log read three times, a look each.
        for _ in 0..3 {
            let text: String = records.iter().map(|record| format!("{record}\n")).collect();
            let read = known(&fresh_reading("s1", &text)).clone();
            // The child is held as it was: running, as before.
            assert_eq!(read.pending.agents, 1);
        }
    });
    assert_eq!(
        said,
        [
            "a child record of kind recovery_pending_review is not read: the work it is of is held as it was",
            "a child status of kind wedged is not read: the work it is of is held as it was",
        ]
    );
}

#[test]
fn a_monitor_whose_time_passed_is_over() {
    // A monitor armed in the turn, which the turn leaves watching; its task ends `timed_out`
    // when its `timeout_ms` passes (the binary's `TaskTerminalKind`; no live log has one).
    let task = |sequence: u64, event: Value| {
        json!({
            "schema_version": 1, "id": format!("k{sequence}"), "sequence": sequence,
            "stream": { "kind": "session", "id": "s1" }, "payload_type": "runtime.session",
            "payload": { "kind": "task", "run_id": "t1", "task_id": "k1", "event": event },
        })
    };
    let run = |sequence: u64, event: Value| super::run(sequence, "t1", event);
    let mut records = vec![
        intent(1, "t1", "Watch it."),
        run(2, json!({ "kind": "started" })),
        task(3, json!({ "kind": "started" })),
        task(
            4,
            json!({ "kind": "output", "chunk": json!({ "work_id": "work.v2.monitor.x" }).to_string() }),
        ),
        run(5, json!({ "kind": "terminal", "terminal": "completed" })),
    ];
    assert_eq!(record_of(&records).pending, pending(1, 0, 0), "watching");
    records.push(task(6, json!({ "kind": "timed_out" })));
    assert_eq!(
        record_of(&records).pending,
        Pending::default(),
        "its time passed"
    );
}

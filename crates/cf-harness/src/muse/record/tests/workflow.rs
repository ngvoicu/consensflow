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

#[test]
fn a_child_with_no_end_in_the_record_ends_with_its_workflow_s_task() {
    // The record of child-d's ends is gone (its `result_ready`, lifecycle `completed` and
    // `terminal`): it is said to run until its workflow's task completes, and no longer.
    let lacks = |line: &str| {
        !(line.contains("\"subagent_id\":\"child-d\"") && line.contains("\"result_ready\"")
            || line.contains("\"child_id\":\"child-d\"")
                && (line.contains("\"completed\"") || line.contains("\"terminal\"")))
    };
    let before = read_through(MADE, "\"kind\":\"workflow_launch_reconciled\"", lacks);
    assert_eq!(before.pending.agents, 1, "child-d, though the others ended");
    let after = read_through(MADE, "\"kind\":\"inbox_item_queued\"", lacks);
    assert_eq!(after.pending.agents, 0, "its workflow's task completed");
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

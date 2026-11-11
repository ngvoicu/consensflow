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
fn a_notice_owed_and_never_queued_is_retired_where_the_session_ends_inside_the_wait() {
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

// ---- One bounded wait after a workflow's task ends (T-1481, A5-26) ----

/// The awake-run ordering, made for the test (`tests/fixtures/muse/made/`, its frame says
/// so): the task ends while the agent's run is open, that run ends, and only after
/// that is the notice queued. No Muse wrote this order; nothing rules it out.
const AWAKE: &str =
    include_str!("../../../../tests/fixtures/muse/made/workflow-awake-notice-late.jsonl");

/// When the workflow's task ends in the made records (record 74's `recorded_at`), in ms.
const END_MS: i64 = 1_793_094_244_000;
/// A look inside the wait, and one after it (the wait is 10 s: the made record's notice
/// follows the task's end by 10 ms and its drain by 160 ms).
const INSIDE_MS: i64 = 9_000;
const AFTER_MS: i64 = 11_000;

/// One reader on a log that grows, looked at when the test says.
struct Walk {
    stage: Stage,
    reader: Box<dyn Look + Send>,
    lines: Vec<String>,
    next: usize,
}

impl Walk {
    fn new(lines: Vec<String>) -> Self {
        let stage = Stage::new(&session_of(MADE));
        let reader = stage.reader();
        Self {
            stage,
            reader,
            lines,
            next: 0,
        }
    }

    /// The log grows to the first line that holds `last`, which is in.
    fn through(&mut self, last: &str) -> &mut Self {
        while let Some(line) = self.lines.get(self.next) {
            self.stage.append(line);
            self.next += 1;
            if line.contains(last) {
                return self;
            }
        }
        panic!("no line holds {last}");
    }

    /// A look at `now_ms`, with nothing more read than the log holds.
    fn at(&mut self, now_ms: i64) -> Record {
        known(&self.reader.look(&Options::default(), now_ms)).clone()
    }
}

fn lines_of(log: &str) -> Vec<String> {
    log.lines().map(str::to_owned).collect()
}

#[test]
fn a_workflow_ended_and_told_nothing_is_pending_for_a_set_time_and_not_after() {
    let mut walk = Walk::new(answered_early());
    walk.through(TASK_ENDS);
    let first = walk.at(END_MS + 20);
    assert_eq!(first.settlement, Settlement::Settled, "the early answer");
    assert_eq!(first.pending, pending(1, 0, 0));
    assert_eq!(walk.at(END_MS + INSIDE_MS).pending, pending(1, 0, 0));
    // No record came, and the look says time passed.
    assert_eq!(walk.at(END_MS + AFTER_MS).pending, Pending::default());
}

#[test]
fn a_notice_queued_inside_the_wait_and_not_drained_holds_without_a_limit_of_time() {
    let mut walk = Walk::new(answered_early());
    walk.through("\"kind\":\"inbox_item_queued\"");
    assert_eq!(walk.at(END_MS + 20).pending, pending(1, 0, 0));
    assert_eq!(walk.at(END_MS + 60_000).pending, pending(1, 0, 0));
    assert_eq!(walk.at(END_MS + 3_600_000).pending, pending(1, 0, 0));
    walk.through("\"kind\":\"inbox_item_drained\"");
    assert_eq!(walk.at(END_MS + 3_600_100).pending, Pending::default());
}

#[test]
fn the_awake_run_ending_before_the_notice_is_queued_reads_pending_throughout() {
    // T-1479's finding 1: task end, the awake run answers and ends, then the notice.
    let mut walk = Walk::new(lines_of(AWAKE));
    walk.through(TASK_ENDS);
    let end = walk.at(END_MS + 20);
    assert_eq!(end.settlement, Settlement::InFlight, "run-1 is awake");
    assert_eq!(end.pending, pending(1, 0, 0));
    walk.through("\"event\":{\"kind\":\"terminal\"");
    let ended = walk.at(END_MS + 250);
    assert_eq!(ended.settlement, Settlement::Settled, "the awake run ended");
    assert_eq!(ended.pending, pending(1, 0, 0), "no zero before the notice");
    assert_eq!(walk.at(END_MS + 2_900).pending, pending(1, 0, 0));
    walk.through("\"kind\":\"inbox_item_queued\"");
    assert_eq!(walk.at(END_MS + 3_020).pending, pending(1, 0, 0));
    walk.through("\"run_id\":\"run-2\",\"event\":{\"kind\":\"started\"");
    let woken = walk.at(END_MS + 3_100);
    assert_eq!(
        woken.settlement,
        Settlement::InFlight,
        "the wake-up run holds"
    );
    assert_eq!(woken.pending, pending(1, 0, 0));
    walk.through("\"kind\":\"inbox_item_drained\"");
    assert_eq!(walk.at(END_MS + 3_200).pending, Pending::default());
    walk.through("\"payload_type\":\"session.end\"");
    let whole = walk.at(END_MS + 3_700_000);
    assert_eq!(whole.settlement, Settlement::Settled);
    assert_eq!(whole.pending, Pending::default());
}

#[test]
fn a_clock_that_steps_back_neither_ends_the_wait_early_nor_stretches_it() {
    let mut walk = Walk::new(answered_early());
    walk.through(TASK_ENDS);
    assert_eq!(walk.at(END_MS + 20).pending, pending(1, 0, 0));
    // An hour back: the wait runs from this look, as well as by the record's time.
    let back = END_MS - 3_600_000;
    assert_eq!(walk.at(back).pending, pending(1, 0, 0));
    assert_eq!(walk.at(back + INSIDE_MS).pending, pending(1, 0, 0));
    assert_eq!(walk.at(back + AFTER_MS).pending, Pending::default());
}

#[test]
fn a_task_ended_long_before_the_first_look_is_waited_on_from_that_look() {
    let mut walk = Walk::new(answered_early());
    walk.through(TASK_ENDS);
    let late = END_MS + 3_600_000;
    assert_eq!(walk.at(late).pending, pending(1, 0, 0));
    assert_eq!(walk.at(late + INSIDE_MS).pending, pending(1, 0, 0));
    assert_eq!(walk.at(late + AFTER_MS).pending, Pending::default());
}

/// The made record without child-d's own end.
fn without_child_d_s_end() -> Vec<String> {
    MADE.lines()
        .filter(|line| {
            !(line.contains("\"subagent_id\":\"child-d\"") && line.contains("\"result_ready\"")
                || line.contains("\"child_id\":\"child-d\"")
                    && (line.contains("\"status\":\"completed\"")
                        || line.contains("\"status\":\"terminal\"")))
        })
        .map(str::to_owned)
        .collect()
}

#[test]
fn every_child_of_a_workflow_that_completed_is_ended_at_its_task_s_end() {
    let mut walk = Walk::new(without_child_d_s_end());
    walk.through("\"child-e\",\"attempt\":1,\"status\":\"terminal\"");
    assert_eq!(walk.at(END_MS - 1_000).pending.agents, 1, "child-d runs");
    walk.through(TASK_ENDS);
    let ended = walk.at(END_MS + 20);
    assert_eq!(ended.pending, pending(1, 0, 0), "the notice is owed");
}

#[test]
fn a_child_of_a_workflow_that_failed_was_cancelled_or_timed_out_is_ended_with_the_wait_and_said() {
    for end in ["failed", "cancelled", "timed_out"] {
        let lines: Vec<String> = without_child_d_s_end()
            .into_iter()
            .map(|line| {
                line.replace(
                    TASK_ENDS,
                    &format!("\"task_id\":\"task-workflow\",\"event\":{{\"kind\":\"{end}\""),
                )
            })
            .collect();
        let said = said_while(|| {
            let mut walk = Walk::new(lines.clone());
            walk.through(&format!("\"event\":{{\"kind\":\"{end}\""));
            let record = walk.at(END_MS + 20);
            assert_eq!(record.pending, pending(1, 1, 0), "{end}: held in the wait");
            assert_eq!(walk.at(END_MS + INSIDE_MS).pending, pending(1, 1, 0));
            assert_eq!(
                walk.at(END_MS + AFTER_MS).pending,
                Pending::default(),
                "{end}: the wait is over, and the child with it"
            );
            // Looked at again, and the log read again from its start by a fresh reader.
            assert_eq!(walk.at(END_MS + 20_000).pending, Pending::default());
            let mut again = Walk::new(lines.clone());
            again.through(&format!("\"event\":{{\"kind\":\"{end}\""));
            again.at(END_MS + 30_000);
            assert_eq!(
                again.at(END_MS + 30_000 + AFTER_MS).pending,
                Pending::default()
            );
        });
        assert_eq!(
            said,
            ["workflow task task-workflow ended: children with no end of their own, now counted ended: 1"],
            "{end}"
        );
    }
}

#[test]
fn a_child_of_a_workflow_that_failed_is_ended_when_its_notice_is_drained() {
    let lines: Vec<String> = without_child_d_s_end()
        .into_iter()
        .map(|line| {
            line.replace(
                TASK_ENDS,
                "\"task_id\":\"task-workflow\",\"event\":{\"kind\":\"failed\"",
            )
        })
        .collect();
    let mut walk = Walk::new(lines);
    walk.through("\"kind\":\"inbox_item_queued\"");
    assert_eq!(walk.at(END_MS + 20).pending, pending(1, 1, 0));
    assert_eq!(
        walk.at(END_MS + 60_000).pending,
        pending(1, 1, 0),
        "queued, not drained"
    );
    walk.through("\"kind\":\"inbox_item_drained\"");
    assert_eq!(walk.at(END_MS + 60_100).pending, Pending::default());
    // The wake-up run answers and ends: the child is not counted for good.
    walk.through("\"run_id\":\"run-2\",\"event\":{\"kind\":\"terminal\"");
    let done = walk.at(END_MS + 61_000);
    assert_eq!(done.settlement, Settlement::Settled);
    assert_eq!(done.pending, Pending::default());
}

#[test]
fn an_ended_child_stays_ended_whatever_comes_after_it() {
    let mut lines: Vec<String> = MADE.lines().map(str::to_owned).collect();
    let at = lines
        .iter()
        .position(|line| {
            line.contains("\"child_id\":\"child-b\"") && line.contains("\"status\":\"terminal\"")
        })
        .unwrap();
    let lifecycle = |sequence: u64, status: &str| {
        run_line(
            sequence,
            json!({ "kind": "workflow_child_lifecycle", "workflow_run_id": "wf-1",
            "child_id": "child-b", "attempt": 2, "status": status, "task_id": "task-child-b" }),
        )
    };
    lines.insert(at + 1, lifecycle(9001, "scheduled"));
    lines.insert(at + 2, lifecycle(9002, "started"));
    lines.insert(
        at + 3,
        child_record(9003, "attempt_admitted", "child-b", json!({})).to_string(),
    );
    let mut walk = Walk::new(lines);
    walk.through("\"attempt\":2,\"status\":\"started\"");
    assert_eq!(walk.at(END_MS - 1).pending.agents, 4, "b ended");
    walk.through("\"kind\":\"attempt_admitted\",\"subagent_id\":\"child-b\"");
    assert_eq!(walk.at(END_MS - 1).pending.agents, 4, "b stays ended");
    walk.through("\"payload_type\":\"session.end\"");
    assert_eq!(walk.at(END_MS + 1).pending, Pending::default());
}

//! The work a turn leaves running, on records made here in the shapes the live log
//! shows (`live`), for what no live log holds yet: work that outlives its turn, the run
//! that wakes for it, work stopped, a child that ends after its parent's turn, work a
//! process left, and the neighbouring shapes of each.

use serde_json::{json, Value};

use super::{fixture, intent, read_to, record_of};
use crate::records::{Role, Settlement};
use crate::shared::record::reading::Pending;

/// A task's event of the run `run`.
fn task(sequence: u64, run: &str, task: &str, event: Value) -> Value {
    let mut event = event;
    event["task_id"] = json!(task);
    json!({
        "schema_version": 1, "id": format!("r{sequence}"), "sequence": sequence,
        "stream": { "kind": "session", "id": "s1" }, "payload_type": "runtime.session",
        "payload": { "kind": "task", "run_id": run, "task_id": task, "event": event },
    })
}

/// A task's output that says what `chunk` says.
fn output(sequence: u64, run: &str, id: &str, chunk: Value) -> Value {
    task(
        sequence,
        run,
        id,
        json!({ "kind": "output", "chunk": chunk.to_string() }),
    )
}

/// An item queued for the agent, from `source`, the record `item`.
fn queued(sequence: u64, run: &str, item: &str, source: Value) -> Value {
    let mut record = run_event(
        sequence,
        run,
        json!({ "kind": "inbox_item_queued", "source": source, "body": "done" }),
    );
    record["payload"]["source_run_record_id"] = json!(item);
    record
}

/// The item `item` drained into the run `into`.
fn drained(sequence: u64, run: &str, item: &str, into: &str) -> Value {
    run_event(
        sequence,
        run,
        json!({ "kind": "inbox_item_drained", "item_id": item,
                "drain_target_run_stream": { "kind": "run", "id": into } }),
    )
}

fn run_event(sequence: u64, run: &str, event: Value) -> Value {
    super::run(sequence, run, event)
}

/// A child's control record.
fn child(sequence: u64, kind: &str, more: Value) -> Value {
    let mut record = json!({ "kind": kind, "subagent_id": "c1" });
    for (name, value) in more.as_object().cloned().unwrap_or_default() {
        record[name] = value;
    }
    json!({
        "schema_version": 1, "id": format!("r{sequence}"), "sequence": sequence,
        "stream": { "kind": "session", "id": "s1" },
        "payload_type": format!("subagent.control.{kind}"),
        "payload": { "kind": "subagent_control", "record": record },
    })
}

fn session(sequence: u64, payload_type: &str) -> Value {
    json!({
        "schema_version": 1, "id": format!("r{sequence}"), "sequence": sequence,
        "stream": { "kind": "session", "id": "s1" }, "payload_type": payload_type,
        "payload": { "kind": "session" },
    })
}

fn started(sequence: u64, run: &str) -> Value {
    run_event(sequence, run, json!({ "kind": "started" }))
}

fn ended(sequence: u64, run: &str, terminal: &str) -> Value {
    run_event(
        sequence,
        run,
        json!({ "kind": "terminal", "terminal": terminal }),
    )
}

/// A turn that answered and left the command `k1` running in the background.
fn left_running() -> Vec<Value> {
    vec![
        intent(1, "t1", "Start it and end the turn."),
        started(2, "t1"),
        task(3, "t1", "k1", json!({ "kind": "started" })),
        output(
            4,
            "t1",
            "k1",
            json!({ "execution_state": "background_running", "work_id": "work.v2.managed_bash.x" }),
        ),
        run_event(
            5,
            "t1",
            json!({ "kind": "assistant_message_committed", "message_id": "m1", "text": "Started." }),
        ),
        ended(6, "t1", "completed"),
    ]
}

fn pending(jobs: usize, agents: usize, retired: usize) -> Pending {
    Pending {
        jobs,
        agents,
        retired,
        noticed: 0,
    }
}

#[test]
fn a_command_that_outlives_its_turn_is_pending_until_the_run_it_wakes_has_begun() {
    let mut records = left_running();
    let left = record_of(&records);
    assert_eq!(left.settlement, Settlement::Settled, "the turn is over");
    assert_eq!(left.pending, pending(1, 0, 0), "and its command runs");
    // Its end is queued, its task ends: one command, its agent not told yet.
    records.push(queued(7, "t1", "i1", json!({ "source": "background_task_terminal", "task_id": "k1", "terminal_kind": "completed" })));
    records.push(task(8, "t1", "k1", json!({ "kind": "completed" })));
    assert_eq!(record_of(&records).pending, pending(1, 0, 0));
    // Drained into the run it wakes, which has not begun: still waiting on it.
    records.push(drained(9, "t1", "i1", "t2"));
    let draining = record_of(&records);
    assert_eq!(draining.settlement, Settlement::Settled);
    assert_eq!(
        draining.pending,
        pending(1, 0, 0),
        "between the drain and the wake"
    );
    // The run it woke is under way, and then over, with nothing left.
    records.push(started(10, "t2"));
    let woken = record_of(&records);
    assert_eq!(woken.settlement, Settlement::InFlight);
    assert_eq!(woken.pending, Pending::default());
    records.push(run_event(11, "t2", json!({ "kind": "assistant_message_committed", "message_id": "m2", "text": "It printed done." })));
    records.push(ended(12, "t2", "completed"));
    let done = record_of(&records);
    assert_eq!(done.settlement, Settlement::Settled);
    assert_eq!(done.pending, Pending::default());
    // What it delivered is the harness's own words.
    assert!(done
        .items
        .iter()
        .any(|item| item.role == Role::Custom && &*item.text == "done"));
}

#[test]
fn a_task_still_open_when_its_run_ends_is_background_work_whatever_it_said() {
    let records = [
        started(2, "t1"),
        task(3, "t1", "k1", json!({ "kind": "started" })),
        output(4, "t1", "k1", json!({ "exit_code": null })),
    ];
    assert_eq!(
        record_of(&records).pending,
        Pending::default(),
        "the turn's own, so far"
    );
    let mut records = records.to_vec();
    records.push(ended(5, "t1", "completed"));
    assert_eq!(record_of(&records).pending, pending(1, 0, 0));
    // A foreground task that ended before its run, and one of another run, are not.
    let records = [
        started(2, "t1"),
        task(3, "t1", "k1", json!({ "kind": "started" })),
        task(4, "t1", "k1", json!({ "kind": "completed" })),
        task(5, "t2", "k2", json!({ "kind": "started" })),
        ended(6, "t1", "completed"),
    ];
    assert_eq!(record_of(&records).pending, Pending::default());
}

#[test]
fn a_background_command_stopped_ends_its_work_with_no_word_to_wait_for() {
    for end in ["cancelled", "failed", "rejected"] {
        let mut records = left_running();
        records.push(task(
            7,
            "t1",
            "k1",
            json!({ "kind": end, "reason": "stopped by work_stop" }),
        ));
        assert_eq!(record_of(&records).pending, Pending::default(), "{end}");
    }
    // A status or an output is no end.
    let mut records = left_running();
    records.push(task(
        7,
        "t1",
        "k1",
        json!({ "kind": "status", "message": "" }),
    ));
    records.push(output(
        8,
        "t1",
        "k1",
        json!({ "exit_code": 0, "terminal_status": "completed" }),
    ));
    assert_eq!(record_of(&records).pending, pending(1, 0, 0));
}

#[test]
fn a_monitors_events_wait_for_the_agent_each_counted_with_the_monitor_once() {
    let mut records = vec![
        started(2, "t1"),
        task(3, "t1", "w1", json!({ "kind": "started" })),
        output(
            4,
            "t1",
            "w1",
            json!({ "work_id": "work.v2.monitor.sha256.x", "timeout_ms": 30000 }),
        ),
        ended(5, "t1", "completed"),
    ];
    assert_eq!(record_of(&records).pending, pending(1, 0, 0));
    let event = |sequence, item: &str| {
        queued(
            sequence,
            "t1",
            item,
            json!({ "source": "monitor_event", "task_id": "w1" }),
        )
    };
    records.extend([
        event(6, "e1"),
        event(7, "e2"),
        task(8, "t1", "w1", json!({ "kind": "completed" })),
    ]);
    assert_eq!(record_of(&records).pending, pending(1, 0, 0), "one monitor");
    records.extend([started(9, "t2"), drained(10, "t2", "e1", "t2")]);
    assert_eq!(
        record_of(&records).pending,
        pending(1, 0, 0),
        "an event still waits"
    );
    records.push(drained(11, "t2", "e2", "t2"));
    assert_eq!(record_of(&records).pending, Pending::default());
}

#[test]
fn a_child_that_ends_after_its_parents_turn_is_pending_until_its_result_is_taken_in() {
    let mut records = vec![
        started(2, "t1"),
        child(3, "spawn_accepted", json!({ "parent_run_id": "t1" })),
        child(4, "status_updated", json!({ "control_status": "running" })),
        ended(5, "t1", "completed"),
    ];
    let left = record_of(&records);
    assert_eq!(left.settlement, Settlement::Settled);
    assert_eq!(left.pending, pending(0, 1, 0));
    records.push(child(6, "result_ready", json!({ "text": "CHILD_DONE" })));
    records.push(queued(
        7,
        "t1",
        "i1",
        json!({ "source": "subagent_result", "subagent_id": "c1" }),
    ));
    assert_eq!(
        record_of(&records).pending,
        pending(0, 1, 0),
        "its result not taken in"
    );
    records.extend([started(8, "t2"), drained(9, "t2", "i1", "t2")]);
    records.push(ended(10, "t2", "completed"));
    let woken = record_of(&records);
    assert_eq!(
        woken.settlement,
        Settlement::Settled,
        "the run its result woke"
    );
    assert_eq!(
        woken.pending,
        Pending::default(),
        "its result taken in, the child is done"
    );
    // A status that says it ended ends it too (provisional: the binary's strings).
    let failed = [
        started(2, "t1"),
        child(3, "spawn_accepted", json!({})),
        child(4, "status_updated", json!({ "control_status": "failed" })),
        ended(5, "t1", "completed"),
    ];
    assert_eq!(record_of(&failed).pending, Pending::default());
}

#[test]
fn what_a_process_left_running_is_retired_when_its_segment_ends_and_a_resume_reads_it_so() {
    let mut records = left_running();
    records.push(child(7, "spawn_accepted", json!({})));
    records.push(queued(8, "t1", "i1", json!({ "source": "overdue_notice" })));
    records.push(session(9, "session.end"));
    let ended_there = record_of(&records);
    assert_eq!(ended_there.settlement, Settlement::Settled);
    assert_eq!(
        ended_there.pending,
        pending(0, 0, 3),
        "a command, a child, a notice"
    );
    // The session resumed: its new segment begins with nothing of the old one running,
    // and a fresh reader of the whole log reads the same.
    records.extend([
        session(10, "session.started"),
        intent(11, "t3", "Go on."),
        started(12, "t3"),
        ended(13, "t3", "completed"),
    ]);
    let resumed = record_of(&records);
    assert_eq!(resumed.settlement, Settlement::Settled);
    assert_eq!(resumed.pending, pending(0, 0, 3));
    // A process killed with its work running wrote no end: it waits on it, until a
    // resume writes the dead process's end.
    let killed = left_running();
    assert_eq!(record_of(&killed).pending, pending(1, 0, 0));
}

#[test]
fn the_users_message_pasted_mid_turn_keeps_the_turn_in_flight_until_a_run_takes_it() {
    let steer = |sequence| queued(sequence, "t1", "s1", json!({ "source": "user_steer" }));
    let mut records = vec![
        intent(1, "t1", "One."),
        started(2, "t1"),
        steer(3),
        ended(4, "t1", "completed"),
    ];
    let queued_up = record_of(&records);
    assert_eq!(queued_up.settlement, Settlement::InFlight);
    assert_eq!(
        queued_up.pending,
        Pending::default(),
        "the user's words are no work"
    );
    records.extend([drained(5, "t2", "s1", "t2"), started(6, "t2")]);
    assert_eq!(
        record_of(&records).settlement,
        Settlement::InFlight,
        "its run"
    );
    records.push(ended(7, "t2", "completed"));
    assert_eq!(record_of(&records).settlement, Settlement::Settled);
    // Escape puts queued messages back in the box: after a cancelled turn, no wait.
    let cancelled = [
        intent(1, "t1", "One."),
        started(2, "t1"),
        steer(3),
        ended(4, "t1", "cancelled"),
    ];
    assert_eq!(record_of(&cancelled).settlement, Settlement::Settled);
}

#[test]
fn a_turn_whose_foreground_is_done_is_in_flight_while_its_reminders_run_and_leaves_nothing() {
    // The stretch `turn/foregroundCompleted` names on the wire: the agent's answer is
    // in, Muse's reminder children run, and the turn is not over until its `terminal`.
    let log = fixture("plain");
    let kinds: Vec<Option<String>> = log
        .lines()
        .map(|line| {
            let record: Value = serde_json::from_str(line).unwrap();
            (record["payload"]["kind"] == "run")
                .then(|| {
                    record["payload"]["event"]["kind"]
                        .as_str()
                        .map(str::to_owned)
                })
                .flatten()
        })
        .collect();
    let at = |kind: &str| {
        kinds
            .iter()
            .position(|found| found.as_deref() == Some(kind))
            .unwrap()
    };
    let (answer, terminal) = (at("assistant_message_committed"), at("terminal"));
    assert!(terminal > answer + 5, "reminders run between them");
    for at in answer + 1..terminal {
        let record = read_to("plain", at + 1);
        assert_eq!(record.settlement, Settlement::InFlight, "line {at}");
    }
    let over = read_to("plain", terminal + 1);
    assert_eq!(over.settlement, Settlement::Settled);
    assert_eq!(
        over.pending,
        Pending::default(),
        "the reminders were no work of the agent's"
    );
}

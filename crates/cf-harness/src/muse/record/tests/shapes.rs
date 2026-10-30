//! The reader on T-801's live recordings (`tests/fixtures/muse/live`, Muse 1.4.13 on
//! `muse-spark-1.3` at `minimal`, contributor mode off; the README there says what each
//! holds): work that outlives its turn and the run it wakes, a child its parent did not
//! wait on, a command stopped by the model and by the human, a process killed and
//! resumed, a goal, and an Escape before a word. Each is read at the physical lines
//! T-801's README names, and read on a record at a time the same as fresh.

use super::*;
use crate::shared::record::reading::Pending;

fn log(name: &str) -> &'static str {
    match name {
        "monitor" => {
            include_str!("../../../../tests/fixtures/muse/live/monitor-outlives-turn.jsonl")
        }
        "child" => include_str!("../../../../tests/fixtures/muse/live/child-not-waited.jsonl"),
        "stop-model" => {
            include_str!("../../../../tests/fixtures/muse/live/stopped-by-the-model.jsonl")
        }
        "stop-human" => {
            include_str!("../../../../tests/fixtures/muse/live/stopped-by-the-human.jsonl")
        }
        "kill-resume" => {
            include_str!("../../../../tests/fixtures/muse/live/killed-and-resumed.jsonl")
        }
        "goal" => include_str!("../../../../tests/fixtures/muse/live/goal.jsonl"),
        "escape" => include_str!("../../../../tests/fixtures/muse/live/escape-before-a-word.jsonl"),
        other => panic!("no recording {other}"),
    }
}

const SHAPES: [&str; 7] = [
    "monitor",
    "child",
    "stop-model",
    "stop-human",
    "kill-resume",
    "goal",
    "escape",
];

/// What a fresh reader reads of the first `lines` lines of the recording `name`.
fn at(name: &str, lines: usize) -> Record {
    let text = log(name);
    let head: String = text
        .lines()
        .take(lines)
        .map(|line| format!("{line}\n"))
        .collect();
    known(&fresh_reading(&session_of(text), &head)).clone()
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
fn every_shape_reads_the_same_read_on_a_record_at_a_time_as_read_fresh() {
    for name in SHAPES {
        let text = log(name);
        let session = session_of(text);
        let stage = Stage::new(&session);
        let mut reading_on = stage.reader();
        let mut so_far = String::new();
        for (at, line) in text.lines().enumerate() {
            so_far.push_str(line);
            so_far.push('\n');
            stage.append(line);
            let on = look(&mut reading_on);
            if at % 10 == 0 || line.contains("\"kind\":\"terminal\"") {
                assert_eq!(
                    on,
                    fresh_reading(&session, &so_far),
                    "{name}, line {}",
                    at + 1
                );
            }
        }
    }
}

#[test]
fn a_monitor_that_outlives_its_turn_holds_it_until_the_run_it_wakes_takes_its_events() {
    // The turn ends with the monitor watching (95); its events are queued under the
    // ended run (98, 99) and its task completes (100): its agent not told yet.
    for line in [95, 98, 99, 100, 102] {
        let record = at("monitor", line);
        assert_eq!(record.settlement, Settlement::Settled, "line {line}");
        assert_eq!(record.pending, pending(1, 0, 0), "line {line}");
    }
    // Another run starts (103) and drains both events (115, 116).
    assert_eq!(at("monitor", 103).settlement, Settlement::InFlight);
    assert_eq!(at("monitor", 115).pending.jobs, 1, "one event still waits");
    assert_eq!(at("monitor", 116).pending, Pending::default());
    let done = at("monitor", 136);
    assert_eq!(done.settlement, Settlement::Settled);
    assert_eq!(done.pending, Pending::default());
}

#[test]
fn a_child_its_parent_did_not_wait_on_is_an_agent_pending_until_its_result_is_taken_in() {
    // Spawned (67), running (77), the parent's turn over (104).
    let left = at("child", 104);
    assert_eq!(left.settlement, Settlement::Settled);
    assert_eq!(left.pending, pending(0, 1, 0));
    // The child's result ready (107) and queued under the old run (108).
    for line in [106, 107, 108, 110] {
        assert_eq!(at("child", line).pending, pending(0, 1, 0), "line {line}");
    }
    // The run it wakes (111) takes it in (123) and ends (143).
    assert_eq!(at("child", 111).settlement, Settlement::InFlight);
    let done = at("child", 143);
    assert_eq!(done.settlement, Settlement::Settled);
    assert_eq!(done.pending, Pending::default());
}

#[test]
fn a_command_the_model_stopped_ends_its_work_and_its_end_wakes_the_agent() {
    // Running in the background (74), stopped by `work_stop` (91, accepted at 109): its
    // end is queued (120) and its task cancelled (122, `terminated by bash_input`).
    assert_eq!(at("stop-model", 109).pending.jobs, 1, "accepted is no end");
    // The turn ends (134) with the end not taken in: held, as any delivery.
    let ended = at("stop-model", 134);
    assert_eq!(ended.settlement, Settlement::Settled);
    assert_eq!(ended.pending, pending(1, 0, 0));
    // The run it wakes (138) drains it (150) and ends (170).
    assert_eq!(at("stop-model", 150).pending, Pending::default());
    let done = at("stop-model", 170);
    assert_eq!(done.settlement, Settlement::Settled);
    assert_eq!(done.pending, Pending::default());
}

#[test]
fn a_command_the_human_stopped_from_the_task_list_ends_its_work_the_same_way() {
    // Running (74), the turn over (95): held.
    assert_eq!(at("stop-human", 95).pending, pending(1, 0, 0));
    // Stopped from `/tasks`: its end queued (99), its task cancelled (101, `cancelled by
    // runtime client`), not yet taken in.
    assert_eq!(at("stop-human", 101).pending, pending(1, 0, 0));
    // Woken (104), drained (116), over (136).
    assert_eq!(at("stop-human", 116).pending, Pending::default());
    let done = at("stop-human", 136);
    assert_eq!(done.settlement, Settlement::Settled);
    assert_eq!(done.pending, Pending::default());
}

#[test]
fn a_command_a_killed_process_left_is_retired_then_ended_by_the_resumes_word() {
    // Killed with the command running (the log as it stood: 95 lines).
    let killed = at("kill-resume", 95);
    assert_eq!(killed.settlement, Settlement::Settled);
    assert_eq!(killed.pending, pending(1, 0, 0));
    // The resume writes the dead process's end (96): no Muse that runs will tell its
    // agent of the command, so it is retired, not waited on.
    assert_eq!(at("kill-resume", 96).pending, pending(0, 0, 1));
    // Then Muse cancels the task itself (106, `resume_reconcile:orphaned_by_process_loss`):
    // word came that it ended, so it is retired no more.
    let reconciled = at("kill-resume", 106);
    assert_eq!(reconciled.settlement, Settlement::Settled);
    assert_eq!(reconciled.pending, Pending::default());
    assert_eq!(at("kill-resume", 109).pending, Pending::default());
}

#[test]
fn a_goal_set_is_the_users_input_and_its_run_holds_it_until_the_goal_is_closed() {
    // The `/goal` the user typed is queued (24) before any run: the turn it starts is
    // coming, and it is no work running.
    let set = at("goal", 24);
    assert_eq!(set.settlement, Settlement::InFlight);
    assert_eq!(set.pending, Pending::default());
    // Drained into its run (42) and bound to it (43): the run is the goal's work.
    for line in [42, 43, 57, 129, 145] {
        let record = at("goal", line);
        assert_eq!(record.settlement, Settlement::InFlight, "line {line}");
        assert_eq!(record.pending, Pending::default(), "line {line}");
    }
    // `update_goal` says it is complete (145), and the run ends (165): nothing pending.
    let done = at("goal", 165);
    assert_eq!(done.settlement, Settlement::Settled);
    assert_eq!(done.pending, Pending::default());
}

#[test]
fn an_escape_before_a_word_on_a_real_model_takes_the_message_back() {
    let stopped = at("escape", 46);
    assert_eq!(
        stopped.settlement,
        Settlement::Settled,
        "cancelled during model step"
    );
    assert!(!stopped.took_back, "not yet said to be put back");
    let taken = at("escape", 47);
    assert!(
        taken.took_back,
        "run_retracted, no_output_interrupt_restore"
    );
    assert!(!taken.failed);
    assert!(taken.items.iter().all(|item| item.role != Role::Assistant));
    assert_eq!(taken.items.last().map(|item| item.role), Some(Role::User));
}

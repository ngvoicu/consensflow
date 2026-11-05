//! The last need taken from a task given to a window (T-798, 3.0.7; the field report of
//! 2026-10-27). The chief holds work behind a step of its own ("a stage" waiting for a
//! release) and, with the machine idle, was told to start a few of the held tasks early.
//! `cf task done` of a step that had not arrived was refused, and nothing dropped a need
//! from a task, so the chief cancelled eleven tasks and added them again. `cf task edit T-n
//! --needs -T-m` (T-1108) drops one now, but a task given to a window was found for its
//! release by the needs it still had, so taking its last need left it open for good, waiting
//! for nothing. It is released as if the need had been accepted: into its window when that is
//! free, and once the window is, when it is not; the stage itself is untouched.

#![allow(clippy::expect_used)]

use cf_ledger::{
    open_ledger, Ledger, Links, NewChief, NewMember, NewProject, NewTask, Options, TaskEdit,
};
use serde_json::json;

fn ledger_with_a_project(dir: &tempfile::TempDir) -> Ledger {
    let mut ledger =
        open_ledger(&dir.path().join("consensflow.db"), Options::default()).expect("a ledger");
    ledger
        .create_project(&NewProject {
            directory: "/work/app".into(),
            name: "app".into(),
            chief: NewChief {
                harness: "claude-code".into(),
                model: Some("apollo".into()),
                effort: None,
            },
            staff: vec![NewMember {
                model: None,
                effort: None,
                model_name: Some("zeus".into()),
                harness: "claude-code".into(),
                designer: false,
                roles: vec!["worker".into()],
                tier: "standard".into(),
            }],
            gate: false,
            wake_up: None,
            specs: None,
        })
        .expect("a project");
    ledger
}

/// The stage: a task for a standard worker nobody has taken, so it stays open.
fn add_the_stage(ledger: &mut Ledger) {
    ledger
        .create_task(
            1,
            &NewTask {
                from: "chief".into(),
                pool: Some("worker".into()),
                tier: Some("standard".into()),
                body: "Stage".into(),
                ..NewTask::default()
            },
        )
        .expect("the stage");
}

/// The chief's own step (`cf task add --self --needs …`), waiting for `needs`.
fn add_own_step(ledger: &mut Ledger, body: &str, needs: &[u64]) {
    ledger
        .create_task(
            1,
            &NewTask {
                from: "chief".into(),
                to: Some("chief".into()),
                body: body.into(),
                needs: needs.to_vec(),
                ..NewTask::default()
            },
        )
        .expect("an own step");
}

fn let_go_of(ledger: &mut Ledger, number: i64, need: i64) {
    ledger
        .edit_task_with(
            1,
            number,
            "chief",
            &TaskEdit {
                needs: Some(Links::Change {
                    add: Vec::new(),
                    remove: vec![need],
                }),
                ..TaskEdit::default()
            },
        )
        .expect("the need is taken");
}

fn state(ledger: &Ledger, number: i64) -> String {
    ledger
        .task(1, number)
        .expect("a thread")
        .expect("a task")
        .task
        .state
}

fn waits_for(ledger: &Ledger, number: i64) -> Vec<i64> {
    ledger
        .task(1, number)
        .expect("a thread")
        .expect("a task")
        .task
        .blocked_by
}

/// The states of the task messages of T-`number`.
fn briefs(ledger: &Ledger, number: i64) -> Vec<String> {
    ledger
        .task(1, number)
        .expect("a thread")
        .expect("a task")
        .messages
        .into_iter()
        .filter(|message| message.kind == "task")
        .map(|message| format!("{} {}", message.recipient, message.state))
        .collect()
}

#[test]
fn a_stage_holding_several_own_steps_lets_go_of_only_the_one_whose_need_is_taken() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger = ledger_with_a_project(&dir);
    add_the_stage(&mut ledger);
    add_own_step(&mut ledger, "Notes", &[1]);
    add_own_step(&mut ledger, "Docs", &[1]);
    add_own_step(&mut ledger, "Release", &[1]);
    for number in 2..=4 {
        assert_eq!(state(&ledger, number), "open", "T-{number}");
        assert_eq!(waits_for(&ledger, number), [1], "T-{number}");
    }

    let_go_of(&mut ledger, 3, 1);

    assert_eq!(
        state(&ledger, 3),
        "queued",
        "its brief goes back to the chief, as if T-1 had been accepted"
    );
    assert_eq!(briefs(&ledger, 3), ["chief queued"]);
    assert_eq!(waits_for(&ledger, 3), Vec::<i64>::new());
    for number in [2, 4] {
        assert_eq!(state(&ledger, number), "open", "T-{number} still waits");
        assert_eq!(waits_for(&ledger, number), [1], "T-{number}");
        assert_eq!(briefs(&ledger, number), Vec::<String>::new(), "T-{number}");
    }
    assert_eq!(state(&ledger, 1), "open", "the stage is as it was");
}

#[test]
fn a_follow_up_let_go_of_the_task_it_follows_still_waits_for_its_window_and_is_not_stranded() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger = ledger_with_a_project(&dir);
    // T-1 at work in a session of zeus, and a follow-up (T-2) for that window, waiting behind it.
    add_the_stage(&mut ledger);
    let zeus = ledger
        .project(1)
        .expect("a project")
        .expect("project 1")
        .participants
        .into_iter()
        .find(|participant| participant.handle == "zeus")
        .expect("zeus")
        .id;
    let brief = ledger
        .assign_task(1, 1, zeus)
        .expect("assigned")
        .message
        .expect("its brief")
        .id;
    ledger.begin_delivery(brief).expect("its delivery begins");
    ledger
        .confirm_delivery(brief, Some(&json!({ "item": "i-1" })))
        .expect("its delivery is confirmed");
    ledger
        .create_task(
            1,
            &NewTask {
                from: "chief".into(),
                after: Some(1),
                body: "Part 2".into(),
                ..NewTask::default()
            },
        )
        .expect("a follow-up");
    assert_eq!(
        (state(&ledger, 2).as_str(), waits_for(&ledger, 2)),
        ("open", vec![1])
    );

    // The need is taken. The window has T-1 in hand, so T-2 cannot go in yet, and it must still go
    // in once the window is free: the need rows it was found by are gone.
    let_go_of(&mut ledger, 2, 1);
    assert_eq!(waits_for(&ledger, 2), Vec::<i64>::new());
    assert_eq!(state(&ledger, 2), "open", "the window is on T-1");

    ledger
        .record_result(1, 1, "Part 1 done")
        .expect("T-1 is done");
    assert_eq!(
        state(&ledger, 2),
        "open",
        "a window with a result undecided takes nothing new (T-1128)"
    );
    ledger.accept_task(1, 1, "chief").expect("T-1 is accepted");
    assert_eq!(
        state(&ledger, 2),
        "queued",
        "the window is free, and T-2 goes in"
    );
}

//! Held work let go early (`cf task release`, T-798, 3.0.7; the field report of 2026-10-27).
//! The chief holds work behind a step ("stage") that waits for a release, and with the machine
//! idle the human said to start a few of the held tasks early: `cf task done` was refused for a
//! step that had not arrived, no command dropped a need, and cancelling the stage would have
//! blocked everything that needs it for good, so the chief cancelled eleven tasks and added them
//! again. A task is now let go of what it waits for, by name: only the named tasks go, the stage
//! stays as it was, and a follow-up is never let go of the task whose window it goes into.

#![allow(clippy::expect_used)]

use cf_ledger::{
    open_ledger, Ledger, NewChief, NewMember, NewProject, NewTask, Options, TaskLetGo,
};
use serde_json::json;

fn options() -> Options {
    let mut names = ["amber-pine", "brisk-birch", "calm-brook"]
        .into_iter()
        .cycle();
    Options {
        names: Box::new(move || names.next().unwrap_or_default().to_owned()),
        ..Options::default()
    }
}

/// A project at /work/app: apollo its chief, zeus its standard worker.
fn ledger_with_a_project(dir: &tempfile::TempDir) -> Ledger {
    let mut ledger = open_ledger(&dir.path().join("consensflow.db"), options()).expect("ledger");
    ledger
        .create_project(&NewProject {
            directory: "/work/app".into(),
            name: "app".into(),
            chief: NewChief {
                harness: "claude-code".into(),
                agent: Some("apollo".into()),
            },
            staff: vec![NewMember {
                agent: "zeus".into(),
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

/// The chief adds a task for a standard worker, to be given to `to` by name when it is one,
/// waiting for `needs`.
fn add(ledger: &mut Ledger, body: &str, to: Option<&str>, needs: &[u64]) {
    ledger
        .create_task(
            1,
            &NewTask {
                from: "chief".into(),
                to: to.map(str::to_owned),
                pool: to.is_none().then(|| "worker".to_owned()),
                tier: to.is_none().then(|| "standard".to_owned()),
                body: body.into(),
                needs: needs.to_vec(),
                ..NewTask::default()
            },
        )
        .expect("a task");
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

fn let_go(
    ledger: &mut Ledger,
    number: i64,
    from: Option<i64>,
) -> Result<TaskLetGo, (Option<&'static str>, String)> {
    ledger
        .let_go(1, number, "chief", from)
        .map_err(|error| (error.code(), error.to_string()))
}

#[test]
fn a_stage_holding_several_tasks_lets_go_of_only_the_ones_named() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger = ledger_with_a_project(&dir);
    // T-1 is the stage; four tasks wait for it: two for a tier, one for zeus by name, and the
    // chief's own step.
    add(&mut ledger, "Stage", None, &[]);
    add(&mut ledger, "Lexer", None, &[1]);
    add(&mut ledger, "Parser", None, &[1]);
    add(&mut ledger, "Docs", Some("zeus"), &[1]);
    add(&mut ledger, "Release notes", Some("chief"), &[1]);
    for number in 2..=5 {
        assert_eq!(waits_for(&ledger, number), [1], "T-{number}");
        assert_eq!(state(&ledger, number), "open", "T-{number}");
    }

    // The tier task goes on the board for a member, and nothing else moved.
    let lexer = let_go(&mut ledger, 2, Some(1)).expect("T-2 is let go");
    assert_eq!(lexer.from, [1]);
    assert_eq!(waits_for(&ledger, 2), Vec::<i64>::new());
    assert_eq!(
        state(&ledger, 2),
        "open",
        "a task for a tier waits for a member"
    );
    for number in [3, 4, 5] {
        assert_eq!(waits_for(&ledger, number), [1], "T-{number} still waits");
    }
    assert_eq!(state(&ledger, 1), "open", "the stage itself is as it was");

    // Given to a window by name, or the chief's own, the task goes into it at once, with its brief.
    for number in [4, 5] {
        let released = let_go(&mut ledger, number, None).expect("let go of every need");
        assert_eq!(released.from, [1], "T-{number}");
        assert_eq!(
            state(&ledger, number),
            "queued",
            "T-{number} goes to its window"
        );
        let thread = ledger.task(1, number).expect("a thread").expect("a task");
        assert!(
            thread
                .messages
                .iter()
                .any(|message| message.kind == "task" && message.state == "queued"),
            "T-{number}'s brief is on its way"
        );
    }
    assert_eq!(waits_for(&ledger, 3), [1], "the one not named still waits");

    // What is left waits as before, and the stage still cannot be cancelled under it.
    let refused = ledger
        .cancel_task(1, 1, "chief")
        .expect_err("T-2 and T-3 still wait for it");
    assert_eq!(refused.code(), Some("waited-for"));
}

#[test]
fn a_task_that_waits_for_nothing_or_not_for_the_task_named_is_refused_with_the_way_it_stands() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger = ledger_with_a_project(&dir);
    add(&mut ledger, "Stage", None, &[]);
    add(&mut ledger, "Lexer", None, &[1]);
    add(&mut ledger, "Parser", None, &[1, 2]);

    assert_eq!(
        let_go(&mut ledger, 1, None).err(),
        Some((
            Some("not-waiting"),
            "T-1 waits for no task that is not accepted".to_owned()
        ))
    );
    assert_eq!(
        let_go(&mut ledger, 2, Some(3)).err(),
        Some((
            Some("not-waiting"),
            "T-2 does not wait for T-3; it waits for T-1".to_owned()
        )),
        "a need it does not have"
    );
    assert_eq!(waits_for(&ledger, 2), [1], "a refusal changes nothing");
    // One need of two, named: the other stays.
    let let_go_of_one = let_go(&mut ledger, 3, Some(2)).expect("T-3 is let go of T-2");
    assert_eq!(let_go_of_one.from, [2]);
    assert_eq!(waits_for(&ledger, 3), [1]);
    // None named: every need not yet accepted.
    let everything = let_go(&mut ledger, 3, None).expect("T-3 is let go of the rest");
    assert_eq!(everything.from, [1]);
    assert_eq!(waits_for(&ledger, 3), Vec::<i64>::new());
}

#[test]
fn only_an_open_or_paused_task_is_let_go_and_a_follow_up_is_never_let_go_of_the_task_it_follows() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger = ledger_with_a_project(&dir);
    add(&mut ledger, "Review", None, &[]);
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
    assert_eq!(state(&ledger, 1), "working");

    // A task in its window waits for nothing.
    assert_eq!(
        let_go(&mut ledger, 1, None).err(),
        Some((
            Some("not-waiting"),
            "T-1 is working: only an open or paused task can be let go of what it waits for"
                .to_owned()
        ))
    );

    // A follow-up of a task at work waits behind it, for the window that is on it.
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
    assert_eq!(state(&ledger, 2), "open");
    assert_eq!(waits_for(&ledger, 2), [1]);
    assert_eq!(
        let_go(&mut ledger, 2, None).err(),
        Some((
            Some("follows-in-window"),
            "T-2 follows T-1 in the window that has it: it goes in once T-1 is accepted, which no release hurries; cancel T-2 and add it for a tier if it should not wait"
                .to_owned()
        ))
    );
    assert_eq!(waits_for(&ledger, 2), [1], "a refusal changes nothing");
}

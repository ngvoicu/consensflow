//! Deleting a finished task another task needs (T-721, 3.0.6; Gabriel's board
//! of 2026-10-26: 48 finished tasks, every one needed by a task the chief held,
//! so none could be deleted and the Finished column could not be cleared).
//!
//! Deleting only takes a task off the board (`deleted_at`): what waits for it
//! reads its state as before, and `release_ready` lets a waiter go once every
//! task it needs is accepted, deleted or not. So a finished task is kept only
//! while an unfinished task needs it *and it can still change*: a failed task,
//! which the chief can reopen and then accept (a deleted task is not reopened).
//! An accepted task is accepted for good, and a cancelled one will never be
//! accepted, so each goes though a task needs it.

#![allow(clippy::expect_used)]

use cf_ledger::{
    open_ledger, Board, Ledger, NewChief, NewMember, NewProject, NewTask, Options, ProjectView,
};
use cf_proto::ledger::Need;
use serde_json::json;

/// Session names in a fixed order, so a test can say `zeus-amber-pine`.
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
fn project(ledger: &mut Ledger) -> ProjectView {
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
        .expect("a project")
}

/// The chief opens a task for a standard worker, waiting for `needs`.
fn open(ledger: &mut Ledger, body: &str, needs: &[u64]) {
    ledger
        .create_task(
            1,
            &NewTask {
                from: "chief".into(),
                pool: Some("worker".into()),
                tier: Some("standard".into()),
                body: body.into(),
                needs: needs.to_vec(),
                ..NewTask::default()
            },
        )
        .expect("a task");
}

/// The chief gives a task to zeus by name, waiting for `needs`: it waits on the board.
fn by_name(ledger: &mut Ledger, body: &str, needs: &[u64]) {
    ledger
        .create_task(
            1,
            &NewTask {
                from: "chief".into(),
                to: Some("zeus".into()),
                body: body.into(),
                needs: needs.to_vec(),
                ..NewTask::default()
            },
        )
        .expect("a task given by name");
}

/// The daemon gives the open task `number` to a new session of `member`, whose
/// window takes the brief in, and its turn ends with `result`: the task is done.
fn done(ledger: &mut Ledger, number: i64, member: i64, result: &str) {
    let brief = ledger
        .assign_task(1, number, member)
        .expect("assigned")
        .message
        .expect("its brief")
        .id;
    ledger.begin_delivery(brief).expect("its delivery begins");
    ledger
        .confirm_delivery(brief, Some(&json!({ "item": "i-1" })))
        .expect("its delivery is confirmed");
    ledger.record_result(1, number, result).expect("its result");
}

fn zeus(project: &ProjectView) -> i64 {
    project
        .participants
        .iter()
        .find(|participant| participant.handle == "zeus")
        .expect("zeus")
        .id
}

fn state(ledger: &Ledger, number: i64) -> String {
    ledger
        .task(1, number)
        .expect("a thread")
        .expect("a task")
        .task
        .state
}

/// The numbers of the tasks the board lists, in its lanes and among the open ones.
fn on_board(board: &Board) -> Vec<i64> {
    let mut numbers: Vec<i64> = board
        .open
        .iter()
        .chain(board.lanes.iter().flat_map(|lane| lane.tasks.iter()))
        .map(|task| task.number)
        .collect();
    numbers.sort_unstable();
    numbers
}

/// What deleting `numbers` came to: the refusal's code and words, or none.
fn refused(ledger: &mut Ledger, numbers: &[i64]) -> Option<(&'static str, String)> {
    ledger
        .delete_tasks(1, numbers)
        .err()
        .map(|error| (error.code().expect("a refusal"), error.to_string()))
}

#[test]
fn an_accepted_task_others_wait_for_is_deleted_and_they_are_released_when_the_rest_is_accepted() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger = open_ledger(&dir.path().join("consensflow.db"), options()).expect("ledger");
    let view = project(&mut ledger);
    let member = zeus(&view);
    open(&mut ledger, "Stage one", &[]);
    open(&mut ledger, "Stage two", &[]);
    // The chief holds T-3 for zeus until both stages are accepted.
    by_name(&mut ledger, "Ship", &[1, 2]);
    done(&mut ledger, 1, member, "One");
    done(&mut ledger, 2, member, "Two");
    ledger.accept_task(1, 1, "chief").expect("T-1 accepted");

    let deleted = ledger
        .delete_tasks(1, &[1])
        .expect("T-1 is accepted for good: it goes, though T-3 needs it");
    assert!(deleted[0].deleted_at.is_some(), "T-1 is off the board");
    assert_eq!(on_board(&ledger.board(1).expect("a board")), [2, 3]);

    // T-3 still reads T-1 as accepted, and waits for T-2 alone.
    let held = ledger.task(1, 3).expect("a thread").expect("T-3").task;
    assert_eq!(
        held.needs,
        [
            Need {
                number: 1,
                state: "accepted".into()
            },
            Need {
                number: 2,
                state: "done".into()
            }
        ]
    );
    assert_eq!(held.blocked_by, [2]);
    assert_eq!(held.state, "open");

    // And when T-2 is accepted the deleted T-1 counts as accepted: T-3 goes on.
    ledger.accept_task(1, 2, "chief").expect("T-2 accepted");
    assert_eq!(state(&ledger, 3), "queued", "T-3 is released");
    let released = ledger.task(1, 3).expect("a thread").expect("T-3").task;
    assert_eq!(released.blocked_by, Vec::<i64>::new());
}

#[test]
fn a_cancelled_task_others_wait_for_is_deleted_too_for_it_never_releases_them() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger = open_ledger(&dir.path().join("consensflow.db"), options()).expect("ledger");
    project(&mut ledger);
    // T-1 is zeus's, and T-2 waits for it; the removal cancels T-1 and leaves T-2 waiting.
    by_name(&mut ledger, "Parser", &[]);
    open(&mut ledger, "Docs", &[1]);
    ledger.remove_member(1, "zeus").expect("zeus leaves");
    assert_eq!(state(&ledger, 1), "cancelled");

    ledger
        .delete_tasks(1, &[1])
        .expect("nothing can come of T-1, so it goes");
    assert_eq!(on_board(&ledger.board(1).expect("a board")), [2]);
    let waiter = ledger.task(1, 2).expect("a thread").expect("T-2").task;
    assert_eq!(
        waiter.needs,
        [Need {
            number: 1,
            state: "cancelled".into()
        }],
        "T-2 still says what it waits for"
    );
    assert_eq!(waiter.blocked_by, [1]);
    assert_eq!(waiter.state, "open");
}

#[test]
fn a_failed_task_an_unfinished_one_needs_stays_until_that_one_is_finished() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger = open_ledger(&dir.path().join("consensflow.db"), options()).expect("ledger");
    project(&mut ledger);
    open(&mut ledger, "Parser", &[]);
    open(&mut ledger, "Docs", &[1]);
    ledger
        .fail_task(1, 1, "its launch never came up")
        .expect("T-1 fails");

    // The chief can reopen T-1 and accept it, which releases T-2: a deleted task is not reopened.
    assert_eq!(
        refused(&mut ledger, &[1]),
        Some((
            "task-needed",
            "T-2 still needs T-1, which failed and may be reopened: it stays on the board until T-2 is finished"
                .to_owned()
        ))
    );
    assert_eq!(on_board(&ledger.board(1).expect("a board")), [1, 2]);

    // The waiter ends, and nothing can come of T-1 through it any more.
    ledger.cancel_task(1, 2, "chief").expect("T-2 is cancelled");
    ledger
        .delete_tasks(1, &[1])
        .expect("no unfinished task needs T-1 now");
    assert_eq!(on_board(&ledger.board(1).expect("a board")), [2]);
}

#[test]
fn a_failed_task_says_every_task_that_needs_it() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger = open_ledger(&dir.path().join("consensflow.db"), options()).expect("ledger");
    project(&mut ledger);
    open(&mut ledger, "Parser", &[]);
    open(&mut ledger, "Docs", &[1]);
    open(&mut ledger, "Tests", &[1]);
    ledger
        .fail_task(1, 1, "its launch never came up")
        .expect("T-1 fails");
    assert_eq!(
        refused(&mut ledger, &[1]),
        Some((
            "task-needed",
            "T-2, T-3 still need T-1, which failed and may be reopened: it stays on the board until they are finished"
                .to_owned()
        ))
    );
}

#[test]
fn a_batch_with_a_failed_task_an_unfinished_one_needs_deletes_none_of_it() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger = open_ledger(&dir.path().join("consensflow.db"), options()).expect("ledger");
    let view = project(&mut ledger);
    let member = zeus(&view);
    open(&mut ledger, "Stage one", &[]);
    open(&mut ledger, "Parser", &[]);
    open(&mut ledger, "Docs", &[1, 2]);
    done(&mut ledger, 1, member, "One");
    ledger.accept_task(1, 1, "chief").expect("T-1 accepted");
    ledger
        .fail_task(1, 2, "its launch never came up")
        .expect("T-2 fails");

    // T-1 alone may go, but all of them go, or none.
    assert_eq!(
        refused(&mut ledger, &[1, 2]).map(|(code, _)| code),
        Some("task-needed")
    );
    assert_eq!(
        on_board(&ledger.board(1).expect("a board")),
        [1, 2, 3],
        "T-1 stayed with the rest"
    );
    ledger.delete_tasks(1, &[1]).expect("T-1 alone goes");
    assert_eq!(on_board(&ledger.board(1).expect("a board")), [2, 3]);
}

#[test]
fn a_task_not_yet_finished_is_still_not_deleted_whatever_waits_for_it() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger = open_ledger(&dir.path().join("consensflow.db"), options()).expect("ledger");
    let view = project(&mut ledger);
    let member = zeus(&view);
    open(&mut ledger, "Parser", &[]);
    open(&mut ledger, "Docs", &[1]);
    done(&mut ledger, 1, member, "One");
    assert_eq!(
        refused(&mut ledger, &[1]),
        Some((
            "not-finished",
            "T-1 is done: only a finished task leaves the board; the chief accepts it or sends it back first"
                .to_owned()
        ))
    );
}

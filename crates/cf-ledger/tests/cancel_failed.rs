//! A failed task is called off like any other (T-675, 3.0.7; the field report of
//! 2026-10-26): `cf task cancel T-70` answered "cannot cancel T-70: it is failed", and
//! a task that had failed twice on a stuck window stayed failed on the board for good,
//! its work added again as a new task with the whole context restated. A failed task
//! is cancelled as an open, queued, working or paused one is: its window, if one is
//! left, is stopped and its requester told; what waits for it still refuses the
//! cancel, in words that say how to go on (reopen it, since it did not finish). And a
//! task already in a window, which `--before` cannot make wait for a new one, is
//! refused in words that say what to do instead.

#![allow(clippy::expect_used)]

use cf_ledger::{
    open_ledger, Ledger, NewChief, NewMember, NewProject, NewTask, Options, ProjectView,
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
fn project(ledger: &mut Ledger) -> ProjectView {
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

/// The daemon gives the open task `number` to a new session of zeus, whose window takes
/// the brief in: the task is working.
fn in_its_window(ledger: &mut Ledger, number: i64) {
    let zeus = project_member(ledger);
    let brief = ledger
        .assign_task(1, number, zeus)
        .expect("assigned")
        .message
        .expect("its brief")
        .id;
    ledger.begin_delivery(brief).expect("its delivery begins");
    ledger
        .confirm_delivery(brief, Some(&json!({ "item": "i-1" })))
        .expect("its delivery is confirmed");
}

/// The id of zeus, the member.
fn project_member(ledger: &Ledger) -> i64 {
    ledger
        .project(1)
        .expect("a project")
        .expect("project 1")
        .participants
        .into_iter()
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

/// What cancelling `number` as `by` came to: the refusal's code and words, or none.
fn refused(ledger: &mut Ledger, number: i64, by: &str) -> Option<(&'static str, String)> {
    ledger
        .cancel_task(1, number, by)
        .err()
        .map(|error| (error.code().expect("a refusal"), error.to_string()))
}

/// The bodies of the notes in T-`number`'s thread, oldest first.
fn notes(ledger: &Ledger, number: i64) -> Vec<String> {
    ledger
        .task(1, number)
        .expect("a thread")
        .expect("a task")
        .messages
        .into_iter()
        .filter(|message| message.kind == "note")
        .map(|message| message.body)
        .collect()
}

#[test]
fn a_failed_task_is_cancelled_as_an_open_one_is() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger = open_ledger(&dir.path().join("consensflow.db"), options()).expect("ledger");
    project(&mut ledger);
    open(&mut ledger, "Parser", &[]);
    ledger
        .fail_task(1, 1, "its window never came up")
        .expect("T-1 fails");
    assert_eq!(state(&ledger, 1), "failed");

    assert_eq!(refused(&mut ledger, 1, "chief"), None);
    assert_eq!(
        state(&ledger, 1),
        "cancelled",
        "it is over, and leaves the board's open work"
    );
    // The same task is not cancelled twice.
    assert_eq!(
        refused(&mut ledger, 1, "chief"),
        Some((
            "invalid-transition",
            "cannot cancel T-1: it is cancelled".to_owned()
        ))
    );
}

#[test]
fn a_failed_task_whose_window_is_still_up_is_cancelled_and_its_requester_hears_it_was_stopped() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger = open_ledger(&dir.path().join("consensflow.db"), options()).expect("ledger");
    project(&mut ledger);
    open(&mut ledger, "Parser", &[]);
    in_its_window(&mut ledger, 1);
    assert_eq!(state(&ledger, 1), "working");
    ledger
        .fail_task(1, 1, "its window stuck")
        .expect("T-1 fails");

    // The human cancels what the chief asked for: the chief is told, and that the window,
    // which began on the task and is still on the board, is stopped.
    assert_eq!(refused(&mut ledger, 1, "human"), None);
    assert_eq!(state(&ledger, 1), "cancelled");
    assert_eq!(
        notes(&ledger, 1).last().map(String::as_str),
        Some("@human cancelled T-1 (Parser): @zeus-amber-pine's window was stopped.")
    );
}

#[test]
fn what_waits_for_a_failed_task_still_refuses_its_cancel_and_is_told_to_reopen_it() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger = open_ledger(&dir.path().join("consensflow.db"), options()).expect("ledger");
    project(&mut ledger);
    open(&mut ledger, "Parser", &[]);
    open(&mut ledger, "Docs", &[1]);
    ledger
        .fail_task(1, 1, "its window died")
        .expect("T-1 fails");

    assert_eq!(
        refused(&mut ledger, 1, "chief"),
        Some((
            "waited-for",
            "T-1 cannot be cancelled while T-2 waits for it: cancel T-2 first, reopen T-1 and let it finish, or accept it with what you checked"
                .to_owned()
        ))
    );
    assert_eq!(state(&ledger, 1), "failed", "the refusal changed nothing");
    assert_eq!(refused(&mut ledger, 2, "chief"), None);
    assert_eq!(
        refused(&mut ledger, 1, "chief"),
        None,
        "nothing waits for it now"
    );
    assert_eq!(state(&ledger, 1), "cancelled");
}

#[test]
fn a_task_in_a_window_cannot_wait_for_a_new_one_and_the_refusal_says_what_to_do_instead() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger = open_ledger(&dir.path().join("consensflow.db"), options()).expect("ledger");
    project(&mut ledger);
    open(&mut ledger, "Review", &[]);

    let before = |ledger: &mut Ledger| {
        ledger
            .create_task(
                1,
                &NewTask {
                    from: "chief".into(),
                    pool: Some("worker".into()),
                    tier: Some("standard".into()),
                    body: "Fix first".into(),
                    before: vec![1],
                    ..NewTask::default()
                },
            )
            .expect_err("T-1 is in a window")
    };
    // Queued for its window, the brief not begun: it can be put behind the new task with an edit.
    let brief = ledger
        .assign_task(1, 1, project_member(&ledger))
        .expect("assigned")
        .message
        .expect("its brief")
        .id;
    assert_eq!(state(&ledger, 1), "queued");
    let queued = before(&mut ledger);
    assert_eq!(queued.code(), Some("not-on-the-board"));
    assert_eq!(
        queued.to_string(),
        "T-1 is queued: only a task still on the board can wait for a new one; a task in a window is not pulled back. Add the new task without --before, then put T-1 behind it with cf task edit T-1 --needs +T-<the new task>; or cancel it and add it again with --needs on the new task"
    );
    ledger.begin_delivery(brief).expect("its delivery begins");
    ledger
        .confirm_delivery(brief, Some(&json!({ "item": "i-1" })))
        .expect("its delivery is confirmed");
    let working = before(&mut ledger);
    assert_eq!(working.code(), Some("not-on-the-board"));
    assert_eq!(
        working.to_string(),
        "T-1 is working: only a task still on the board can wait for a new one; a task in a window is not pulled back. Add the new task without --before, pause T-1 now (cf task pause T-1), and send it on with cf task resume T-1 \"…\" once the new task is accepted; or cancel it and add it again with --needs on the new task"
    );
    assert_eq!(state(&ledger, 1), "working", "the refusal changed nothing");

    ledger
        .pause_task(1, 1, Some("chief"), None)
        .expect("T-1 is paused");
    assert_eq!(
        before(&mut ledger).to_string(),
        "T-1 is paused: only a task still on the board can wait for a new one; a task in a window is not pulled back. Add the new task without --before, and send T-1 on with cf task resume T-1 \"…\" once the new task is accepted; or cancel it and add it again with --needs on the new task"
    );

    // A task that is over has no window to wait for anything.
    ledger.cancel_task(1, 1, "chief").expect("T-1 is cancelled");
    assert_eq!(
        before(&mut ledger).to_string(),
        "T-1 is cancelled: only a task still on the board can wait for a new one; add that work again with --needs on the new task if it is still wanted"
    );
}

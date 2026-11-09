//! The advice a `--before` refusal gives (T-675, 3.0.7, and the review T-1212 of T-1208,
//! finding 4): a task already in a window cannot be made to wait for a new one, and the
//! refusal says what to do instead. The advice is chosen by who the task is given to and
//! whether its brief can still be edited, not by its state alone, and each piece of it is
//! run here to show it works: an edit of `--needs` for a task given to a tier whose brief
//! has not begun to reach its window; a pause and a resume for a worker's task, a follow-up
//! included; finishing it later for the chief's own step, which cannot be paused and, being
//! given by name, takes no `--needs`; and adding the work again, which every task can be.

#![allow(clippy::expect_used)]

use cf_ledger::{
    open_ledger, Ledger, LedgerError, Links, NewChief, NewMember, NewProject, NewTask, Options,
    TaskEdit,
};
use serde_json::json;

fn ledger_in(dir: &tempfile::TempDir) -> Ledger {
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
                model_name: None,
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
            staff_rules: None,
            compaction: None,
        })
        .expect("a project");
    ledger
}

fn zeus(ledger: &Ledger) -> i64 {
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

fn task(body: &str) -> NewTask {
    NewTask {
        from: "chief".into(),
        pool: Some("worker".into()),
        tier: Some("standard".into()),
        body: body.into(),
        ..NewTask::default()
    }
}

/// A task for a standard worker, given to a new session of zeus: its brief is queued for a
/// window, not begun.
fn queued(ledger: &mut Ledger, body: &str) -> i64 {
    let number = ledger
        .create_task(1, &task(body))
        .expect("a task")
        .task
        .number;
    let member = zeus(ledger);
    ledger.assign_task(1, number, member).expect("assigned");
    number
}

/// The same, with its brief received: the task is working.
fn working(ledger: &mut Ledger, body: &str) -> i64 {
    let number = ledger
        .create_task(1, &task(body))
        .expect("a task")
        .task
        .number;
    let member = zeus(ledger);
    let brief = ledger
        .assign_task(1, number, member)
        .expect("assigned")
        .message
        .expect("its brief")
        .id;
    ledger.begin_delivery(brief).expect("its delivery begins");
    ledger
        .confirm_delivery(brief, Some(&json!({ "item": format!("i-{number}") })))
        .expect("its delivery is confirmed");
    number
}

/// What adding a task `before` T-`number` came to: the refusal.
fn refused_before(ledger: &mut Ledger, number: u64) -> LedgerError {
    let mut new = task("Fix first");
    new.before = vec![number];
    ledger
        .create_task(1, &new)
        .expect_err("T-{number} is in a window")
}

fn state(ledger: &Ledger, number: i64) -> String {
    ledger
        .task(1, number)
        .expect("a thread")
        .expect("a task")
        .task
        .state
}

/// The task is done by its worker and accepted by the chief.
fn accepted(ledger: &mut Ledger, number: i64) {
    ledger.record_result(1, number, "Done").expect("its result");
    ledger.accept_task(1, number, "chief").expect("accepted");
}

#[test]
fn a_task_for_a_tier_queued_for_its_window_is_put_behind_the_new_task_by_an_edit() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger = ledger_in(&dir);
    let review = queued(&mut ledger, "Review");

    let refusal = refused_before(&mut ledger, review as u64);
    assert!(
        refusal
            .to_string()
            .contains("cf task edit T-1 --needs +T-<the new task>"),
        "{refusal}"
    );

    // The advice: add the new task without --before, then put the review behind it.
    let fix = ledger
        .create_task(1, &task("Fix first"))
        .expect("the new task")
        .task
        .number;
    ledger
        .edit_task_with(
            1,
            review,
            "chief",
            &TaskEdit {
                needs: Some(Links::Change {
                    add: vec![fix],
                    remove: Vec::new(),
                }),
                ..TaskEdit::default()
            },
        )
        .expect("the edit the refusal named works");
    let waiting = ledger
        .task(1, review)
        .expect("a thread")
        .expect("a task")
        .task;
    assert_eq!(
        (waiting.state.as_str(), waiting.blocked_by),
        ("open", vec![fix])
    );
}

#[test]
fn a_task_for_a_tier_whose_brief_is_being_pasted_is_told_to_pause_it_not_to_edit_it() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger = ledger_in(&dir);
    // Still queued, as the task is until its window has the brief, but the paste has begun.
    let review = ledger
        .create_task(1, &task("Review"))
        .expect("a task")
        .task
        .number;
    let member = zeus(&ledger);
    let brief = ledger
        .assign_task(1, review, member)
        .expect("assigned")
        .message
        .expect("its brief")
        .id;
    ledger.begin_delivery(brief).expect("the paste begins");
    assert_eq!(state(&ledger, review), "queued");

    let refusal = refused_before(&mut ledger, review as u64).to_string();
    assert!(
        refusal.contains("pause T-1 now (cf task pause T-1)") && !refusal.contains("cf task edit"),
        "{refusal}"
    );
    // The edit would be refused (the window has the brief), and the pause is not.
    let fix = ledger
        .create_task(1, &task("Fix first"))
        .expect("the new task")
        .task
        .number;
    let edit = ledger
        .edit_task_with(
            1,
            review,
            "chief",
            &TaskEdit {
                needs: Some(Links::Change {
                    add: vec![fix],
                    remove: Vec::new(),
                }),
                ..TaskEdit::default()
            },
        )
        .expect_err("a brief being pasted is not edited");
    assert_eq!(edit.code(), Some("already-given"));
    ledger
        .pause_task(1, review, Some("chief"), None)
        .expect("the pause the refusal named works");
    assert_eq!(state(&ledger, review), "paused");
}

#[test]
fn a_task_for_a_tier_at_work_is_paused_and_sent_on_when_the_new_task_is_accepted() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger = ledger_in(&dir);
    let review = working(&mut ledger, "Review");

    let refusal = refused_before(&mut ledger, review as u64).to_string();
    assert!(
        refusal.contains("pause T-1 now (cf task pause T-1)")
            && refusal.contains("cf task resume T-1")
            && !refusal.contains("cf task edit"),
        "{refusal}"
    );

    // The advice: add the new task, pause the review, resume it once the new one is accepted.
    let fix = working(&mut ledger, "Fix first");
    ledger
        .pause_task(1, review, Some("chief"), None)
        .expect("the pause the refusal named works");
    assert_eq!(state(&ledger, review), "paused");
    accepted(&mut ledger, fix);
    ledger
        .resume_task(1, review, Some("chief"), "The fix is in: go on.")
        .expect("the resume the refusal named works");
    assert_eq!(state(&ledger, review), "queued", "back in its window");
}

#[test]
fn a_paused_task_is_resumed_when_the_new_task_is_accepted() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger = ledger_in(&dir);
    let review = working(&mut ledger, "Review");
    ledger
        .pause_task(1, review, Some("chief"), None)
        .expect("paused");

    let refusal = refused_before(&mut ledger, review as u64).to_string();
    assert!(
        refusal.contains("send T-1 on with cf task resume T-1")
            && !refusal.contains("pause T-1 now"),
        "{refusal}"
    );
    ledger
        .resume_task(1, review, Some("chief"), "The fix is in: go on.")
        .expect("the resume the refusal named works");
    assert_eq!(state(&ledger, review), "queued");
}

#[test]
fn a_follow_up_is_not_told_to_edit_its_needs_which_it_cannot_but_to_pause_and_resume_it() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger = ledger_in(&dir);
    // T-1 is done and accepted, and its follow-up T-2 goes to the session that did it: queued.
    let first = working(&mut ledger, "Parser");
    accepted(&mut ledger, first);
    let follow_up = ledger
        .create_task(
            1,
            &NewTask {
                from: "chief".into(),
                after: Some(first),
                body: "The lexer".into(),
                ..NewTask::default()
            },
        )
        .expect("a follow-up")
        .task
        .number;
    assert_eq!(state(&ledger, follow_up), "queued");

    let refusal = refused_before(&mut ledger, follow_up as u64).to_string();
    assert!(
        refusal.contains("pause T-2 now (cf task pause T-2)")
            && refusal.contains("cf task resume T-2")
            && !refusal.contains("cf task edit"),
        "{refusal}"
    );
    // What the earlier advice named does not work for it ...
    let fix = ledger
        .create_task(1, &task("Fix first"))
        .expect("the new task")
        .task
        .number;
    let edit = ledger
        .edit_task_with(
            1,
            follow_up,
            "chief",
            &TaskEdit {
                needs: Some(Links::Change {
                    add: vec![fix],
                    remove: Vec::new(),
                }),
                ..TaskEdit::default()
            },
        )
        .expect_err("a follow-up has no pool to wait on the board in");
    assert_eq!(edit.code(), Some("invalid-transition"));
    // ... and what it names does.
    ledger
        .pause_task(1, follow_up, Some("chief"), None)
        .expect("the pause the refusal named works");
    ledger
        .resume_task(1, follow_up, Some("chief"), "The fix is in: go on.")
        .expect("the resume the refusal named works");
    assert_eq!(state(&ledger, follow_up), "queued");
}

#[test]
fn a_task_given_by_name_is_not_told_to_edit_its_needs_which_it_cannot_but_to_pause_and_resume_it() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger = ledger_in(&dir);
    // A task for zeus by name, with no tier to wait on the board in: queued for his window.
    let by_name = ledger
        .create_task(
            1,
            &NewTask {
                from: "chief".into(),
                to: Some("zeus".into()),
                body: "Review".into(),
                ..NewTask::default()
            },
        )
        .expect("a task by name")
        .task
        .number;
    assert_eq!(state(&ledger, by_name), "queued");

    let refusal = refused_before(&mut ledger, by_name as u64).to_string();
    assert!(
        refusal.contains("pause T-1 now (cf task pause T-1)")
            && refusal.contains("cf task resume T-1")
            && !refusal.contains("cf task edit"),
        "{refusal}"
    );
    // The edit the advice for a tier names is refused for it, in the ledger's words ...
    let fix = ledger
        .create_task(1, &task("Fix first"))
        .expect("the new task")
        .task
        .number;
    let edit = ledger
        .edit_task_with(
            1,
            by_name,
            "chief",
            &TaskEdit {
                needs: Some(Links::Change {
                    add: vec![fix],
                    remove: Vec::new(),
                }),
                ..TaskEdit::default()
            },
        )
        .expect_err("a task given by name has no pool to wait on the board in");
    assert_eq!(edit.code(), Some("invalid-transition"));
    assert!(edit.to_string().contains("given by name"), "{edit}");
    // ... and what the refusal names works.
    ledger
        .pause_task(1, by_name, Some("chief"), None)
        .expect("the pause the refusal named works");
    ledger
        .resume_task(1, by_name, Some("chief"), "The fix is in: go on.")
        .expect("the resume the refusal named works");
    assert_eq!(state(&ledger, by_name), "queued");
}

#[test]
fn the_chiefs_own_step_is_told_to_finish_it_later_or_add_it_again_and_both_work() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger = ledger_in(&dir);
    // A stage for a worker, and the chief's step behind it, whose last need is then taken:
    // its brief goes back to the chief, queued.
    let stage = ledger
        .create_task(1, &task("Stage"))
        .expect("the stage")
        .task
        .number;
    let step = ledger
        .create_task(
            1,
            &NewTask {
                from: "chief".into(),
                to: Some("chief".into()),
                body: "Release notes".into(),
                needs: vec![stage as u64],
                ..NewTask::default()
            },
        )
        .expect("an own step")
        .task
        .number;
    ledger
        .edit_task_with(
            1,
            step,
            "chief",
            &TaskEdit {
                needs: Some(Links::Change {
                    add: Vec::new(),
                    remove: vec![stage],
                }),
                ..TaskEdit::default()
            },
        )
        .expect("the need is taken");
    assert_eq!(state(&ledger, step), "queued");

    let refusal = refused_before(&mut ledger, step as u64).to_string();
    assert!(
        refusal.contains("T-2 is your own step")
            && refusal.contains("finish it later with cf task done T-2")
            && refusal.contains("add it again with --self --needs on the new task")
            && !refusal.contains("cf task pause")
            && !refusal.contains("cf task edit"),
        "{refusal}"
    );
    // What a worker's task is told does not work for it ...
    let pause = ledger
        .pause_task(1, step, Some("chief"), None)
        .expect_err("the chief's own step is not paused");
    assert_eq!(pause.code(), Some("own-work"));
    // ... and finishing it later does.
    ledger
        .record_result(1, step, "Written")
        .expect("the step is finished by the chief's word");
    assert_eq!(state(&ledger, step), "accepted");
}

#[test]
fn a_task_cancelled_and_added_again_waits_for_the_new_task_as_every_refusal_says() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger = ledger_in(&dir);
    let review = working(&mut ledger, "Review");
    let refusal = refused_before(&mut ledger, review as u64).to_string();
    assert!(
        refusal.contains("cancel it and add it again with --needs on the new task"),
        "{refusal}"
    );

    let fix = ledger
        .create_task(1, &task("Fix first"))
        .expect("the new task")
        .task
        .number;
    ledger.cancel_task(1, review, "chief").expect("cancelled");
    let again = ledger
        .create_task(
            1,
            &NewTask {
                needs: vec![fix as u64],
                ..task("Review")
            },
        )
        .expect("added again with --needs")
        .task;
    assert_eq!(again.blocked_by, [fix]);
}

//! A follow-up held for the conversation it cannot go on in (board task T-957, 3.0.7, from the
//! field issue of 2026-10-29): `--after T-65` was accepted as "continues in the window that did
//! T-65", and then went to a fresh window because the window that did T-65 had closed with no
//! proof its program was gone, though its brief counted on that window ("your T-65 snapshot").
//! The engine now holds such a task, paused, with the mark that it was held for this; the chief
//! rewrites its brief and sends it on, and it is not held a second time.

#![allow(clippy::expect_used)]

use cf_ledger::{open_ledger, Ledger, NewChief, NewMember, NewProject, NewTask, Options};

fn ledger_with_two_queued_tasks(dir: &tempfile::TempDir) -> Ledger {
    let mut ledger =
        open_ledger(&dir.path().join("consensflow.db"), Options::default()).expect("ledger");
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
    for body in ["Snapshot", "Compare it with the snapshot"] {
        ledger
            .create_task(
                1,
                &NewTask {
                    from: "chief".into(),
                    to: Some("zeus".into()),
                    body: body.into(),
                    ..NewTask::default()
                },
            )
            .expect("a task given by name");
    }
    ledger
}

fn state(ledger: &Ledger, number: i64) -> String {
    ledger
        .task(1, number)
        .expect("a thread")
        .expect("a task")
        .task
        .state
}

#[test]
fn a_queued_task_is_held_for_the_conversation_it_cannot_go_on_in_and_the_chiefs_word_sends_it_on() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger = ledger_with_two_queued_tasks(&dir);
    assert_eq!(state(&ledger, 2), "queued");
    assert!(!ledger.held_for_context(1, 2).expect("a read"));

    let held = ledger
        .hold_for_context(
            1,
            2,
            "@zeus's window that worked on T-1 closed with no proof its program was gone",
        )
        .expect("held");
    assert_eq!(held.state, "paused");
    assert!(ledger.held_for_context(1, 2).expect("a read"));
    assert!(
        !ledger.held_for_context(1, 1).expect("a read"),
        "T-1 was not"
    );

    // The chief rewrites the brief, which a paused task takes, and sends it on.
    ledger
        .edit_task(1, 2, "chief", "Compare it with what the snapshot said")
        .expect("a paused task is edited");
    ledger
        .resume_task(1, 2, Some("chief"), "Go on in a window of your own.")
        .expect("resumed");
    assert_eq!(state(&ledger, 2), "queued");
    assert!(
        ledger.held_for_context(1, 2).expect("a read"),
        "it was held once, and the chief's word after it is the chief's knowing"
    );
}

#[test]
fn only_a_queued_task_is_held_and_an_ordinary_pause_is_no_such_hold() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger = ledger_with_two_queued_tasks(&dir);
    ledger
        .pause_task(1, 1, Some("chief"), None)
        .expect("T-1 is paused");
    assert!(
        !ledger.held_for_context(1, 1).expect("a read"),
        "the chief's own pause is not a hold for a conversation"
    );
    let refused = ledger
        .hold_for_context(1, 1, "a window cannot go on")
        .expect_err("T-1 is paused already");
    assert_eq!(refused.code(), Some("invalid-transition"));
    assert_eq!(refused.to_string(), "cannot hold T-1: it is paused");
}

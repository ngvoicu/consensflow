//! A follow-up held for the conversation it cannot go on in (board task T-957, 3.0.7, from the
//! field issue of 2026-10-29): `--after T-65` was accepted as "continues in the window that did
//! T-65", and then went to a fresh window because the window that did T-65 had closed with no
//! proof its program was gone, though its brief counted on that window ("your T-65 snapshot").
//! The engine now holds such a task, paused, with the mark that it was held for this; the chief
//! rewrites its brief and sends it on, and it is not held a second time.

#![allow(clippy::expect_used)]

use cf_ledger::{
    open_ledger, Ledger, MessageView, NewChief, NewMember, NewProject, NewTask, Options,
    NOT_PROVEN_GONE,
};

fn ledger_with_two_queued_tasks(dir: &tempfile::TempDir) -> Ledger {
    let mut ledger =
        open_ledger(&dir.path().join("consensflow.db"), Options::default()).expect("ledger");
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

/// What the window before closed with: the reason a hold says, and the warning it carries.
fn unproven() -> String {
    format!(
        "@zeus's window that worked on T-1 {NOT_PROVEN_GONE}, which may still write to T-1's conversation"
    )
}

/// The conversation is gone: the reason a hold says, and no warning.
const MISSING: &str = "@zeus's harness holds nothing of the conversation of T-1 any more";

/// The chief's inbox, oldest first.
fn chiefs_inbox(ledger: &Ledger) -> Vec<MessageView> {
    let chief = ledger
        .project(1)
        .expect("a project")
        .expect("project 1")
        .participants
        .into_iter()
        .find(|participant| participant.handle == "chief")
        .expect("the chief")
        .id;
    let mut inbox = ledger.inbox(chief, 100).expect("the chief's inbox");
    inbox.sort_by_key(|message| message.id);
    inbox
}

/// The chief's notes (ConsensFlow's own) about T-`number`, oldest first.
fn notes_about(ledger: &Ledger, number: i64) -> Vec<MessageView> {
    chiefs_inbox(ledger)
        .into_iter()
        .filter(|message| {
            message.kind == "note"
                && message.sender.is_none()
                && message.task_number == Some(number)
        })
        .collect()
}

/// What the chief is told again of a task that went on, still queued: the warnings that
/// outlive a pause, each in a note of its own.
fn said_again(ledger: &Ledger) -> Vec<String> {
    chiefs_inbox(ledger)
        .into_iter()
        .filter(|message| message.state == "queued" && message.body.contains(" Before that, "))
        .map(|message| message.body)
        .collect()
}

fn message(ledger: &Ledger, id: i64) -> MessageView {
    ledger.message(id).expect("a read").expect("the message")
}

#[test]
fn the_hold_is_told_in_a_pause_note_that_says_why_and_the_way_on() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger = ledger_with_two_queued_tasks(&dir);
    ledger.hold_for_context(1, 2, &unproven()).expect("held");

    let notes = notes_about(&ledger, 2);
    assert_eq!(notes.len(), 1, "told once: {notes:?}");
    assert_eq!(notes[0].state, "queued");
    assert_eq!(
        notes[0].body,
        format!(
            "T-2 is paused: {}. It does not go on in that conversation, and a window of its own would not remember it: if its brief counts on that, rewrite it with cf task edit T-2 \"…\" first, then send it to a window of its own with: cf task resume T-2 \"…\", or cancel it.",
            unproven()
        )
    );
}

#[test]
fn a_hold_note_still_queued_goes_at_the_resume_and_the_warning_stays_in_a_note_of_its_own() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger = ledger_with_two_queued_tasks(&dir);
    ledger.hold_for_context(1, 2, &unproven()).expect("held");
    let held = notes_about(&ledger, 2)[0].id;

    ledger
        .resume_task(1, 2, Some("chief"), "Go on in a window of your own.")
        .expect("resumed");

    assert_eq!(
        message(&ledger, held).state,
        "cancelled",
        "the instruction is stale"
    );
    assert_eq!(
        said_again(&ledger),
        [format!("T-2 resumed. Before that, {}.", unproven())],
        "only what is still true: the window before may still write"
    );
    assert!(
        notes_about(&ledger, 2)
            .iter()
            .all(|note| note.state != "queued"),
        "no instruction to resume is left queued"
    );
}

#[test]
fn a_hold_note_on_its_way_when_the_task_is_resumed_is_not_sent_again_after_a_refusal() {
    // The field of the review of T-1208 (finding 3): the note began its delivery to the
    // chief, the human resumed the task, and the chief's paste was refused. The retry queued
    // "T-2 is paused …" again for a task that was paused no more.
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger = ledger_with_two_queued_tasks(&dir);
    ledger.hold_for_context(1, 2, &unproven()).expect("held");
    let held = notes_about(&ledger, 2)[0].id;
    ledger.begin_delivery(held).expect("on its way");
    ledger
        .resume_task(1, 2, Some("human"), "Go on in a window of your own.")
        .expect("resumed");
    assert_eq!(
        message(&ledger, held).state,
        "delivering",
        "a note on its way runs its course"
    );

    ledger
        .retry_delivery(held, "the chief's paste was refused", true)
        .expect("the retry");

    assert_eq!(
        message(&ledger, held).state,
        "cancelled",
        "the instruction to resume a task that was resumed is not sent again"
    );
    assert_eq!(
        said_again(&ledger),
        [format!("T-2 resumed. Before that, {}.", unproven())],
        "the warning that the window before may still write is said, apart"
    );
}

#[test]
fn a_hold_note_given_up_after_the_task_was_resumed_is_withdrawn_and_not_failed_and_its_warning_stays(
) {
    // The other way a delivery ends: the chief's window is gone, and the note fails for good
    // (`fail_delivery`). A note for a task that moved on while it was on its way is no failure:
    // it is withdrawn, and the warning that the window before may still write is said apart.
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger = ledger_with_two_queued_tasks(&dir);
    ledger.hold_for_context(1, 2, &unproven()).expect("held");
    let held = notes_about(&ledger, 2)[0].id;
    ledger.begin_delivery(held).expect("on its way");
    ledger
        .resume_task(1, 2, Some("human"), "Go on in a window of your own.")
        .expect("resumed");

    ledger
        .fail_delivery(held, "the chief's window is gone")
        .expect("given up");

    assert_eq!(
        message(&ledger, held).state,
        "cancelled",
        "withdrawn, not failed"
    );
    assert_eq!(
        said_again(&ledger),
        [format!("T-2 resumed. Before that, {}.", unproven())]
    );
}

#[test]
fn a_hold_for_a_conversation_the_harness_lost_leaves_no_warning_when_its_note_goes() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger = ledger_with_two_queued_tasks(&dir);
    ledger.hold_for_context(1, 2, MISSING).expect("held");
    let held = notes_about(&ledger, 2)[0].id;
    ledger.begin_delivery(held).expect("on its way");
    ledger
        .resume_task(1, 2, Some("human"), "Go on in a window of your own.")
        .expect("resumed");

    ledger
        .retry_delivery(held, "the chief's paste was refused", true)
        .expect("the retry");

    assert_eq!(message(&ledger, held).state, "cancelled");
    assert_eq!(
        said_again(&ledger),
        Vec::<String>::new(),
        "a lost conversation is nothing that outlives the pause"
    );
    assert!(notes_about(&ledger, 2)
        .iter()
        .all(|note| note.state != "queued"));
}

#[test]
fn a_hold_note_on_its_way_to_a_task_still_held_goes_back_to_its_queue() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger = ledger_with_two_queued_tasks(&dir);
    ledger.hold_for_context(1, 2, &unproven()).expect("held");
    let held = notes_about(&ledger, 2)[0].id;
    ledger.begin_delivery(held).expect("on its way");

    ledger
        .retry_delivery(held, "the chief's paste was refused", true)
        .expect("the retry");

    assert_eq!(
        message(&ledger, held).state,
        "queued",
        "the task is still held: the instruction is true, and is told again"
    );
}

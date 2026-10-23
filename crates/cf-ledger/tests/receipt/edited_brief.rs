//! A brief replaced (`cf task edit`, T-110, 3.0.6; the chief of 2026-10-21): the
//! brief of a task no window was given yet is the chief's to change, and the
//! window that gets the task gets the new words. A task a window has is told,
//! or reopened, never edited. The old brief stays in the log.

use crate::fixture::{gated_world, world, World};

/// The task `number`'s brief and title.
fn brief(w: &World, number: i64) -> (String, String) {
    let task = w
        .ledger
        .task(w.project, number)
        .expect("a read")
        .expect("the task")
        .task;
    (task.body, task.title)
}

/// What the chief's edits of the tasks left in the log: the task, who, the old brief.
fn edits(w: &World) -> Vec<(i64, String, String)> {
    w.ledger
        .events(w.project, 0, 500)
        .expect("the events")
        .into_iter()
        .filter(|event| event.kind == "task.edited")
        .map(|event| {
            let text = |field: &str| event.data[field].as_str().unwrap_or_default().to_owned();
            (
                event.data["task"].as_i64().unwrap_or_default(),
                text("by"),
                text("from"),
            )
        })
        .collect()
}

/// A task done by a session, and a follow-up of it that waits for `later`'s needs.
fn waiting_follow_up(w: &mut World, body: &str) -> (String, i64, i64) {
    let a = w.open("Parser").task.number;
    let moved = w.assign(a, "zeus");
    let session = moved.task.assignee.clone().expect("a session");
    w.deliver(moved.message.expect("its brief").id);
    w.ledger
        .record_result(w.project, a, "Parser, done")
        .expect("a result");
    let need = w.open("Lexer").task.number;
    let follow_up = w.follow_up(a, body, &[need]).expect("a follow-up");
    assert_eq!(follow_up.task.state, "open");
    (session, need, follow_up.task.number)
}

#[test]
fn the_brief_of_a_task_open_for_a_member_is_replaced_and_the_old_one_stays_in_the_log() {
    let mut w = world();
    let task = w.open("Parse the old way").task.number;

    let edited = w
        .ledger
        .edit_task(w.project, task, "chief", "Parse the new way\nand say so")
        .expect("edited");
    assert_eq!(edited.state, "open", "it stays where it is on the board");
    assert_eq!(edited.body, "Parse the new way\nand say so");
    assert_eq!(
        edited.title, "Parse the new way",
        "the title follows the brief"
    );
    assert_eq!(brief(&w, task).0, "Parse the new way\nand say so");
    assert_eq!(
        edits(&w),
        [(task, "chief".to_owned(), "Parse the old way".to_owned())]
    );

    // A second edit keeps the brief it replaced, so the whole chain is there.
    w.ledger
        .edit_task(w.project, task, "chief", "Parse it a third way")
        .expect("edited again");
    assert_eq!(
        edits(&w)
            .iter()
            .map(|(_, _, from)| from.as_str())
            .collect::<Vec<_>>(),
        ["Parse the old way", "Parse the new way\nand say so"]
    );

    // The member that takes it is given the new words.
    let moved = w.assign(task, "zeus");
    assert_eq!(
        moved.message.expect("its brief").body,
        "Parse it a third way"
    );
}

#[test]
fn a_blocked_task_edited_starts_with_the_new_brief_once_what_it_needs_is_accepted() {
    let mut w = world();
    let (session, need, later) = waiting_follow_up(&mut w, "Tests, the old way");

    w.ledger
        .edit_task(w.project, later, "chief", "Tests, the new way")
        .expect("a blocked task is edited");
    assert_eq!(w.state(later), "open", "it still waits for what it needs");
    assert_eq!(w.next(&session), None);

    let moved = w.assign(need, "diana");
    w.deliver(moved.message.expect("its brief").id);
    w.ledger
        .record_result(w.project, need, "Lexer, done")
        .expect("a result");
    w.ledger
        .accept_task(w.project, need, "chief")
        .expect("accepted");
    assert_eq!(w.state(later), "queued");
    let head = w.next(&session).expect("the follow-up for the window");
    assert_eq!(w.message(head).body, "Tests, the new way");
    w.deliver(head);
    assert_eq!(w.state(later), "working");
}

#[test]
fn a_task_queued_for_its_window_and_not_yet_pasted_is_edited_in_the_message_that_carries_it() {
    let mut w = world();
    let created = w.give("zeus", "Parser, old");
    let (task, message) = (created.task.number, created.message.expect("a brief").id);
    assert_eq!(w.state(task), "queued");

    w.ledger
        .edit_task(w.project, task, "chief", "Parser, new")
        .expect("edited before it was pasted");
    assert_eq!(w.state(task), "queued");
    assert_eq!(w.next("zeus"), Some(message), "the same message");
    assert_eq!(w.message(message).body, "Parser, new");
}

#[test]
fn a_task_held_at_the_humans_gate_is_edited_in_the_message_the_human_will_pass_on() {
    let mut w = gated_world();
    let created = w.give("zeus", "Parser, old");
    let (task, message) = (created.task.number, created.message.expect("a brief").id);
    assert_eq!(w.message(message).state, "gated");

    w.ledger
        .edit_task(w.project, task, "chief", "Parser, new")
        .expect("edited at the gate");
    assert_eq!(w.message(message).body, "Parser, new");
    w.ledger
        .approve_message(message, "human")
        .expect("passed on");
    let begun = w.deliver(message);
    assert_eq!(begun.message.body, "Parser, new");
}

#[test]
fn a_task_paused_before_any_window_had_it_is_edited_and_goes_on_with_the_new_brief() {
    let mut w = world();
    let task = w.open("Parse the old way").task.number;
    w.pause(task);
    assert_eq!(w.state(task), "paused");

    w.ledger
        .edit_task(w.project, task, "chief", "Parse the new way")
        .expect("a task no window had is edited, paused or not");
    assert_eq!(w.state(task), "paused");
    w.resume(task, "Go on");
    assert_eq!(w.state(task), "open", "back on the board for a member");
    let moved = w.assign(task, "zeus");
    let brief = moved.message.expect("its brief").body;
    assert!(brief.starts_with("Parse the new way"), "{brief}");
    assert!(!brief.contains("old way"), "{brief}");
}

/// What `edit_task` of `number` came to: the refusal's code and words.
fn refused(w: &mut World, number: i64) -> (&'static str, String) {
    let refusal = w
        .ledger
        .edit_task(w.project, number, "chief", "Something else")
        .expect_err("refused");
    (refusal.code().expect("a refusal"), refusal.to_string())
}

#[test]
fn a_task_a_window_was_given_is_not_edited_and_the_refusal_says_what_to_do_instead() {
    let mut w = world();
    let task = w.open("Parser").task.number;
    let moved = w.assign(task, "zeus");
    let session = moved.task.assignee.clone().expect("a session");
    let message = moved.message.expect("its brief").id;

    // Its brief is being pasted: the window has it, or is about to. What it is told
    // now is kept for it and goes in after the brief (B3, merged with `edit` on
    // T-340): the refusal says to tell it, and the tell is taken.
    w.begin(message);
    assert_eq!(
        refused(&mut w, task),
        (
            "already-given",
            format!("cannot edit T-{task}: it was given to @{session}, which has the brief; tell its window: cf tell T-{task} \"…\"")
        )
    );
    assert_eq!(w.state(task), "queued");
    w.ledger
        .tell_queued(w.project, task, "chief", "Parse it the new way")
        .expect("the words are kept for its window");
    w.confirm(message);
    w.deliver(w.next(&session).expect("the words told it"));
    assert_eq!(w.state(task), "working");
    let told = format!(
        "cannot edit T-{task}: it was given to @{session}, which has the brief; tell its window: cf tell T-{task} \"…\""
    );
    assert_eq!(refused(&mut w, task), ("already-given", told.clone()));

    // Paused and waiting for an answer: its window still has it.
    w.pause(task);
    assert_eq!(refused(&mut w, task), ("already-given", told));

    // With a result, it is reopened.
    w.resume(task, "Go on");
    w.deliver(w.next(&session).expect("the words"));
    w.ledger
        .record_result(w.project, task, "Parser, done")
        .expect("a result");
    assert_eq!(
        refused(&mut w, task),
        (
            "already-given",
            format!("cannot edit T-{task}: it was given to @{session}, which has the brief; reopen it with your words: cf task reopen T-{task} \"…\"")
        )
    );
    assert_eq!(
        brief(&w, task).0,
        "Parser",
        "nothing of the refused edits is left"
    );
    assert!(edits(&w).is_empty());
}

#[test]
fn a_task_that_is_over_is_not_edited() {
    let mut w = world();
    let cancelled = w.open("Docs").task.number;
    w.ledger
        .cancel_task(w.project, cancelled, "chief")
        .expect("cancelled");
    assert_eq!(
        refused(&mut w, cancelled),
        (
            "invalid-transition",
            format!("cannot edit T-{cancelled}: it is cancelled")
        )
    );
}

#[test]
fn a_task_taken_back_from_a_window_that_had_its_brief_is_not_edited() {
    let mut w = world();
    let task = w.open("Parser").task.number;
    let moved = w.assign(task, "zeus");
    w.deliver(moved.message.expect("its brief").id);
    w.ledger
        .release_task(w.project, task, "out of quota")
        .expect("taken back");
    assert_eq!(w.state(task), "open");
    let (code, said) = refused(&mut w, task);
    assert_eq!(code, "already-given");
    assert!(
        said.starts_with(&format!("cannot edit T-{task}: it was given to a window")),
        "{said}"
    );
}

#[test]
fn an_empty_brief_is_refused_and_a_task_that_is_not_there_is_not_edited() {
    let mut w = world();
    let task = w.open("Parser").task.number;
    let blank = w
        .ledger
        .edit_task(w.project, task, "chief", "  \n ")
        .expect_err("no brief");
    assert_eq!(blank.code(), Some("invalid-text"));
    assert_eq!(brief(&w, task).0, "Parser");
    let missing = w
        .ledger
        .edit_task(w.project, 99, "chief", "Something")
        .expect_err("no such task");
    assert_eq!(missing.code(), Some("unknown-task"));
}

/// What the window is pasted for `carrier`: its own words, and the rows it carries.
fn pasted(w: &mut World, carrier: i64) -> String {
    let begun = w.deliver(carrier);
    std::iter::once(begun.message.body)
        .chain(begun.carried.into_iter().map(|row| row.body))
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[test]
fn a_task_held_for_quota_and_resumed_by_the_daemon_is_edited_in_its_brief_alone() {
    let mut w = world();
    let created = w.give("zeus", "Parser, old");
    let task = created.task.number;
    // The member runs out of quota before the brief is pasted; the daemon resumes it.
    w.hold(task);
    let carrier = w.daemon_resumes(task).message.expect("its words").id;

    w.ledger
        .edit_task(w.project, task, "chief", "Parser, new")
        .expect("no window has the brief yet");
    let said = pasted(&mut w, carrier);
    assert_eq!(
        said.matches("Parser, new").count(),
        1,
        "the brief once: {said}"
    );
    assert_eq!(
        said.matches("Go on where you stopped.").count(),
        1,
        "and the words that resume it: {said}"
    );
    assert!(!said.contains("Parser, old"), "{said}");
}

#[test]
fn a_task_paused_and_resumed_by_the_chief_is_edited_in_the_brief_its_words_follow() {
    let mut w = world();
    let task = w.give("zeus", "Parser, old").task.number;
    w.pause(task);
    let carrier = w.resume(task, "Use JSON").message.expect("its words").id;

    w.ledger
        .edit_task(w.project, task, "chief", "Parser, new")
        .expect("no window has the brief yet");
    let said = pasted(&mut w, carrier);
    assert_eq!(
        said.matches("Parser, new").count(),
        1,
        "the brief once: {said}"
    );
    assert_eq!(said.matches("Resumed: Use JSON").count(), 1, "{said}");
    assert!(!said.contains("Parser, old"), "{said}");
}

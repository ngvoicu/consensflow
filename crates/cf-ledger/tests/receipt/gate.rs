//! The human's gate and what rides in a paste: only words the human passed on
//! are carried, nothing of a task is deliverable while its words wait at the
//! gate, and the words that resume a task never rebuild a brief a row holds.

use crate::fixture::{gated_world, ids, world, World};

/// A task given to zeus in a project with a gate: its brief passed on and
/// received, a question asked, passed on and answered, the answer passed on.
fn answered_behind_the_gate(w: &mut World) -> (i64, i64) {
    let created = w.give("zeus", "Parser");
    let (task, brief) = (created.task.number, created.message.expect("a brief").id);
    w.ledger.approve_message(brief, "human").expect("passed on");
    w.deliver(brief);
    let question = w.ask("zeus", task, "Which format?");
    let answer = w.answer(question.id, "JSON");
    w.ledger
        .approve_message(answer.id, "human")
        .expect("passed on");
    (task, answer.id)
}

#[test]
fn a_chiefs_resume_in_a_gated_project_is_one_gated_carrier_with_the_approved_words_attached() {
    let mut w = gated_world();
    let (task, answer) = answered_behind_the_gate(&mut w);
    w.pause(task);
    let words = w.resume(task, "Use JSON").message.expect("its words");
    assert_eq!(w.states(&[words.id, answer]), ["gated", "queued"]);
    assert_eq!(
        w.next("zeus"),
        None,
        "nothing of the task is deliverable until the human passes the words on"
    );

    w.ledger
        .approve_message(words.id, "human")
        .expect("passed on");
    assert_eq!(w.next("zeus"), Some(words.id));
    let begun = w.deliver(words.id);
    assert_eq!(ids(&begun.carried), [answer]);
    assert_eq!(w.state(task), "working");
}

#[test]
fn a_row_still_held_for_the_human_is_not_folded_and_goes_on_its_own_once_passed_on() {
    let mut w = gated_world();
    let (task, answer) = answered_behind_the_gate(&mut w);
    // A note of the chief's the human has not passed on, as the task is held and goes on.
    let note = w.note("chief", "zeus", task, "Mind the tests");
    assert_eq!(w.message(note.id).state, "gated");
    w.hold(task);
    let words = w.daemon_resumes(task).message.expect("its words");
    let begun = w.deliver(words.id);
    assert_eq!(
        ids(&begun.carried),
        [answer],
        "what the human has not passed on is not in the paste"
    );
    assert_eq!(w.states(&[note.id]), ["gated"]);

    // Passed on later, it is no one's constituent: it is pasted on its own.
    w.ledger
        .approve_message(note.id, "human")
        .expect("passed on");
    assert_eq!(w.next("zeus"), Some(note.id));
}

#[test]
fn declining_the_gated_carrier_cancels_the_task_and_everything_of_it() {
    let mut w = gated_world();
    let (task, answer) = answered_behind_the_gate(&mut w);
    w.pause(task);
    let words = w.resume(task, "Use JSON").message.expect("its words");
    w.ledger
        .decline_message(words.id, "human")
        .expect("declined");
    assert_eq!(w.state(task), "cancelled");
    assert_eq!(w.states(&[words.id, answer]), ["cancelled", "cancelled"]);
    assert_eq!(w.next("zeus"), None);
}

#[test]
fn a_daemon_carrier_beside_a_gated_task_message_is_undeliverable_until_the_approval_adopts_it() {
    let mut w = gated_world();
    let created = w.give("zeus", "Parser");
    let (task, brief) = (created.task.number, created.message.expect("a brief").id);
    assert_eq!(w.message(brief).state, "gated");
    // The task was held before its brief was passed on, and the daemon resumes it.
    w.hold(task);
    let words = w.daemon_resumes(task).message.expect("its words");
    assert_eq!(
        words.body, "Resumed: Go on where you stopped.",
        "a row still holds the brief: the words do not rebuild it"
    );
    assert_eq!(
        words.state, "queued",
        "what the daemon says is the daemon's own"
    );
    assert_eq!(
        w.next("zeus"),
        None,
        "approving the words before the brief would open the window: it is blocked"
    );

    w.ledger.approve_message(brief, "human").expect("passed on");
    assert_eq!(
        w.next("zeus"),
        Some(words.id),
        "one paste: the brief joins it"
    );
    let begun = w.deliver(words.id);
    assert_eq!(ids(&begun.carried), [brief]);
    assert_eq!(w.state(task), "working");
}

#[test]
fn a_brief_nobody_holds_any_more_is_built_into_the_words_that_resume() {
    let mut w = world();
    let created = w.give("zeus", "Parser");
    let (task, brief) = (created.task.number, created.message.expect("a brief").id);
    // The chief paused it before the brief was delivered, and took it back with its words.
    w.pause(task);
    assert_eq!(w.states(&[brief]), ["cancelled"]);
    let words = w.resume(task, "Go on").message.expect("its words");
    assert_eq!(
        words.body, "Parser\n\nResumed: Go on",
        "no task message is received or on its way: the brief goes first"
    );
    let begun = w.deliver(words.id);
    assert!(begun.carried.is_empty());
    assert_eq!(w.state(task), "working");
}

#[test]
fn a_brief_that_was_never_received_goes_in_the_paste_of_the_words_that_resume_the_task() {
    let mut w = world();
    let created = w.give("zeus", "Parser");
    let (task, brief) = (created.task.number, created.message.expect("a brief").id);
    // Held before the window ever received it (the daemon's words, so nothing of the chief's is withdrawn).
    w.hold(task);
    let words = w.daemon_resumes(task).message.expect("its words");
    assert_eq!(words.body, "Resumed: Go on where you stopped.");
    let begun = w.deliver(words.id);
    assert_eq!(ids(&begun.carried), [brief], "the brief rides in it: once");
    assert_eq!(w.state(task), "working");
}

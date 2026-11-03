//! A step that tells one reader of an event and of what it leaves behind says the
//! event first, and says it once (3.0.7-alpha.2, T-1113; the field board of
//! 2026-11-02, where the daemon gave up on T-302 and the chief was sent "T-303
//! waits for T-302, which failed" as m-6900 and "T-302 failed" as m-6901, in one
//! write, a millisecond apart).
//!
//! The chief is given its notes one at a time, the oldest first, so the id order
//! is the order it reads them in. The note of the waiting task went first, the
//! chief acted on the consequence with the cause unknown (it cancelled T-303 and
//! opened another task), and the cause reached it 46 seconds later, when its turn
//! on the first was over, as news it had dealt with. Each message was delivered
//! once: the ledger's log holds one `delivery.begun` and one `delivery.confirmed`
//! for m-6901, and the chief's record one paste of each.

use crate::fixture::{world, World};

/// A task opened for the standard workers, given to a session of zeus and at
/// work: the session and the task.
fn at_work(w: &mut World) -> (String, i64) {
    let task = w.open("Parser").task.number;
    let moved = w.assign(task, "zeus");
    let session = moved.task.assignee.clone().expect("a session");
    w.deliver(moved.message.expect("its brief").id);
    assert_eq!(w.state(task), "working");
    (session, task)
}

/// The notes the chief has been sent, oldest first: the task each is about and
/// its words.
fn chief_notes(w: &World) -> Vec<(Option<i64>, String)> {
    let mut notes: Vec<_> = w
        .ledger
        .inbox(w.id("chief"), 100)
        .expect("the chief's inbox")
        .into_iter()
        .filter(|message| message.kind == "note")
        .collect();
    notes.sort_by_key(|message| message.id);
    notes
        .into_iter()
        .map(|message| (message.task_number, message.body))
        .collect()
}

#[test]
fn a_task_the_daemon_gives_up_on_is_told_to_its_requester_before_the_task_that_waits_for_it() {
    let mut w = world();
    let (_, a) = at_work(&mut w);
    let b = w
        .follow_up(a, "Tests", &[])
        .expect("a follow-up")
        .task
        .number;
    let before = chief_notes(&w).len();

    w.ledger
        .give_up_task(w.project, a, "its pane died")
        .expect("given up");

    let told = &chief_notes(&w)[before..];
    assert_eq!(
        told.first().map(|(task, _)| *task),
        Some(Some(a)),
        "the cause comes first, T-{b}'s wait after it: {told:?}"
    );
    let head = w.next("chief").expect("what the chief is given first");
    assert_eq!(
        w.message(head).task_number,
        Some(a),
        "and it is the first the chief is given"
    );
}

#[test]
fn a_task_given_up_on_and_the_one_waiting_for_it_are_one_note_to_their_requester() {
    let mut w = world();
    let (_, a) = at_work(&mut w);
    let b = w
        .follow_up(a, "Tests", &[])
        .expect("a follow-up")
        .task
        .number;
    let before = chief_notes(&w).len();

    w.ledger
        .give_up_task(w.project, a, "its pane died")
        .expect("given up");

    assert_eq!(
        chief_notes(&w)[before..],
        [(
            Some(a),
            format!(
                "T-{a} failed: its pane died. Reopen it with: cf task reopen T-{a} \"…\", or accept it with what you checked: cf task accept T-{a} \"…\" T-{b} waits for it: cancel T-{b}, or reopen T-{a} and let it finish."
            )
        )],
        "one turn of the chief reads the cause and what it leaves behind"
    );
}

#[test]
fn several_tasks_waiting_for_one_given_up_on_are_named_in_the_one_note() {
    let mut w = world();
    let (_, a) = at_work(&mut w);
    let b = w.open_needing("Docs", &[a]).task.number;
    let c = w.open_needing("Notes", &[a]).task.number;
    let before = chief_notes(&w).len();

    w.ledger
        .give_up_task(w.project, a, "its pane died")
        .expect("given up");

    assert_eq!(
        chief_notes(&w)[before..],
        [(
            Some(a),
            format!(
                "T-{a} failed: its pane died. Reopen it with: cf task reopen T-{a} \"…\", or accept it with what you checked: cf task accept T-{a} \"…\" T-{b}, T-{c} wait for it: cancel them, or reopen T-{a} and let it finish."
            )
        )]
    );
}

#[test]
fn a_task_given_up_on_that_nothing_waits_for_keeps_the_note_it_had() {
    let mut w = world();
    let (_, a) = at_work(&mut w);
    let before = chief_notes(&w).len();

    w.ledger
        .give_up_task(w.project, a, "its pane died")
        .expect("given up");

    assert_eq!(
        chief_notes(&w)[before..],
        [(
            Some(a),
            format!("T-{a} failed: its pane died. Reopen it with: cf task reopen T-{a} \"…\", or accept it with what you checked: cf task accept T-{a} \"…\"")
        )]
    );
}

#[test]
fn a_member_removed_is_told_to_the_chief_before_the_task_that_waited_on_its_work() {
    let mut w = world();
    let (_, a) = at_work(&mut w);
    let c = w.open_needing("Docs", &[a]).task.number;
    // The removal word goes to a requester whose window has started.
    let chief = w.id("chief");
    w.ledger
        .start_conversation(chief, "claude-code")
        .expect("the chief's window has started");
    let before = chief_notes(&w).len();

    w.ledger
        .remove_member(w.project, "zeus")
        .expect("zeus leaves");

    let told = &chief_notes(&w)[before..];
    assert_eq!(
        told.iter().map(|(task, _)| *task).collect::<Vec<_>>(),
        [None, Some(c)],
        "the removal first, T-{c}'s stranded wait after it: {told:?}"
    );
    assert!(
        told[0].1.starts_with("@zeus left the staff"),
        "the first note is the removal's: {told:?}"
    );
    assert_eq!(
        told[1].1,
        format!("T-{c} waits on cancelled T-{a} and will never start.")
    );
}

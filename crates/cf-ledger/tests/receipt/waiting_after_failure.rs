//! What the engine asks the ledger of the tasks that wait for a task a delivery
//! failed (3.0.7-alpha.2, T-1148; the field board of 2026-11-02). A turn that failed is
//! told to the chief by the ledger, in one note, the failure first (`cause_first`). A
//! delivery that did not arrive (a launch that did not come up, a brief or told words
//! that never reached the window) is given up by the ledger, and the words of the
//! failure are the engine's, written after: so the ledger tells no waiter in that step,
//! says in a sentence which of them the requester's note names, and tells the others,
//! each in a note of its own, when it is asked to, after those words.

use cf_ledger::NewTask;

use crate::fixture::{world, World};

/// A task given to zeus whose brief is being pasted, and a task of the chief's that
/// waits for it: the numbers, and the brief.
fn given_with_a_waiter(w: &mut World) -> (i64, i64, i64) {
    let a = w.give("zeus", "Parser");
    let brief = a.message.expect("its brief").id;
    let b = w.open_needing("Docs", &[a.task.number]).task.number;
    w.begin(brief);
    (a.task.number, b, brief)
}

/// A task of the human's that waits for task `a`.
fn human_waits_for(w: &mut World, a: i64) -> i64 {
    w.ledger
        .create_task(
            w.project,
            &NewTask {
                from: "human".into(),
                pool: Some("worker".into()),
                tier: Some("standard".into()),
                needs: vec![a.try_into().expect("a task number")],
                body: "Notes".into(),
                ..NewTask::default()
            },
        )
        .expect("a task for the tier")
        .task
        .number
}

/// The notes `handle` was sent, oldest first: the task each is about and its words.
fn notes_of(w: &World, handle: &str) -> Vec<(Option<i64>, String)> {
    let mut notes: Vec<_> = w
        .ledger
        .inbox(w.id(handle), 100)
        .expect("the inbox")
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
fn a_delivery_given_up_tells_no_waiter_and_the_requesters_note_names_the_ones_it_asked_for() {
    let mut w = world();
    let (a, b, brief) = given_with_a_waiter(&mut w);
    let c = human_waits_for(&mut w, a);
    assert_eq!(
        w.ledger.waiting_for_it(w.project, a, "chief").unwrap(),
        "",
        "T-{a} has not failed"
    );
    let before = notes_of(&w, "chief").len();

    w.ledger
        .give_up_delivery(brief, "its window never came up")
        .expect("given up");

    assert_eq!(w.state(a), "failed");
    assert_eq!(
        notes_of(&w, "chief").len(),
        before,
        "the words of the failure are the engine's, and come first"
    );
    assert!(notes_of(&w, "human").is_empty());
    assert_eq!(
        w.ledger.waiting_for_it(w.project, a, "chief").unwrap(),
        format!(" T-{b} waits for it: cancel T-{b}, or reopen T-{a} and let it finish.")
    );
    assert_eq!(
        w.ledger.waiting_for_it(w.project, a, "human").unwrap(),
        format!(" T-{c} waits for it: cancel T-{c}, or reopen T-{a} and let it finish.")
    );
}

#[test]
fn the_requesters_of_the_other_waiters_are_told_in_notes_of_their_own_and_the_named_one_is_not() {
    let mut w = world();
    let (a, b, brief) = given_with_a_waiter(&mut w);
    let c = human_waits_for(&mut w, a);
    w.ledger
        .give_up_delivery(brief, "its window never came up")
        .expect("given up");

    // The chief's note of the failure names T-b: the human's T-c is told on its own.
    w.ledger
        .tell_waiting_for_failed(w.project, a, "chief")
        .expect("told");
    assert_eq!(
        notes_of(&w, "human"),
        [(
            Some(c),
            format!("T-{c} waits for T-{a}, which failed: cancel T-{c}, or reopen T-{a} and let it finish.")
        )]
    );
    assert!(
        !notes_of(&w, "chief")
            .iter()
            .any(|(task, _)| *task == Some(b)),
        "T-{b} is in the chief's note of the failure"
    );

    // Named the other way round, the chief is told of T-b and the human is not told again.
    w.ledger
        .tell_waiting_for_failed(w.project, a, "human")
        .expect("told");
    assert_eq!(
        notes_of(&w, "chief")
            .into_iter()
            .filter(|(task, _)| *task == Some(b))
            .collect::<Vec<_>>(),
        [(
            Some(b),
            format!("T-{b} waits for T-{a}, which failed: cancel T-{b}, or reopen T-{a} and let it finish.")
        )]
    );
    assert_eq!(notes_of(&w, "human").len(), 1);
}

#[test]
fn a_task_that_has_not_failed_has_no_waiter_told_of_it() {
    let mut w = world();
    let (a, _, _) = given_with_a_waiter(&mut w);
    let (chief, human) = (notes_of(&w, "chief"), notes_of(&w, "human"));

    w.ledger
        .tell_waiting_for_failed(w.project, a, "chief")
        .expect("nothing to tell");

    assert_eq!(w.state(a), "queued");
    assert_eq!(
        (notes_of(&w, "chief"), notes_of(&w, "human")),
        (chief, human)
    );
    assert_eq!(w.ledger.waiting_for_it(w.project, a, "chief").unwrap(), "");
}

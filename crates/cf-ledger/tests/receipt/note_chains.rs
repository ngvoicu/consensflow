//! A chain of ConsensFlow's notes is one message (RS-25): the notes of its own that wait for
//! a chief together, when its window can take the first, go in the first one's paste, a line
//! each, and not one turn each. Each is still a row of its own, which the paste confirms.
//! The chain ends at the first row of another kind: a member's note, a question, a result.

use cf_ledger::{MessageView, NewNote};

use crate::fixture::{world, World};

/// What the failures said, to tell one outage from the next.
const CAUSE: &str = "api error: can't reach the api server (enotfound)";

/// ConsensFlow's own note to the chief, about T-`number` or about no task.
fn told(w: &mut World, number: Option<i64>, body: &str) -> MessageView {
    w.ledger
        .note(
            w.project,
            &NewNote {
                from: None,
                to: "chief".into(),
                body: body.into(),
                task: number,
            },
        )
        .expect("a note")
}

/// Two tasks, queued for the standard workers.
fn two_tasks(w: &mut World) -> (i64, i64) {
    (w.open("Parser").task.number, w.open("Lexer").task.number)
}

fn carried(begun: &cf_ledger::Begun) -> Vec<i64> {
    begun.carried.iter().map(|row| row.id).collect()
}

#[test]
fn the_notes_that_wait_together_for_the_chief_go_in_one_paste() {
    let mut w = world();
    let (one, two) = two_tasks(&mut w);
    let first = told(&mut w, Some(one), "T-1's window closed.");
    let second = told(&mut w, Some(two), "T-2's window closed.");
    let third = told(&mut w, None, "The human changed the staff's rules.");
    let chief = w.id("chief");
    assert_eq!(w.next("chief"), Some(first.id));
    let begun = w.begin(first.id);
    assert_eq!(
        carried(&begun),
        [second.id, third.id],
        "one paste, a line each"
    );
    assert_eq!(w.ledger.next_delivery(chief).expect("a read"), None);
    w.ledger
        .admit(first.id, &carried(&begun))
        .expect("handed over");
    w.confirm(first.id);
    assert_eq!(
        w.states(&[first.id, second.id, third.id]),
        ["delivered", "delivered", "delivered"]
    );
    assert_eq!(w.ledger.next_delivery(chief).expect("a read"), None);
}

#[test]
fn a_chain_ends_at_the_first_row_that_is_not_consensflows_note() {
    let mut w = world();
    let (one, two) = two_tasks(&mut w);
    let first = told(&mut w, Some(one), "T-1's window closed.");
    let words = w.note("human", "chief", one, "Look at T-1 first.");
    let third = told(&mut w, Some(two), "T-2's window closed.");
    let begun = w.begin(first.id);
    assert_eq!(
        carried(&begun),
        Vec::<i64>::new(),
        "the human's words are not ConsensFlow's"
    );
    w.confirm(first.id);
    assert_eq!(w.next("chief"), Some(words.id));
    w.deliver(words.id);
    assert_eq!(w.next("chief"), Some(third.id));
}

#[test]
fn a_chain_withdrawn_before_it_was_handed_over_lets_its_notes_go_on_their_own() {
    let mut w = world();
    let (one, two) = two_tasks(&mut w);
    let first = told(&mut w, Some(one), "T-1's window closed.");
    let second = told(&mut w, Some(two), "T-2's window closed.");
    let third = told(&mut w, None, "The human changed the staff's rules.");
    let begun = w.begin(first.id);
    assert_eq!(carried(&begun), [second.id, third.id]);
    w.ledger
        .cancel_message(first.id, "T-1 moved on")
        .expect("withdrawn");
    assert_eq!(
        w.next("chief"),
        Some(second.id),
        "not stranded under a carrier gone"
    );
    let begun = w.begin(second.id);
    assert_eq!(carried(&begun), [third.id]);
    w.confirm(second.id);
    assert_eq!(w.states(&[second.id, third.id]), ["delivered", "delivered"]);
}

#[test]
fn a_chain_whose_delivery_failed_lets_its_notes_go_on_their_own() {
    let mut w = world();
    let (one, two) = two_tasks(&mut w);
    let first = told(&mut w, Some(one), "T-1's window closed.");
    let second = told(&mut w, Some(two), "T-2's window closed.");
    w.begin(first.id);
    w.ledger
        .fail_delivery(first.id, "the window is gone")
        .expect("failed");
    assert_eq!(w.message(first.id).state, "failed");
    assert_eq!(w.next("chief"), Some(second.id));
}

#[test]
fn a_chain_given_back_carries_none_and_is_gathered_again_from_what_waits() {
    let mut w = world();
    let (one, two) = two_tasks(&mut w);
    let first = told(&mut w, Some(one), "T-1's window closed.");
    let second = told(&mut w, Some(two), "T-2's window closed.");
    let begun = w.begin(first.id);
    assert_eq!(carried(&begun), [second.id]);
    let later = told(&mut w, None, "The human changed the staff's rules.");
    w.ledger
        .retry_delivery(first.id, "the window was busy", true)
        .expect("queued again");
    assert_eq!(w.message(second.id).state, "queued");
    let again = w.begin(first.id);
    assert_eq!(
        carried(&again),
        [second.id, later.id],
        "what waits now, in order"
    );
    w.confirm(first.id);
    assert_eq!(w.states(&[second.id, later.id]), ["delivered", "delivered"]);
}

#[test]
fn a_failure_handed_over_in_a_chain_is_not_written_again_into_the_note_of_the_next() {
    // A failure that waits is written again with the next failure of the same cause, in
    // one note; one that went in a chain's paste already is the chief's, as a note on its
    // way is, and the next failure is a note of its own.
    let mut w = world();
    let first = told(&mut w, None, "The human changed the staff's rules.");
    let zeus = w.give("zeus", "Parser").task.number;
    w.ledger
        .give_up_task_for_cause(w.project, zeus, "@zeus's harness reported a failure", CAUSE)
        .expect("given up");
    let failure = w
        .ledger
        .inbox(w.id("chief"), 100)
        .expect("the inbox")
        .into_iter()
        .find(|message| message.body.starts_with(&format!("T-{zeus} failed: ")))
        .expect("the failure is told")
        .id;
    let begun = w.begin(first.id);
    assert_eq!(carried(&begun), [failure]);
    w.ledger
        .admit(first.id, &carried(&begun))
        .expect("handed over");
    let diana = w.give("diana", "Lexer").task.number;
    w.ledger
        .give_up_task_for_cause(
            w.project,
            diana,
            "@diana's harness reported a failure",
            CAUSE,
        )
        .expect("given up");
    w.confirm(first.id);
    let rode = w.message(failure);
    assert_eq!((rode.state.as_str(), rode.reason), ("delivered", None));
    let next = w.message(w.next("chief").expect("the next failure waits"));
    assert!(
        next.body.starts_with(&format!("T-{diana} failed: ")),
        "{}",
        next.body
    );
}

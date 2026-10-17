//! A hand-over begun is let go to its window by its transport, and the ledger
//! marks it so in the write that admits it (`messages/delivery.rs`'s `admit`):
//! only while the project is open and nothing of its paste was withdrawn since
//! it was begun. A reader of a closed project reads only what a window was handed
//! (Sol's review of 2026-10-16, round 7: a delivery begun before the Close was
//! read whole after it, though it never went in).

use cf_ledger::{Claim, HandOver};

use crate::fixture::{world, World};

/// T-1 given to zeus and at work, paused by the chief, a note of the chief's kept
/// for its window meanwhile, and the words that resume it, which carry the note,
/// begun: the words and the note.
fn resumed_with_a_note(w: &mut World) -> (i64, i64) {
    let created = w.give("zeus", "Parser");
    let (task, brief) = (created.task.number, created.message.expect("a brief").id);
    w.deliver(brief);
    w.pause(task);
    let note = w.note("chief", "zeus", task, "Use tabs");
    let words = w.resume(task, "Go on").message.expect("its words").id;
    let begun = w.begin(words);
    let carried: Vec<i64> = begun.carried.iter().map(|row| row.id).collect();
    assert_eq!(carried, [note.id], "the words carry the note");
    (words, note.id)
}

#[test]
fn a_hand_over_goes_while_the_project_is_open_and_marks_the_message_and_what_it_carries_handed() {
    let mut w = world();
    let (words, note) = resumed_with_a_note(&mut w);
    assert!(
        w.ledger.handed_over(w.project).unwrap().is_empty(),
        "begun is not handed"
    );

    assert_eq!(w.ledger.admit(words, &[note]).unwrap(), HandOver::Admitted);

    assert_eq!(w.ledger.handed_over(w.project).unwrap(), [note, words]);
    assert_eq!(w.states(&[words, note]), ["delivering", "queued"]);
}

#[test]
fn a_hand_over_of_a_closed_project_goes_nowhere_and_hands_nothing() {
    let mut w = world();
    let (words, note) = resumed_with_a_note(&mut w);
    w.ledger
        .set_project_state(w.project, "suspended")
        .expect("closed");

    assert_eq!(w.ledger.admit(words, &[note]).unwrap(), HandOver::Closed);

    assert!(w.ledger.handed_over(w.project).unwrap().is_empty());
}

#[test]
fn a_hand_over_whose_message_or_a_row_of_its_paste_was_withdrawn_since_it_began_goes_nowhere() {
    let mut w = world();
    let (words, note) = resumed_with_a_note(&mut w);
    w.ledger
        .cancel_message(note, "the chief took it back")
        .expect("withdrawn");
    assert_eq!(
        w.ledger.admit(words, &[note]).unwrap(),
        HandOver::Withdrawn,
        "a row of its paste"
    );

    let mut w = world();
    let (words, note) = resumed_with_a_note(&mut w);
    w.ledger
        .cancel_message(words, "the chief took it back")
        .expect("withdrawn");
    assert_eq!(
        w.ledger.admit(words, &[note]).unwrap(),
        HandOver::Withdrawn,
        "the message"
    );
    assert!(w.ledger.handed_over(w.project).unwrap().is_empty());
}

/// One that goes back to its queue was handed nothing: it is handed again, or not
/// at all.
#[test]
fn a_hand_over_sent_back_to_its_queue_is_handed_no_more() {
    let mut w = world();
    let (words, note) = resumed_with_a_note(&mut w);
    w.ledger.admit(words, &[note]).unwrap();

    w.ledger
        .retry_delivery(words, "the window went", true)
        .expect("tried again");

    assert!(w.ledger.handed_over(w.project).unwrap().is_empty());
}

/// What a paste carried is its own again once its carrier is given up, and handed
/// nothing: it goes again in another paste, or not at all.
#[test]
fn what_a_paste_carried_is_handed_no_more_once_its_carrier_is_given_up() {
    let mut w = world();
    let (words, note) = resumed_with_a_note(&mut w);
    w.ledger.admit(words, &[note]).unwrap();

    w.ledger
        .fail_delivery(words, "the window refused it")
        .expect("given up");

    assert_eq!(w.message(note).state, "queued");
    assert!(!w.ledger.handed_over(w.project).unwrap().contains(&note));
}

/// Both reviews of 2026-10-17: a start during an admitted carrier's delivery let
/// go of what it carried with its marks, and a reader of the closed project read
/// it as handed. What a start lets go of is handed nothing.
#[test]
fn what_a_start_lets_go_of_is_handed_no_more() {
    let mut w = world();
    let (words, note) = resumed_with_a_note(&mut w);
    w.ledger.admit(words, &[note]).unwrap();

    // The app quits with the words on their way, and the ledger is opened again.
    w.edit(|_| {});

    assert_eq!(w.message(note).state, "queued");
    assert!(!w.ledger.handed_over(w.project).unwrap().contains(&note));
}

/// Astra's review of 2026-10-17: a message delivered before migration 0020 has
/// no mark, and one its window takes back out of its conversation was read as
/// handed nothing. It was delivered, so it was handed, and so was what its paste
/// carried: the window holds them in its input box.
#[test]
fn a_message_taken_back_was_handed_whether_or_not_its_transport_marked_it() {
    let mut w = world();
    let (words, note) = resumed_with_a_note(&mut w);
    // Delivered as before 0020: no transport marked it.
    w.confirm(words);
    assert!(w.ledger.handed_over(w.project).unwrap().is_empty());

    w.ledger
        .take_back(words, "the window took it back")
        .expect("taken back");

    assert_eq!(w.states(&[words, note]), ["queued", "queued"]);
    assert_eq!(w.ledger.handed_over(w.project).unwrap(), [note, words]);
}

/// A door's claim is a hand-over begun at its window, as a paste let go is.
#[test]
fn an_answer_a_door_claimed_is_handed_over() {
    let mut w = world();
    let created = w.give("zeus", "Parser");
    let (task, brief) = (created.task.number, created.message.expect("a brief").id);
    w.deliver(brief);
    let question = w.ask_with_options("zeus", task);
    let answer = w.choose(question.id, "blue");
    assert!(w.ledger.handed_over(w.project).unwrap().is_empty());
    let zeus = w.id("zeus");

    let claimed = w.ledger.claim_answer(question.id, zeus).unwrap();

    assert!(matches!(claimed, Claim::Answered(..)));
    assert_eq!(w.ledger.handed_over(w.project).unwrap(), [answer.id]);
}

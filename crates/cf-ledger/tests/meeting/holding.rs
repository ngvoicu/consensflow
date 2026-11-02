//! What a meeting holds: every message queued for the chief, in the order it will
//! go, and none that a launch owes the chief's window or that a Switch chief asks
//! with. A member's queue is its own.

use cf_ledger::{CHIEF_CUT, HANDOFF_TITLE, LEAD_HANDOFF_TITLE, SWITCH_ASKED};

use crate::world::world;
use crate::{a_result, handed, head, held, meetings, note_to_chief};

/// A result, a member's note and ConsensFlow's own note queue for the chief, and a
/// member's question after them: none is the head of the chief's queue while the
/// meeting is on, and the board's list says the order they will go in. The Stop
/// lets them go, the question first, then the oldest.
#[test]
fn a_meeting_holds_every_message_queued_for_the_chief_and_its_stop_lets_them_go_a_question_first() {
    let mut w = world();
    w.ledger.set_meeting(w.project.id, true).expect("a meeting");
    let result = a_result(&mut w);
    let note = note_to_chief(&mut w, Some("zeus"), None, "The build is green").id;
    let failed = note_to_chief(&mut w, None, Some(1), "T-1 failed: the window is gone").id;
    let given = w.give(&[]);
    w.start(&given);
    let question = w.ask(given.task.number).id;
    assert_eq!(head(&w), None, "nothing goes to the chief");
    assert_eq!(held(&w), vec![question, result, note, failed]);
    w.ledger.set_meeting(w.project.id, false).expect("stopped");
    assert_eq!(held(&w), Vec::<i64>::new());
    assert_eq!(handed(&mut w), vec![question, result, note, failed]);
    assert_eq!(meetings(&w).len(), 2);
}

/// What a launch owes the chief's window goes in during a meeting: the handoff a
/// chief that takes over is given (in this build's words or an older one's), the
/// note that its turn was cut and a replacement of its role; and so does the note
/// that asks the chief where things stand before the human's Switch chief. A note of
/// a member's, or one of ConsensFlow's about a task, in the same words is held.
#[test]
fn a_meeting_holds_nothing_a_launch_owes_the_chief_nor_the_note_a_switch_chief_asks_with() {
    for opens in [HANDOFF_TITLE, LEAD_HANDOFF_TITLE, CHIEF_CUT, SWITCH_ASKED] {
        let mut w = world();
        w.give(&[]);
        w.ledger.set_meeting(w.project.id, true).expect("a meeting");
        let quoted = note_to_chief(&mut w, Some("zeus"), None, &format!("{opens}. Quoted"));
        let about = note_to_chief(&mut w, None, Some(1), &format!("{opens}: of a task"));
        let owed = note_to_chief(&mut w, None, None, &format!("{opens}. The rest"));
        assert_eq!(head(&w), Some(owed.id), "{opens}");
        assert_eq!(held(&w), vec![quoted.id, about.id], "{opens}");
    }
    let mut w = world();
    w.ledger.set_meeting(w.project.id, true).expect("a meeting");
    let role = w
        .ledger
        .note_role(
            w.project.id,
            "chief",
            "Your role, again",
            "You are the chief",
        )
        .expect("a replacement of a role");
    assert_eq!(head(&w), Some(role.id));
    assert_eq!(held(&w), Vec::<i64>::new());
}

/// The meeting is the chief's: a task for zeus's window goes in while it is on.
#[test]
fn a_members_queue_goes_on_during_a_meeting() {
    let mut w = world();
    w.ledger.set_meeting(w.project.id, true).expect("a meeting");
    let given = w.give(&[]);
    let message = given.message.expect("the task went to its window");
    assert_eq!(
        w.ledger
            .next_delivery(w.id("zeus"))
            .expect("a read")
            .map(|next| next.id),
        Some(message.id)
    );
    assert_eq!(held(&w), Vec::<i64>::new());
}

/// A message on its way to the chief when the meeting starts goes on to its end: a
/// paste is not taken back. It is not on the list of what is held.
#[test]
fn a_message_on_its_way_when_a_meeting_starts_goes_on_and_is_not_held() {
    let mut w = world();
    let note = note_to_chief(&mut w, Some("zeus"), None, "The build is green");
    w.ledger.begin_delivery(note.id).expect("begun");
    w.ledger.set_meeting(w.project.id, true).expect("a meeting");
    assert_eq!(held(&w), Vec::<i64>::new());
    w.ledger
        .confirm_delivery(note.id, Some(&serde_json::json!({ "item": "i-1" })))
        .expect("confirmed");
}

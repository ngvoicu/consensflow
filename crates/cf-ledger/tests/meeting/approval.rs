//! The human's approval and a meeting (3.0.7 chief-meeting, CMT-15): a message
//! between two agents is gated first, the human's to pass on; passed on, it is
//! queued, and a meeting holds it. Whichever of the two the human turned on first,
//! the board lists a message once: among the gated, or among the held. ConsensFlow's
//! own notes are never gated, and a meeting holds them at once.

use cf_ledger::Board;

use crate::world::{gated_world, world, World};
use crate::{handed, head, note_to_chief};

/// The ids the board lists as gated and as held.
fn listed(w: &World) -> (Vec<i64>, Vec<i64>) {
    let board: Board = w.ledger.board(w.project.id).expect("the board");
    (
        board.gated.iter().map(|message| message.id).collect(),
        board.held.iter().map(|message| message.id).collect(),
    )
}

/// zeus's note to the chief is gated, then passed on by the human, then held by the
/// meeting, and the Stop lets it go; ConsensFlow's note goes to the held at once.
fn gated_then_approved_then_held(mut w: World) {
    let words = note_to_chief(&mut w, Some("zeus"), None, "The build is green");
    assert_eq!(words.state, "gated");
    assert_eq!(listed(&w), (vec![words.id], vec![]), "gated, and only so");
    let own = note_to_chief(&mut w, None, None, "T-9 failed: the window is gone");
    assert_eq!(own.state, "queued", "ConsensFlow's own note is never gated");
    assert_eq!(listed(&w), (vec![words.id], vec![own.id]));
    w.approve(&words);
    assert_eq!(
        listed(&w),
        (vec![], vec![words.id, own.id]),
        "held, and only so"
    );
    assert_eq!(head(&w), None);
    w.ledger.set_meeting(w.project.id, false).expect("stopped");
    assert_eq!(listed(&w), (vec![], vec![]));
    assert_eq!(handed(&mut w), vec![words.id, own.id]);
}

#[test]
fn with_the_gate_on_before_the_meeting_a_message_is_gated_then_approved_then_held_once() {
    let mut w = gated_world();
    w.ledger.set_meeting(w.project.id, true).expect("a meeting");
    gated_then_approved_then_held(w);
}

#[test]
fn with_the_meeting_on_before_the_gate_a_message_is_gated_then_approved_then_held_once() {
    let mut w = world();
    w.ledger.set_meeting(w.project.id, true).expect("a meeting");
    w.ledger.set_gate(w.project.id, true).expect("the gate");
    gated_then_approved_then_held(w);
}

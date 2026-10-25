//! Whether the host proved the program of the last window on a conversation gone, kept
//! on the conversation (migration 0023), so that a restart keeps it (T-541): written
//! for a participant's conversation now, read by the conversation's id, never on another
//! conversation, and none on a conversation the human switches a window to, where that
//! window now writes. A ledger from before the column proves nothing.

use crate::fixture::{world, World};

/// zeus's conversation now, started.
fn conversation_of_zeus(w: &mut World) -> i64 {
    let zeus = w.id("zeus");
    let started = w
        .ledger
        .start_conversation(zeus, "claude-code")
        .expect("started");
    w.ledger
        .bind_conversation(started.id, "zeus-one")
        .expect("bound");
    started.id
}

#[test]
fn the_proof_is_kept_on_the_participants_conversation_now_and_taken_back_before_a_window_opens() {
    let mut w = world();
    let first = conversation_of_zeus(&mut w);
    let zeus = w.id("zeus");
    assert!(!w.ledger.program_gone(first).unwrap(), "none at the start");
    w.ledger.set_program_gone(zeus, true).unwrap();
    assert!(w.ledger.program_gone(first).unwrap());
    w.ledger.set_program_gone(zeus, false).unwrap();
    assert!(
        !w.ledger.program_gone(first).unwrap(),
        "a window opens on it"
    );

    // Proven on the first; a new conversation starts with none, and the first, ended,
    // keeps what it had as a window opens on the second.
    w.ledger.set_program_gone(zeus, true).unwrap();
    let second = w.ledger.start_conversation(zeus, "claude-code").unwrap().id;
    assert!(!w.ledger.program_gone(second).unwrap());
    w.ledger.set_program_gone(zeus, false).unwrap();
    assert!(w.ledger.program_gone(first).unwrap(), "the first keeps it");
    w.ledger.set_program_gone(zeus, true).unwrap();
    assert!(w.ledger.program_gone(second).unwrap());
    let diana = w.id("diana");
    w.ledger.set_program_gone(diana, false).unwrap();
    assert!(
        w.ledger.program_gone(second).unwrap(),
        "another participant's write is its own"
    );
    assert_eq!(
        w.ledger.program_gone(9_999).unwrap_err().code(),
        Some("unknown-conversation")
    );
}

#[test]
fn a_conversation_the_human_switches_a_window_back_to_keeps_no_proof_of_the_window_before() {
    let mut w = world();
    let zeus = w.id("zeus");
    let first = conversation_of_zeus(&mut w);
    w.ledger.set_program_gone(zeus, true).unwrap();
    // A window opens on a new conversation, and the human switches it back to the first
    // (/resume): that window writes to it now.
    let second = w.ledger.start_conversation(zeus, "claude-code").unwrap().id;
    w.ledger.set_program_gone(zeus, false).unwrap();
    let back = w
        .ledger
        .follow_conversation(zeus, "claude-code", "zeus-one")
        .unwrap();
    assert_eq!(back.id, first);
    assert!(
        !w.ledger.program_gone(first).unwrap(),
        "its window writes to it"
    );
    assert!(!w.ledger.program_gone(second).unwrap());
}

#[test]
fn a_ledger_from_before_the_column_proves_no_program_gone() {
    let mut w = world();
    let first = conversation_of_zeus(&mut w);
    w.edit(|db| {
        cf_ledger::testing::undo_migration_0023(db).expect("undone");
        db.execute_batch("PRAGMA user_version = 22")
            .expect("version 22");
    });
    assert!(!w.ledger.program_gone(first).unwrap());
}

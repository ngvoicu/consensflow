//! The turn a record had open when a window was opened on it, and how long a
//! later look still shows that turn and no more.

use std::sync::Arc;

use super::*;
use crate::records::{Item, Record, Role};

fn item(id: &str, role: Role) -> Item {
    Item {
        id: Arc::from(id),
        role,
        text: Arc::from(""),
        complete: false,
        at: None,
        commentary: false,
        compaction: false,
    }
}

/// A reading of `items` whose turn is `settlement`.
fn reading(items: &[(&str, Role)], settlement: Settlement) -> Reading {
    let mut record = Record::new();
    record.items = items.iter().map(|(id, role)| item(id, *role)).collect();
    record.settlement = settlement;
    Reading::Known(record)
}

const CUT: &[(&str, Role)] = &[("u1", Role::User), ("a1", Role::Assistant)];

#[test]
fn a_turn_the_record_has_not_ended_is_the_one_the_window_was_opened_on_and_one_that_ended_is_none()
{
    // In flight, or one whose end the record cannot say: a final answer its harness never
    // marked as the last (Devin's GPT models'), a turn that failed (Pi's).
    for settlement in [Settlement::InFlight, Settlement::Unknown] {
        assert!(
            LeftOpen::of(&reading(CUT, settlement)).is_some(),
            "{settlement:?}"
        );
    }
    assert_eq!(LeftOpen::of(&reading(CUT, Settlement::Settled)), None);
    // Nothing in the record, and a record that could not be read, are no turn.
    for settlement in [Settlement::InFlight, Settlement::Unknown] {
        assert_eq!(LeftOpen::of(&reading(&[], settlement)), None);
    }
    assert_eq!(
        LeftOpen::of(&Reading::Unknown("unreadable: gone".to_owned())),
        None
    );
}

#[test]
fn the_turn_is_still_there_while_the_record_holds_no_more_than_it_did() {
    for at_open in [Settlement::InFlight, Settlement::Unknown] {
        let open = LeftOpen::of(&reading(CUT, at_open)).unwrap();
        for now in [Settlement::InFlight, Settlement::Unknown] {
            assert!(open.still(&reading(CUT, now)), "{at_open:?}, then {now:?}");
        }
    }
}

#[test]
fn a_record_that_grew_a_turn_of_its_own_or_ended_the_old_one_is_not_the_turn_left() {
    let open = LeftOpen::of(&reading(CUT, Settlement::InFlight)).unwrap();
    // A message was given to the window: a user's item after the old ones.
    let given = [CUT, &[("u2", Role::User)]].concat();
    assert!(!open.still(&reading(&given, Settlement::InFlight)));
    assert!(!open.still(&reading(&given, Settlement::Unknown)));
    // The turn of the window ended: whatever it holds is its own.
    assert!(!open.still(&reading(&given, Settlement::Settled)));
    assert!(!open.still(&reading(CUT, Settlement::Settled)));
    // Another conversation's record, of as many items.
    let other = [("x1", Role::User), ("x2", Role::Assistant)];
    assert!(!open.still(&reading(&other, Settlement::InFlight)));
    // A record that cannot be read says nothing of it.
    assert!(!open.still(&Reading::Unknown("unreadable: gone".to_owned())));
}

#[test]
fn what_the_record_held_at_the_open_is_held_whatever_it_said_of_its_turn() {
    // A turn that ended is no turn left open, but the messages it holds are the old
    // process's: a window that has been given nothing has begun none of them.
    for settlement in [
        Settlement::InFlight,
        Settlement::Unknown,
        Settlement::Settled,
    ] {
        let held = LeftOpen::held(&reading(CUT, settlement)).unwrap();
        assert!(
            held.unchanged(&reading(CUT, Settlement::Settled)),
            "{settlement:?}"
        );
        assert!(
            held.unchanged(&reading(CUT, Settlement::InFlight)),
            "{settlement:?}"
        );
        let given = [CUT, &[("u2", Role::User)]].concat();
        assert!(
            !held.unchanged(&reading(&given, Settlement::Settled)),
            "{settlement:?}"
        );
        let other = [("x1", Role::User), ("x2", Role::Assistant)];
        assert!(
            !held.unchanged(&reading(&other, Settlement::Settled)),
            "{settlement:?}"
        );
        assert!(!held.unchanged(&Reading::Unknown("unreadable: gone".to_owned())));
    }
    // Nothing in the record, and a record that could not be read, hold nothing.
    assert_eq!(LeftOpen::held(&reading(&[], Settlement::Settled)), None);
    assert_eq!(
        LeftOpen::held(&Reading::Unknown("unreadable: gone".to_owned())),
        None
    );
}

#[test]
fn what_the_old_process_wrote_after_the_open_is_no_turn_the_window_began() {
    // Devin's agent outlives its window by a second or two and writes the rows of its
    // last step: its harness's own notes, the call it was making. A turn of the window's
    // own begins with a message of the user's, and nothing else is one.
    let held = LeftOpen::held(&reading(CUT, Settlement::Unknown)).unwrap();
    for later in [
        [CUT, &[("n1", Role::Custom)]].concat(),
        [CUT, &[("t1", Role::Tool)]].concat(),
        [CUT, &[("a2", Role::Assistant), ("n2", Role::Custom)]].concat(),
    ] {
        for settlement in [
            Settlement::InFlight,
            Settlement::Unknown,
            Settlement::Settled,
        ] {
            assert!(held.unchanged(&reading(&later, settlement)), "{later:?}");
        }
    }
    let begun = [CUT, &[("n1", Role::Custom), ("u2", Role::User)]].concat();
    assert!(!held.unchanged(&reading(&begun, Settlement::InFlight)));
}

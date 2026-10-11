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
fn a_turn_in_flight_is_the_one_the_window_was_opened_on_and_one_that_ended_is_none() {
    assert!(LeftOpen::of(&reading(CUT, Settlement::InFlight)).is_some());
    for settlement in [Settlement::Settled, Settlement::Unknown] {
        assert_eq!(
            LeftOpen::of(&reading(CUT, settlement)),
            None,
            "{settlement:?}"
        );
    }
    // Nothing in the record, and a record that could not be read, are no turn.
    assert_eq!(LeftOpen::of(&reading(&[], Settlement::InFlight)), None);
    assert_eq!(
        LeftOpen::of(&Reading::Unknown("unreadable: gone".to_owned())),
        None
    );
}

#[test]
fn the_turn_is_still_there_while_the_record_holds_no_more_than_it_did() {
    let open = LeftOpen::of(&reading(CUT, Settlement::InFlight)).unwrap();
    assert!(open.still(&reading(CUT, Settlement::InFlight)));
}

#[test]
fn a_record_that_grew_a_turn_of_its_own_or_ended_the_old_one_is_not_the_turn_left() {
    let open = LeftOpen::of(&reading(CUT, Settlement::InFlight)).unwrap();
    // A message was given to the window: a user's item after the old ones.
    let given = [CUT, &[("u2", Role::User)]].concat();
    assert!(!open.still(&reading(&given, Settlement::InFlight)));
    // The turn of the window ended: whatever it holds is its own.
    assert!(!open.still(&reading(&given, Settlement::Settled)));
    assert!(!open.still(&reading(CUT, Settlement::Settled)));
    // Another conversation's record, of as many items.
    let other = [("x1", Role::User), ("x2", Role::Assistant)];
    assert!(!open.still(&reading(&other, Settlement::InFlight)));
    // A record that cannot be read says nothing of it.
    assert!(!open.still(&Reading::Unknown("unreadable: gone".to_owned())));
}

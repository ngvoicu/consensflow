//! When an interrupt is what ended a turn Claude wrote nothing of, and when
//! it is not: a turn just pasted has no press, an old press is for the turn
//! before it, and a turn that has begun to be answered is read as it always
//! was.

use super::*;
use crate::records::Item;

/// Item `id`, from `role`.
fn item(id: &str, role: Role) -> Item {
    Item {
        id: Arc::from(id),
        role,
        text: Arc::from("text"),
        complete: role == Role::Assistant,
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

/// What a look reads of a turn just begun by the user's message `u2`, after a
/// turn that ended with an answer.
fn begun(settlement: Settlement) -> Reading {
    reading(
        &[
            ("u1", Role::User),
            ("a1", Role::Assistant),
            ("u2", Role::User),
        ],
        settlement,
    )
}

/// Whether a press at 10,000 ms over `over` stopped what `reading` shows, `after` ms later.
fn stopped_after(over: &str, reading: &Reading, after: i64) -> bool {
    let Reading::Known(record) = reading else {
        panic!("a reading that is known");
    };
    Pressed::new(10_000, Arc::from(over)).stopped(record, 10_000 + after)
}

#[test]
fn a_press_over_the_turn_in_flight_stopped_it_once_a_moment_has_passed_with_nothing_written() {
    let turn = begun(Settlement::InFlight);
    assert!(!stopped_after("u2", &turn, 0), "not at the press");
    assert!(
        !stopped_after("u2", &turn, 999),
        "not before the hold is over"
    );
    assert!(stopped_after("u2", &turn, 1_000));
    assert!(stopped_after("u2", &turn, 600_000), "and not forgotten");
}

#[test]
fn a_turn_just_pasted_is_not_stopped_by_the_press_for_the_turn_before_it() {
    // The words of a resume went in after the press: the user's last message
    // is theirs, with no press over it, and Claude has not begun it.
    let pasted = reading(
        &[("u1", Role::User), ("u2", Role::User), ("u3", Role::User)],
        Settlement::InFlight,
    );
    assert!(!stopped_after("u2", &pasted, 60_000));
}

#[test]
fn a_turn_with_a_word_of_the_assistants_or_a_tools_output_is_not_one_stopped_before_it_began() {
    for (name, written) in [
        ("an assistant's word", Role::Assistant),
        ("a tool's output", Role::Tool),
    ] {
        let turn = reading(&[("u2", Role::User), ("w1", written)], Settlement::InFlight);
        assert!(!stopped_after("u2", &turn, 60_000), "{name}");
    }
}

#[test]
fn what_a_hook_added_to_the_prompt_is_not_a_word_of_the_turn() {
    let turn = reading(
        &[("u2", Role::User), ("h1", Role::Custom)],
        Settlement::InFlight,
    );
    assert!(stopped_after("u2", &turn, 1_000));
}

#[test]
fn a_turn_the_record_says_ended_or_says_nothing_of_needs_no_press_to_be_read() {
    for settlement in [Settlement::Settled, Settlement::Unknown] {
        let turn = begun(settlement);
        assert!(!stopped_after("u2", &turn, 60_000), "{settlement:?}");
    }
}

#[test]
fn a_reading_with_no_message_of_the_users_is_stopped_by_nothing() {
    let turn = reading(&[("a1", Role::Assistant)], Settlement::InFlight);
    assert!(!stopped_after("u1", &turn, 60_000));
    assert!(!stopped_after(
        "",
        &reading(&[], Settlement::InFlight),
        60_000
    ));
}

#[test]
fn the_turn_a_window_is_in_is_the_users_last_message_of_what_it_read() {
    let id = |reading: &Reading| last_user(reading).map(|id| id.to_string());
    assert_eq!(
        id(&begun(Settlement::InFlight)),
        Some("u2".to_owned()),
        "the last of two"
    );
    assert_eq!(
        id(&reading(&[("a1", Role::Assistant)], Settlement::Unknown)),
        None
    );
    assert_eq!(id(&Reading::Unknown("unreadable".to_owned())), None);
}

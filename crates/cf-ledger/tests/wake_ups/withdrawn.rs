//! What is withdrawn: a wake-up that waits for its turn goes when the project
//! closes, or comes back, or the calls are turned off. One that is being
//! delivered is the window's to settle, and one delivered is not touched.

use cf_ledger::WakeUpUnit;

use crate::world::{calls, world, MINUTE};

/// A world with a wake-up queued and waiting: the chief has not been given it.
fn waiting() -> (crate::world::World, cf_ledger::MessageView) {
    let mut w = world();
    w.turn_on(1, WakeUpUnit::Minutes);
    w.advance(MINUTE);
    let sent = w.due().expect("a wake-up");
    (w, sent)
}

#[test]
fn a_close_withdraws_a_wake_up_that_waits_and_it_is_never_delivered() {
    let (mut w, sent) = waiting();
    w.ledger
        .set_project_state(w.project.id, "suspended")
        .unwrap();
    let withdrawn = w.ledger.message(sent.id).unwrap().unwrap();
    assert_eq!(
        (withdrawn.state.as_str(), withdrawn.reason.as_deref()),
        ("cancelled", Some("the project was closed"))
    );
    assert!(w.ledger.next_delivery(w.id("chief")).unwrap().is_none());
    assert!(w.kinds().contains(&"delivery.cancelled".to_owned()));
}

#[test]
fn a_resume_withdraws_one_that_came_back_to_the_queue_after_the_close_and_the_count_begins_again() {
    let (mut w, sent) = waiting();
    // The window closed while it was handed over: the delivery settled after the close.
    w.ledger.begin_delivery(sent.id).unwrap();
    w.ledger
        .set_project_state(w.project.id, "suspended")
        .unwrap();
    assert_eq!(
        w.state_of(&sent),
        "delivering",
        "a delivery is the window's to settle"
    );
    w.ledger
        .retry_delivery(sent.id, "its window closed", true)
        .unwrap();
    assert_eq!(w.state_of(&sent), "queued");
    w.ledger.set_project_state(w.project.id, "open").unwrap();
    assert_eq!(
        w.state_of(&sent),
        "cancelled",
        "stale by the time the chief is back"
    );
    // And a new one is due once an interval has passed since the Resume.
    assert!(w.due().is_none());
    w.advance(MINUTE);
    let next = w.due().expect("a wake-up of the Resume's own");
    assert_ne!(next.id, sent.id);
}

#[test]
fn a_restart_withdraws_a_wake_up_that_waits_as_a_close_does() {
    let (mut w, sent) = waiting();
    w.ledger.suspend_for_restart().unwrap();
    assert_eq!(w.state_of(&sent), "cancelled");
}

#[test]
fn turning_the_calls_off_withdraws_a_wake_up_not_yet_admitted_and_leaves_one_delivered() {
    let (mut w, sent) = waiting();
    w.set(&calls(false, 1, WakeUpUnit::Minutes));
    let withdrawn = w.ledger.message(sent.id).unwrap().unwrap();
    assert_eq!(
        (withdrawn.state.as_str(), withdrawn.reason.as_deref()),
        ("cancelled", Some("wake-up calls were turned off"))
    );
    // One that reached the chief stays as it was.
    w.set(&calls(true, 1, WakeUpUnit::Minutes));
    w.advance(MINUTE);
    let second = w.wake_and_deliver();
    w.set(&calls(false, 1, WakeUpUnit::Minutes));
    assert_eq!(w.state_of(&second), "delivered");
    // One being delivered is the window's.
    w.set(&calls(true, 1, WakeUpUnit::Minutes));
    w.advance(MINUTE);
    w.move_a_task();
    let third = w.due().expect("a third");
    w.ledger.begin_delivery(third.id).unwrap();
    w.set(&calls(false, 1, WakeUpUnit::Minutes));
    assert_eq!(w.state_of(&third), "delivering");
}

#[test]
fn a_change_of_the_words_or_the_interval_leaves_a_wake_up_that_waits_as_it_is() {
    let (mut w, sent) = waiting();
    w.set(&calls(true, 5, WakeUpUnit::Minutes));
    assert_eq!(w.state_of(&sent), "queued");
}

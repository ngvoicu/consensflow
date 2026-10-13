//! When a wake-up is due: once the interval has passed since the latest of the
//! calls being turned on, the last wake-up (and its delivery) and the last
//! Resume; never while the calls are off or the project closed; never while a
//! wake-up is still on its way; and one only, however late the pass is.

use cf_ledger::{NewWakeUp, WakeUpUnit, DEFAULT_WAKE_UP_TEXT};

use crate::world::{calls, world, DAY, HOUR, MINUTE};

#[test]
fn a_wake_up_is_due_once_the_interval_has_passed_and_is_a_note_from_the_human_to_the_chief() {
    let mut w = world();
    w.turn_on(5, WakeUpUnit::Minutes);
    w.advance(5 * MINUTE - 1);
    assert!(w.due().is_none(), "a millisecond short of it");
    w.advance(1);
    let sent = w.due().expect("the interval has passed");
    assert_eq!(
        (
            sent.kind.as_str(),
            sent.sender.as_deref(),
            sent.recipient.as_str(),
            sent.state.as_str(),
            sent.body.as_str(),
            sent.task_number
        ),
        (
            "note",
            Some("human"),
            "chief",
            "queued",
            DEFAULT_WAKE_UP_TEXT,
            None
        ),
        "queued as any note is: no gate holds a note of the human's"
    );
    // The note is sent, then the wake-up logged: so the note is never taken
    // for what moved since it.
    let kinds = w.kinds();
    let at = kinds
        .iter()
        .position(|kind| kind == "project.wake_up")
        .unwrap();
    assert_eq!(kinds[at - 1], "message.sent");
    assert_eq!(
        w.ledger
            .next_delivery(w.id("chief"))
            .unwrap()
            .map(|next| next.id),
        Some(sent.id),
        "the chief's queue holds it"
    );
}

#[test]
fn the_words_a_wake_up_says_are_the_ones_the_human_wrote() {
    let mut w = world();
    w.set(&NewWakeUp {
        text: Some("Carry on, or say what is left.".to_owned()),
        ..calls(true, 1, WakeUpUnit::Minutes)
    });
    w.advance(MINUTE);
    assert_eq!(w.due().unwrap().body, "Carry on, or say what is left.");
}

#[test]
fn nothing_is_due_while_the_calls_are_off_and_none_comes_of_a_project_that_is_closed() {
    let mut w = world();
    w.advance(30 * DAY);
    assert!(w.due().is_none(), "off by default, whatever the clock says");
    w.turn_on(1, WakeUpUnit::Minutes);
    w.ledger
        .set_project_state(w.project.id, "suspended")
        .unwrap();
    w.advance(30 * DAY);
    assert!(w.due().is_none(), "a closed project is sent nothing");
    w.ledger.set_project_state(w.project.id, "open").unwrap();
    w.advance(MINUTE);
    assert!(w.due().is_some(), "and the count began again at the Resume");
}

#[test]
fn the_count_restarts_at_the_resume_whatever_had_passed_before_it() {
    let mut w = world();
    w.turn_on(10, WakeUpUnit::Minutes);
    w.advance(9 * MINUTE);
    w.ledger
        .set_project_state(w.project.id, "suspended")
        .unwrap();
    w.advance(DAY);
    w.ledger.set_project_state(w.project.id, "open").unwrap();
    w.advance(10 * MINUTE - 1);
    assert!(
        w.due().is_none(),
        "ten minutes from the Resume, not from before it"
    );
    w.advance(1);
    assert!(w.due().is_some());
}

#[test]
fn a_wake_up_on_its_way_is_never_followed_by_another_however_many_intervals_pass() {
    let mut w = world();
    w.turn_on(1, WakeUpUnit::Minutes);
    w.advance(MINUTE);
    let first = w.due().expect("the first");
    // Something moves since it, so that nothing but the one on its way holds the
    // next back: a quiet board would.
    w.move_a_task();
    w.advance(10 * MINUTE);
    assert!(w.due().is_none(), "still queued: not followed");
    w.ledger.begin_delivery(first.id).unwrap();
    w.advance(10 * MINUTE);
    assert!(w.due().is_none(), "being delivered: not followed");
    assert_eq!(w.queued(), 1);
    // Once it was delivered, the interval since it passed, the next is due.
    w.ledger
        .confirm_delivery(first.id, Some(&serde_json::json!({ "item": "i-1" })))
        .unwrap();
    w.advance(MINUTE);
    assert!(
        w.due().is_some(),
        "delivered, and the board moved: the next"
    );
}

#[test]
fn a_late_pass_sends_one_wake_up_and_never_a_burst() {
    let mut w = world();
    w.turn_on(1, WakeUpUnit::Minutes);
    w.advance(3 * DAY);
    let first = w.due().expect("one, for all the days that passed");
    assert!(w.due().is_none(), "and not another on the same pass");
    w.deliver(&first);
    w.move_a_task();
    assert!(
        w.due().is_none(),
        "delivered, and the board moved, but a minute has not passed since it"
    );
    w.advance(MINUTE);
    assert!(w.due().is_some());
    assert_eq!(w.queued(), 2);
}

#[test]
fn the_interval_counts_from_the_latest_of_the_calls_turned_on_the_last_wake_up_its_delivery_the_resume_and_a_change_of_the_setting(
) {
    let mut w = world();
    // From the calls turned on.
    w.turn_on(10, WakeUpUnit::Minutes);
    w.advance(10 * MINUTE);
    let first = w.due().expect("ten minutes after the calls were turned on");
    // From the delivery, where the wake-up waited for it: queued at ten, delivered at twenty.
    w.advance(10 * MINUTE);
    w.deliver(&first);
    w.move_a_task();
    w.advance(10 * MINUTE - 1);
    assert!(
        w.due().is_none(),
        "ten minutes after it was delivered, not after it was queued"
    );
    w.advance(1);
    let second = w.due().expect("ten minutes after the delivery");
    w.deliver(&second);
    // From a change of the setting (the interval, here): the count begins at it.
    w.move_a_task();
    w.advance(6 * MINUTE);
    w.set(&calls(true, 8, WakeUpUnit::Minutes));
    w.advance(8 * MINUTE - 1);
    assert!(
        w.due().is_none(),
        "eight minutes after the change, which had been six minutes after the wake-up"
    );
    w.advance(1);
    let third = w.due().expect("eight minutes after the change");
    w.deliver(&third);
    // From the Resume.
    w.move_a_task();
    w.advance(5 * MINUTE);
    w.ledger
        .set_project_state(w.project.id, "suspended")
        .unwrap();
    w.advance(HOUR);
    w.ledger.set_project_state(w.project.id, "open").unwrap();
    w.advance(8 * MINUTE - 1);
    assert!(w.due().is_none());
    w.advance(1);
    assert!(w.due().is_some());
}

#[test]
fn an_interval_of_hours_and_one_of_days_are_counted_whole() {
    let mut w = world();
    w.turn_on(2, WakeUpUnit::Hours);
    w.advance(2 * HOUR - 1);
    assert!(w.due().is_none());
    w.advance(1);
    w.wake_and_deliver();
    w.move_a_task();
    w.set(&calls(true, 3, WakeUpUnit::Days));
    w.advance(3 * DAY - 1);
    assert!(w.due().is_none());
    w.advance(1);
    assert!(w.due().is_some());
}

//! How wake-ups stop (the calls turned off, a Close that withdraws one that
//! waits) and what one leaves behind: nothing but its own delivery.

use std::cell::Cell;
use std::rc::Rc;

use cf_engine::testing::Context;
use cf_harness::contract::{Held, Readiness};
use cf_ledger::{WakeUpUnit, DEFAULT_WAKE_UP_TEXT};

use super::{at_rest, given, moved, passes, set, woken, DAY, MINUTE};

#[test]
fn the_calls_turned_off_stop_at_once_and_a_setting_made_while_it_runs_applies_at_once() {
    let context = Context::new();
    let project = at_rest(&context);
    set(&context, &project, true, 1, WakeUpUnit::Hours);
    context.advance(30 * MINUTE);
    passes(&context);
    assert_eq!(given(&context), 0);
    // The interval shortened: counted from the change, no relaunch.
    set(&context, &project, true, 5, WakeUpUnit::Minutes);
    context.advance(5 * MINUTE);
    passes(&context);
    assert_eq!(given(&context), 1);
    context.adapter.answer("chief", "Nothing is left.");
    context.pass().unwrap();
    moved(&context, &project);
    set(&context, &project, false, 5, WakeUpUnit::Minutes);
    context.advance(DAY);
    passes(&context);
    assert_eq!(given(&context), 1, "turned off: no more");
}

#[test]
fn a_close_withdraws_a_wake_up_that_waits_and_one_that_was_being_handed_over_when_the_window_went()
{
    let context = Context::new();
    let project = at_rest(&context);
    set(&context, &project, true, 1, WakeUpUnit::Minutes);
    // One that waits for the human's text.
    let typed = Rc::new(Cell::new(true));
    let asked = Rc::clone(&typed);
    *context.adapter.ready.borrow_mut() = Some(Rc::new(move || {
        Ok(if asked.get() {
            Readiness::Held(Held::Unsent)
        } else {
            Readiness::Ready
        })
    }));
    context.advance(MINUTE);
    passes(&context);
    let chief = context.id(project.id, "chief");
    let waiting = context
        .inbox(chief)
        .into_iter()
        .find(|message| message.body == DEFAULT_WAKE_UP_TEXT)
        .expect("a wake-up waits");
    assert_eq!(waiting.state, "queued");
    context.close_project(project.id).unwrap();
    assert_eq!(
        context.message(waiting.id).state,
        "cancelled",
        "closed: withdrawn"
    );
    context.resume_project(project.id).unwrap();
    typed.set(false);
    passes(&context);
    assert_eq!(
        given(&context),
        0,
        "a Resume does not deliver what the Close withdrew"
    );
    // One handed over that the window never showed: delivering when the Close comes.
    context.adapter.answer("chief", "Back.");
    passes(&context);
    context.adapter.with("chief", |agent| agent.arrive = false);
    context.advance(MINUTE);
    passes(&context);
    let handed = context
        .inbox(chief)
        .into_iter()
        .find(|message| message.id > waiting.id && message.body == DEFAULT_WAKE_UP_TEXT)
        .expect("a second wake-up");
    assert_eq!(handed.state, "delivering");
    context.close_project(project.id).unwrap();
    assert_eq!(
        context.message(handed.id).state,
        "cancelled",
        "the window's exit gave it back to the queue, and the Close withdrew it"
    );
    context.resume_project(project.id).unwrap();
    passes(&context);
    assert_eq!(given(&context), 0, "neither comes after the Resume");
    assert_eq!(context.message(handed.id).state, "cancelled");
}

#[test]
fn the_chiefs_reply_stays_in_its_terminal_and_the_board_and_the_humans_inbox_gain_nothing() {
    let context = Context::new();
    let project = at_rest(&context);
    set(&context, &project, true, 1, WakeUpUnit::Minutes);
    let human = context.id(project.id, "human");
    let last_event = context
        .ledger
        .borrow()
        .events(project.id, 0, 5000)
        .unwrap()
        .last()
        .map_or(0, |event| event.id);
    context.advance(MINUTE);
    woken(&context, &project);
    assert_eq!(given(&context), 1);
    assert_eq!(
        &*context.adapter.agent("chief").items.last().unwrap().text,
        "Nothing is left."
    );
    assert!(
        context.inbox(human).is_empty(),
        "the human's inbox gained nothing"
    );
    let board = context.ledger.borrow().board(project.id).unwrap();
    assert!(
        board.lanes.iter().all(|lane| lane.tasks.is_empty()) && board.open.is_empty(),
        "no task was made"
    );
    let kinds: Vec<String> = context
        .ledger
        .borrow()
        .events(project.id, last_event, 5000)
        .unwrap()
        .into_iter()
        .map(|event| event.kind)
        .collect();
    assert_eq!(
        kinds,
        [
            "message.sent",
            "project.wake_up",
            "delivery.begun",
            "delivery.confirmed"
        ],
        "the wake-up, and its delivery: and nothing of the chief's reply"
    );
}

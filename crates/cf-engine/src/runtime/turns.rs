//! The turns JavaScript's promises held a participant, and kept a waiter
//! from it, in `#exclusive` and `#act` (`src/core/dispatcher.js`), counted
//! on the kit's executor, where each turn is a trip through the queue.

use std::cell::RefCell;
use std::rc::Rc;

use super::tests::{gated, Log};
use super::*;
use crate::testing::{Executor, Gate};

/// What `hold` held at each of the next `turns` turns, as work of its own
/// sees it: each turn, the work begun before it has gone first.
fn held_each_turn(executor: &Executor, hold: &Rc<Hold>, turns: usize) -> Rc<RefCell<Vec<bool>>> {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let (hold, record) = (Rc::clone(hold), Rc::clone(&seen));
    executor.spawn(Box::pin(async move {
        for _ in 0..turns {
            record.borrow_mut().push(hold.held());
            next_turn().await;
        }
    }));
    seen
}

#[test]
fn a_work_that_ends_where_it_is_begun_holds_its_participant_three_turns_as_javascripts_promises_did(
) {
    // `begin(work)` adopts a promise that was settled already two turns
    // later, and the `finally` that lets go runs a turn after that.
    let executor = Rc::new(Executor::default());
    let hold = Rc::new(Hold::default());
    let (spawn, held) = (Rc::clone(&executor), Rc::clone(&hold));
    let answer = executor.finish(async move {
        let begun = held.exclusive(&*spawn, async { 7 }).await;
        assert!(held.held(), "it holds its participant where it is begun");
        begun.await
    });
    assert_eq!(answer, Some(7));
    assert!(!hold.held());
    assert_eq!(executor.waiting(), 0);

    let (spawn, held) = (Rc::clone(&executor), Rc::clone(&hold));
    executor.spawn(Box::pin(async move {
        drop(held.exclusive(&*spawn, async {}).await);
    }));
    let seen = held_each_turn(&executor, &hold, 5);
    executor.run();
    assert_eq!(*seen.borrow(), [true, true, true, false, false]);
}

#[test]
fn a_work_that_waited_holds_its_participant_two_turns_after_it_ended() {
    let executor = Rc::new(Executor::default());
    let hold = Rc::new(Hold::default());
    let (log, gate) = (Log::default(), Gate::default());
    let (spawn, held, step, opened) = (
        Rc::clone(&executor),
        Rc::clone(&hold),
        log.clone(),
        gate.clone(),
    );
    executor.finish(async move {
        held.try_exclusive(&*spawn, gated(step, opened, "step", "stepped"))
            .await
            .expect("not held yet");
    });
    gate.open();
    let seen = held_each_turn(&executor, &hold, 4);
    executor.run();
    assert_eq!(log.taken(), ["step", "stepped"]);
    assert_eq!(*seen.borrow(), [true, true, false, false]);
}

#[test]
fn a_waiter_goes_on_two_turns_after_its_participant_was_let_go() {
    // It is woken in the turn the participant is let go, and `await
    // (running ?? acting).catch(...)` goes on a turn after that.
    let executor = Rc::new(Executor::default());
    let hold = Rc::new(Hold::default());
    let (log, gate) = (Log::default(), Gate::default());
    let (spawn, held, step, opened) = (
        Rc::clone(&executor),
        Rc::clone(&hold),
        log.clone(),
        gate.clone(),
    );
    executor.finish(async move {
        held.try_exclusive(&*spawn, gated(step, opened, "step", "stepped"))
            .await
            .expect("not held yet");
    });
    let (spawn, held, operation) = (Rc::clone(&executor), Rc::clone(&hold), log.clone());
    executor.spawn(Box::pin(async move {
        held.exclusive(&*spawn, async move { operation.push("operation") })
            .await
            .await;
    }));
    executor.run();
    gate.open();
    // A tick of work of its own at each turn, to count them by.
    let ticking = log.clone();
    executor.spawn(Box::pin(async move {
        for turn in ["0", "1", "2", "3", "4", "5", "6", "7"] {
            ticking.push(turn);
            next_turn().await;
        }
    }));
    executor.run();
    assert_eq!(
        log.taken(),
        [
            "step",
            "stepped",
            "0",
            "1",
            "2",
            "3",
            "operation",
            "4",
            "5",
            "6",
            "7"
        ],
        "let go at the second turn, woken, and the operation goes on two turns after"
    );
}

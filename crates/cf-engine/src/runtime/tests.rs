//! The rules the engine's work runs by, as `src/core/dispatcher.js` keeps
//! them, held on the kit's executor and on tokio's `LocalSet`.

use std::cell::RefCell;
use std::rc::Rc;

use super::*;
use crate::testing::{Executor, Gate};

/// What a test's pieces of work did, in order.
#[derive(Clone, Default)]
struct Log(Rc<RefCell<Vec<&'static str>>>);

impl Log {
    fn push(&self, what: &'static str) {
        self.0.borrow_mut().push(what);
    }

    fn taken(&self) -> Vec<&'static str> {
        std::mem::take(&mut *self.0.borrow_mut())
    }
}

/// A piece of work that says it started, waits on `gate`, and says it ended.
async fn gated(log: Log, gate: Gate, start: &'static str, end: &'static str) {
    log.push(start);
    gate.wait().await;
    log.push(end);
}

#[test]
fn a_work_does_what_comes_before_its_first_wait_where_it_is_begun() {
    let executor = Rc::new(Executor::default());
    let (log, gate) = (Log::default(), Gate::default());
    let spawn = Rc::clone(&executor);
    let (inner, opened) = (log.clone(), gate.clone());
    let kept = Rc::new(RefCell::new(None));
    let keep = Rc::clone(&kept);
    executor.finish(async move {
        let begun = begin(&*spawn, gated(inner.clone(), opened, "start", "end")).await;
        // Before the work's caller goes on, as JavaScript ran it.
        inner.push("caller");
        *keep.borrow_mut() = Some(begun);
    });
    assert_eq!(log.taken(), ["start", "caller"]);
    let begun = kept.borrow_mut().take().expect("begun");
    assert!(!begun.ended());
    gate.open();
    executor.run();
    assert_eq!(log.taken(), ["end"]);
    assert!(begun.ended());
}

#[test]
fn a_work_that_ends_where_it_is_begun_holds_nothing_after() {
    let executor = Rc::new(Executor::default());
    let hold = Rc::new(Hold::default());
    let (spawn, held) = (Rc::clone(&executor), Rc::clone(&hold));
    let answer = executor.finish(async move {
        let begun = held.exclusive(&*spawn, async { 7 }, false).await;
        assert!(!held.held(), "nothing holds it once its work ended");
        begun.expect("not held").await
    });
    assert_eq!(answer, Some(7));
    assert_eq!(executor.waiting(), 0);
}

#[test]
fn a_pass_moves_on_from_a_participant_held_and_a_human_operation_waits_its_turn() {
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
        held.exclusive(&*spawn, gated(step, opened, "step", "stepped"), false)
            .await
            .expect("not held yet");
    });
    assert!(hold.held());
    let (spawn, held) = (Rc::clone(&executor), Rc::clone(&hold));
    let skipped =
        executor.finish(async move { held.exclusive(&*spawn, async {}, false).await.is_none() });
    assert_eq!(skipped, Some(true), "a pass moves on");
    let (spawn, held, operation) = (Rc::clone(&executor), Rc::clone(&hold), log.clone());
    executor.spawn(Box::pin(async move {
        let begun = held
            .exclusive(&*spawn, async move { operation.push("operation") }, true)
            .await;
        begun.expect("waited").await;
    }));
    executor.run();
    assert_eq!(log.taken(), ["step"], "the operation waits");
    gate.open();
    executor.run();
    assert_eq!(log.taken(), ["stepped", "operation"]);
    assert!(!hold.held());
}

#[test]
fn waiters_are_served_first_come_and_nothing_takes_a_participant_handed_on() {
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
        held.exclusive(&*spawn, gated(step, opened, "step", "stepped"), false)
            .await
            .expect("not held yet");
    });
    for name in ["first", "second"] {
        let (spawn, held, operation) = (Rc::clone(&executor), Rc::clone(&hold), log.clone());
        executor.spawn(Box::pin(async move {
            let begun = held
                .exclusive(&*spawn, async move { operation.push(name) }, true)
                .await;
            begun.expect("waited").await;
        }));
    }
    executor.run();
    // The step ends: the participant is handed to the first waiter before
    // it runs, and a pass that comes meanwhile moves on.
    gate.open();
    let (spawn, held) = (Rc::clone(&executor), Rc::clone(&hold));
    executor.spawn(Box::pin(async move {
        assert!(held.exclusive(&*spawn, async {}, false).await.is_none());
    }));
    executor.run();
    assert_eq!(log.taken(), ["step", "stepped", "first", "second"]);
    assert!(!hold.held());
}

#[test]
fn a_launch_going_on_apart_holds_its_participant_until_it_ends() {
    let executor = Rc::new(Executor::default());
    let hold = Rc::new(Hold::default());
    let (log, gate) = (Log::default(), Gate::default());
    let (spawn, held, launch, opened) = (
        Rc::clone(&executor),
        Rc::clone(&hold),
        log.clone(),
        gate.clone(),
    );
    executor.finish(async move {
        // A step starts a launch and ends; the launch goes on.
        let step = {
            let (spawn, held) = (Rc::clone(&spawn), Rc::clone(&held));
            async move {
                held.act(&*spawn, gated(launch, opened, "launch", "launched"))
                    .await;
            }
        };
        held.exclusive(&*spawn, step, false).await.expect("free");
    });
    assert!(hold.held(), "the launch holds its participant");
    let (spawn, held, operation) = (Rc::clone(&executor), Rc::clone(&hold), log.clone());
    executor.spawn(Box::pin(async move {
        held.exclusive(&*spawn, async move { operation.push("operation") }, true)
            .await
            .expect("waited")
            .await;
    }));
    executor.run();
    assert_eq!(log.taken(), ["launch"]);
    gate.open();
    executor.run();
    assert_eq!(log.taken(), ["launched", "operation"]);
}

#[test]
fn work_that_fails_before_its_first_wait_lets_go_of_its_participant() {
    let executor = Rc::new(Executor::default());
    let hold = Rc::new(Hold::default());
    let (spawn, held) = (Rc::clone(&executor), Rc::clone(&hold));
    let failed = executor.finish(async move {
        let begun = held
            .exclusive(&*spawn, async { Err::<(), _>("refused") }, false)
            .await
            .expect("free");
        begun.await
    });
    assert_eq!(failed, Some(Err("refused")));
    assert!(!hold.held());
}

#[test]
fn a_work_whose_answer_nobody_waits_for_goes_on() {
    let executor = Rc::new(Executor::default());
    let (log, gate) = (Log::default(), Gate::default());
    let (spawn, inner, opened) = (Rc::clone(&executor), log.clone(), gate.clone());
    executor.finish(async move {
        drop(begin(&*spawn, gated(inner, opened, "start", "end")).await);
    });
    gate.open();
    executor.run();
    assert_eq!(log.taken(), ["start", "end"]);
}

#[test]
fn a_waiter_that_goes_away_leaves_its_place_and_one_handed_the_participant_passes_it_on() {
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
        held.exclusive(&*spawn, gated(step, opened, "step", "stepped"), false)
            .await
            .expect("not held yet");
    });
    // A waiter that goes away before its turn leaves the queue.
    {
        let waiting = Waiting::new(&hold);
        drop(waiting);
    }
    // One handed the participant that goes away passes it on.
    let given = Waiting::new(&hold);
    let (spawn, held, operation) = (Rc::clone(&executor), Rc::clone(&hold), log.clone());
    executor.spawn(Box::pin(async move {
        held.exclusive(&*spawn, async move { operation.push("operation") }, true)
            .await
            .expect("waited")
            .await;
    }));
    executor.run();
    gate.open();
    executor.run();
    assert!(hold.handed.get(), "handed to the waiter that went away");
    drop(given);
    executor.run();
    assert_eq!(log.taken(), ["step", "stepped", "operation"]);
    assert!(!hold.held());
}

#[test]
#[should_panic(expected = "a borrow twice")]
fn a_panic_in_work_going_on_apart_fails_the_test() {
    let executor = Rc::new(Executor::default());
    let gate = Gate::default();
    let (spawn, opened) = (Rc::clone(&executor), gate.clone());
    executor.finish(async move {
        drop(
            begin(&*spawn, async move {
                opened.wait().await;
                panic!("a borrow twice");
            })
            .await,
        );
    });
    gate.open();
    executor.run();
}

#[test]
fn the_same_rules_hold_on_tokios_local_set() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("a runtime");
    let local = tokio::task::LocalSet::new();
    let log = local.block_on(&runtime, async {
        let (log, hold) = (Log::default(), Rc::new(Hold::default()));
        let gate = Gate::default();
        let begun = hold
            .exclusive(
                &LocalSpawn,
                gated(log.clone(), gate.clone(), "step", "stepped"),
                false,
            )
            .await
            .expect("free");
        log.push("caller");
        let waiter = {
            let (hold, log) = (Rc::clone(&hold), log.clone());
            tokio::task::spawn_local(async move {
                hold.exclusive(&LocalSpawn, async move { log.push("operation") }, true)
                    .await
                    .expect("waited")
                    .await;
            })
        };
        tokio::task::yield_now().await;
        assert!(hold.exclusive(&LocalSpawn, async {}, false).await.is_none());
        gate.open();
        begun.await;
        waiter.await.expect("the waiter");
        log.taken()
    });
    assert_eq!(log, ["step", "caller", "stepped", "operation"]);
}

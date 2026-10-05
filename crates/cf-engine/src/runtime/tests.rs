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
        let begun = held.exclusive(&*spawn, async { 7 }).await;
        assert!(!held.held(), "nothing holds it once its work ended");
        begun.await
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
        held.try_exclusive(&*spawn, gated(step, opened, "step", "stepped"))
            .await
            .expect("not held yet");
    });
    assert!(hold.held());
    let (spawn, held) = (Rc::clone(&executor), Rc::clone(&hold));
    let skipped =
        executor.finish(async move { held.try_exclusive(&*spawn, async {}).await.is_none() });
    assert_eq!(skipped, Some(true), "a pass moves on");
    let (spawn, held, operation) = (Rc::clone(&executor), Rc::clone(&hold), log.clone());
    executor.spawn(Box::pin(async move {
        held.exclusive(&*spawn, async move { operation.push("operation") })
            .await
            .await;
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
        held.try_exclusive(&*spawn, gated(step, opened, "step", "stepped"))
            .await
            .expect("not held yet");
    });
    for name in ["first", "second"] {
        let (spawn, held, operation) = (Rc::clone(&executor), Rc::clone(&hold), log.clone());
        executor.spawn(Box::pin(async move {
            held.exclusive(&*spawn, async move { operation.push(name) })
                .await
                .await;
        }));
    }
    executor.run();
    // The step ends: the participant is handed to the first waiter before
    // it runs, and a pass that comes meanwhile moves on.
    gate.open();
    let (spawn, held) = (Rc::clone(&executor), Rc::clone(&hold));
    executor.spawn(Box::pin(async move {
        assert!(held.try_exclusive(&*spawn, async {}).await.is_none());
    }));
    executor.run();
    assert_eq!(log.taken(), ["step", "stepped", "first", "second"]);
    assert!(!hold.held());
}

#[test]
fn operations_on_many_participants_each_take_their_place_at_once() {
    // A Close of a project whose chief is held and whose member is not: the
    // member's window goes now, the chief's once its step is over.
    let executor = Rc::new(Executor::default());
    let (chief, member) = (Rc::new(Hold::default()), Rc::new(Hold::default()));
    let (log, gate) = (Log::default(), Gate::default());
    let (spawn, held, step, opened) = (
        Rc::clone(&executor),
        Rc::clone(&chief),
        log.clone(),
        gate.clone(),
    );
    executor.finish(async move {
        held.try_exclusive(&*spawn, gated(step, opened, "step", "stepped"))
            .await
            .expect("not held yet");
    });
    let (spawn, holds, closing) = (
        Rc::clone(&executor),
        [Rc::clone(&chief), Rc::clone(&member)],
        log.clone(),
    );
    executor.spawn(Box::pin(async move {
        let mut begun = Vec::new();
        for (hold, name) in holds.iter().zip(["chief closed", "member closed"]) {
            let closed = closing.clone();
            begun.push(
                hold.exclusive(&*spawn, async move { closed.push(name) })
                    .await,
            );
        }
        closing.push("all asked");
        for closed in begun {
            closed.await;
        }
        closing.push("all closed");
    }));
    executor.run();
    assert_eq!(log.taken(), ["step", "member closed", "all asked"]);
    gate.open();
    executor.run();
    assert_eq!(log.taken(), ["stepped", "chief closed", "all closed"]);
}

#[test]
fn work_awaited_together_answers_its_first_failure_at_once_and_the_rest_goes_on() {
    // More than the 30 above which `try_join_all` keeps each answer behind
    // the ones before it.
    let executor = Rc::new(Executor::default());
    let (log, gate) = (Log::default(), Gate::default());
    let (spawn, held, opened) = (Rc::clone(&executor), log.clone(), gate.clone());
    let answered = executor.finish(async move {
        let mut begun = Vec::new();
        let first = {
            let (held, opened) = (held.clone(), opened.clone());
            async move {
                gated(held, opened, "first", "first went on").await;
                Ok(())
            }
        };
        begun.push(begin(&*spawn, first).await);
        for _ in 0..38 {
            begun.push(begin(&*spawn, async { Ok(()) }).await);
        }
        let failing = async move {
            next_turn().await;
            Err("refused")
        };
        begun.push(begin(&*spawn, failing).await);
        all(begun).await
    });
    assert_eq!(
        answered,
        Some(Err("refused")),
        "answered while the first still waits"
    );
    assert_eq!(log.taken(), ["first"]);
    gate.open();
    executor.run();
    assert_eq!(log.taken(), ["first went on"]);
}

#[test]
fn work_awaited_together_answers_in_the_order_begun() {
    let executor = Rc::new(Executor::default());
    let gate = Gate::default();
    let (spawn, opened) = (Rc::clone(&executor), gate.clone());
    let answered = Rc::new(RefCell::new(None));
    let keep = Rc::clone(&answered);
    executor.spawn(Box::pin(async move {
        let late = async move {
            opened.wait().await;
            Ok::<_, ()>("late")
        };
        let begun = vec![
            begin(&*spawn, late).await,
            begin(&*spawn, async { Ok("now") }).await,
        ];
        *keep.borrow_mut() = Some(all(begun).await);
    }));
    executor.run();
    assert!(answered.borrow().is_none());
    gate.open();
    executor.run();
    assert_eq!(answered.borrow_mut().take(), Some(Ok(vec!["late", "now"])));
}

#[test]
fn an_operation_handed_its_participant_waits_its_turns_as_javascript_did() {
    // What the operation begins before it waits goes before what it does after.
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
        let begins = Rc::clone(&spawn);
        let work = async move {
            operation.push("start");
            let sibling = operation.clone();
            Spawn::spawn(&*begins, Box::pin(async move { sibling.push("sibling") }));
            next_turn().await;
            operation.push("end");
        };
        held.exclusive(&*spawn, work).await.await;
    }));
    executor.run();
    assert_eq!(log.taken(), ["step"]);
    gate.open();
    executor.run();
    assert_eq!(log.taken(), ["stepped", "start", "sibling", "end"]);
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
        held.try_exclusive(&*spawn, step).await.expect("free");
    });
    assert!(hold.held(), "the launch holds its participant");
    let (spawn, held, operation) = (Rc::clone(&executor), Rc::clone(&hold), log.clone());
    executor.spawn(Box::pin(async move {
        held.exclusive(&*spawn, async move { operation.push("operation") })
            .await
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
            .try_exclusive(&*spawn, async { Err::<(), _>("refused") })
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
        held.try_exclusive(&*spawn, gated(step, opened, "step", "stepped"))
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
        held.exclusive(&*spawn, async move { operation.push("operation") })
            .await
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
fn an_answer_reaches_its_caller_a_turn_after_it_was_made_as_an_awaited_async_function_does() {
    // JavaScript's `await` of a call that returned at once still waited a
    // turn: what was woken before the call returned goes ahead of its caller.
    let executor = Rc::new(Executor::default());
    let log = Log::default();
    let (spawn, called) = (Rc::clone(&executor), log.clone());
    executor.finish(async move {
        let other = called.clone();
        spawn.spawn(Box::pin(async move { other.push("other") }));
        called.push("call");
        let answer = returning(async { 7 }).await;
        called.push("answered");
        assert_eq!(answer, 7);
    });
    assert_eq!(log.taken(), ["call", "other", "answered"]);

    // Without it the caller goes on at once.
    let executor = Rc::new(Executor::default());
    let (spawn, called) = (Rc::clone(&executor), log.clone());
    executor.finish(async move {
        let other = called.clone();
        spawn.spawn(Box::pin(async move { other.push("other") }));
        let _ = async { 7 }.await;
        called.push("answered");
    });
    assert_eq!(log.taken(), ["answered", "other"]);

    // And on tokio's `LocalSet`, as the daemon runs the engine.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("a runtime");
    let local = tokio::task::LocalSet::new();
    let order = local.block_on(&runtime, async {
        let other = log.clone();
        let spawned = tokio::task::spawn_local(async move { other.push("other") });
        returning(async {}).await;
        log.push("answered");
        spawned.await.expect("the other work");
        log.taken()
    });
    assert_eq!(order, ["other", "answered"]);
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
            .try_exclusive(
                &LocalSpawn,
                gated(log.clone(), gate.clone(), "step", "stepped"),
            )
            .await
            .expect("free");
        log.push("caller");
        let waiter = {
            let (hold, log) = (Rc::clone(&hold), log.clone());
            tokio::task::spawn_local(async move {
                hold.exclusive(&LocalSpawn, async move { log.push("operation") })
                    .await
                    .await;
            })
        };
        tokio::task::yield_now().await;
        assert!(hold.try_exclusive(&LocalSpawn, async {}).await.is_none());
        gate.open();
        begun.await;
        waiter.await.expect("the waiter");
        log.taken()
    });
    assert_eq!(log, ["step", "caller", "stepped", "operation"]);
}

#[test]
fn a_turn_lets_the_work_woken_before_it_go_first() {
    let executor = Executor::default();
    let log = Log::default();
    let (waiting, going) = (log.clone(), log.clone());
    executor.spawn(Box::pin(async move {
        next_turn().await;
        waiting.push("after a turn");
    }));
    executor.spawn(Box::pin(async move { going.push("meanwhile") }));
    executor.run();
    assert_eq!(log.taken(), ["meanwhile", "after a turn"]);
}

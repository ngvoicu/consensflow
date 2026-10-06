//! What the executor promises the daemon (its type's documentation): the
//! order work runs in, the drain at each boundary, and the driver on a real
//! tokio `LocalSet`, where tokio runs the tasks that are not the executor's.

use std::cell::Cell;
use std::future::poll_fn;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Poll, Waker};
use std::time::Duration;

use super::*;
use crate::runtime::next_turn;
use crate::runtime::stage::{Log, Stage};
use crate::testing::Gate;

mod budget;

#[test]
fn a_drain_runs_what_is_woken_and_what_that_wakes_and_says_whether_anything_ran() {
    let executor = Executor::strict();
    assert!(!executor.drain(), "nothing was woken");
    let log = Log::default();
    let (woken, meanwhile) = (log.clone(), log.clone());
    executor.spawn(Box::pin(async move {
        woken.push("a");
        next_turn().await;
        woken.push("a again");
    }));
    executor.spawn(Box::pin(async move { meanwhile.push("b") }));
    assert!(executor.drain());
    assert_eq!(log.taken(), ["a", "b", "a again"]);
    assert_eq!(executor.waiting(), 0);
    assert!(!executor.drain(), "and then nothing is");
}

#[test]
fn work_woken_twice_before_it_runs_runs_once_and_no_stale_wake_cuts_a_turn_short() {
    let executor = Executor::strict();
    let polls = Rc::new(Cell::new(0));
    let waker: Rc<std::cell::RefCell<Option<Waker>>> = Rc::default();
    let (counted, kept) = (Rc::clone(&polls), Rc::clone(&waker));
    let release = Gate::default();
    let waiting = release.clone();
    executor.spawn(Box::pin(async move {
        poll_fn(|context| {
            counted.set(counted.get() + 1);
            *kept.borrow_mut() = Some(context.waker().clone());
            if waiting.is_open() {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        })
        .await;
    }));
    executor.drain();
    assert_eq!(polls.get(), 1);
    release.open();
    let woken = waker.borrow().clone().expect("the work's waker");
    woken.wake_by_ref();
    woken.wake_by_ref();
    woken.wake();
    executor.drain();
    assert_eq!(polls.get(), 2, "woken three times, polled once more");

    // A wake the work does not wait on, which a registration it left behind
    // makes (`begin` polls work with its caller's waker first), finds it
    // already queued for its turn and does not end the turn early.
    let log = Log::default();
    let stale: Rc<std::cell::RefCell<Option<Waker>>> = Rc::default();
    let (turning, kept) = (log.clone(), Rc::clone(&stale));
    executor.spawn(Box::pin(async move {
        poll_fn(|context| {
            *kept.borrow_mut() = Some(context.waker().clone());
            Poll::Ready(())
        })
        .await;
        next_turn().await;
        next_turn().await;
        turning.push("two turns");
    }));
    let (waking, kept) = (log.clone(), Rc::clone(&stale));
    executor.spawn(Box::pin(async move {
        if let Some(waker) = kept.borrow().as_ref() {
            waker.wake_by_ref();
        }
        waking.push("a stale wake");
    }));
    let ticking = log.clone();
    executor.spawn(Box::pin(async move {
        for turn in ["tick 0", "tick 1", "tick 2", "tick 3"] {
            ticking.push(turn);
            next_turn().await;
        }
    }));
    executor.drain();
    assert_eq!(
        log.taken(),
        [
            "a stale wake",
            "tick 0",
            "tick 1",
            "two turns",
            "tick 2",
            "tick 3"
        ]
    );
}

#[test]
fn a_wake_of_work_that_ended_does_nothing() {
    let executor = Executor::strict();
    let waker: Rc<std::cell::RefCell<Option<Waker>>> = Rc::default();
    let kept = Rc::clone(&waker);
    executor.spawn(Box::pin(async move {
        poll_fn(|context| {
            *kept.borrow_mut() = Some(context.waker().clone());
            Poll::Ready(())
        })
        .await;
    }));
    executor.drain();
    assert_eq!(executor.waiting(), 0);
    waker.borrow().as_ref().expect("a waker").wake_by_ref();
    assert!(!executor.drain(), "there is nothing to run");
}

#[test]
fn a_drain_called_from_inside_work_does_nothing_and_the_drain_that_runs_it_goes_on() {
    let executor = Rc::new(Executor::strict());
    let log = Log::default();
    let (inner, called) = (Rc::clone(&executor), log.clone());
    executor.spawn(Box::pin(async move {
        called.push(if inner.drain() { "ran" } else { "ran nothing" });
    }));
    let later = log.clone();
    executor.spawn(Box::pin(async move { later.push("the next") }));
    assert!(executor.drain());
    assert_eq!(log.taken(), ["ran nothing", "the next"]);
}

#[test]
fn a_panic_ends_its_work_and_nothing_else_unless_the_executor_is_strict() {
    let executor = Executor::new();
    let log = Log::default();
    executor.spawn(Box::pin(async move { panic!("a borrow twice") }));
    let after = log.clone();
    executor.spawn(Box::pin(async move { after.push("the next") }));
    assert!(executor.drain());
    assert_eq!(log.taken(), ["the next"]);
    assert_eq!(executor.waiting(), 0, "the work that panicked is gone");

    let strict = Executor::strict();
    strict.spawn(Box::pin(async move { panic!("a borrow twice") }));
    let result = std::panic::catch_unwind(AssertUnwindSafe(|| strict.drain()));
    assert!(result.is_err(), "it comes out of the drain");
    // The drain that panicked let go of its place: the next one runs.
    let later = log.clone();
    strict.spawn(Box::pin(async move { later.push("again") }));
    assert!(strict.drain());
    assert_eq!(log.taken(), ["again"]);
}

#[test]
fn a_chain_of_turns_ends_before_a_task_of_tokios_own_woken_meanwhile_runs() {
    let stage = Stage::local_set();
    let (log, gate) = (Log::default(), Gate::default());
    // A task of tokio's own that waits, as the bridge's reader waits on its input.
    let (heard, woken) = (log.clone(), gate.clone());
    drop(stage.local().set.spawn_local(async move {
        woken.wait().await;
        heard.push("the other task");
    }));
    stage.run();
    // Work on the executor wakes it, and goes on through five more turns.
    let (chain, opens) = (log.clone(), gate.clone());
    stage.executor.spawn(Box::pin(async move {
        opens.open();
        for turn in ["0", "1", "2", "3", "4"] {
            chain.push(turn);
            next_turn().await;
        }
    }));
    stage.run();
    assert_eq!(
        log.taken(),
        ["0", "1", "2", "3", "4", "the other task"],
        "the other task waits for the chain's last turn, as Node's next callback did"
    );
}

#[test]
fn work_woken_from_outside_a_drain_is_run_by_the_driver_with_no_call_of_drain() {
    let stage = Stage::local_set();
    let (log, gate) = (Log::default(), Gate::default());
    let (started, opened) = (log.clone(), gate.clone());
    stage.executor.spawn(Box::pin(async move {
        started.push("waits");
        opened.wait().await;
        started.push("goes on");
        next_turn().await;
        started.push("and on");
    }));
    stage.run();
    assert_eq!(log.taken(), ["waits"]);
    // An answer arrives while nobody drains: the gate stands for the bridge's.
    gate.open();
    stage.run();
    assert_eq!(log.taken(), ["goes on", "and on"]);
}

#[test]
fn a_wake_from_another_thread_is_run_by_the_driver() {
    let stage = Stage::local_set();
    let (waker, ready) = (
        Arc::new(Mutex::new(None::<Waker>)),
        Arc::new(AtomicBool::new(false)),
    );
    let (kept, go) = (Arc::clone(&waker), Arc::clone(&ready));
    let log = Log::default();
    let ended = log.clone();
    stage.executor.spawn(Box::pin(async move {
        poll_fn(|context| {
            *kept.lock().expect("the slot") = Some(context.waker().clone());
            if go.load(Ordering::SeqCst) {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        })
        .await;
        ended.push("woken from another thread");
    }));
    stage.run();
    assert!(log.taken().is_empty());
    // What a blocking task's result does: wakes from a thread that is not the engine's.
    let (kept, go) = (Arc::clone(&waker), Arc::clone(&ready));
    std::thread::spawn(move || {
        go.store(true, Ordering::SeqCst);
        if let Some(waker) = kept.lock().expect("the slot").take() {
            waker.wake();
        }
    })
    .join()
    .expect("the thread");
    stage.run();
    assert_eq!(log.taken(), ["woken from another thread"]);
}

#[test]
fn a_timer_wakes_work_and_its_chain_ends_before_the_next_task_tokio_runs() {
    let stage = Stage::local_set();
    let log = Log::default();
    let slept = log.clone();
    stage.executor.spawn(Box::pin(async move {
        tokio::time::sleep(Duration::from_millis(5)).await;
        slept.push("slept");
        next_turn().await;
        slept.push("a turn after");
    }));
    // A task of tokio's own, whose timer is due at the same moment.
    let other = log.clone();
    drop(stage.local().set.spawn_local(async move {
        tokio::time::sleep(Duration::from_millis(5)).await;
        other.push("the other task");
    }));
    // Both timers are due 5 ms on; a loaded machine runs them later (a CI
    // runner once ran neither within 50 ms), so the wait is for the three
    // entries, not for a time.
    let waited = log.clone();
    stage.local().block_on(async move {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        while waited.count() < 3 && tokio::time::Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    });
    // Whichever of the two tokio woke first, the chain was not broken.
    let heard = log.taken();
    let slept = heard.iter().position(|what| *what == "slept");
    assert_eq!(heard.len(), 3, "{heard:?}");
    assert_eq!(
        slept.map(|at| heard[at + 1]),
        Some("a turn after"),
        "its turn followed at once: {heard:?}"
    );
}

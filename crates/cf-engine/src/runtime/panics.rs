//! Work that panics, which the executor ends and nothing else: what that
//! leaves undone (a participant held for good, a waiter that waits for ever)
//! and what the runtime does about it. A panic is what a throw was, which
//! `finally` let go of the participant for and a rejected promise ended its
//! waiter for. The words of each panic are read on an executor that lets it
//! out of its drain; what is left of the work is read on one that does not,
//! as the daemon's does, on both stages.

use std::cell::RefCell;
use std::rc::Rc;

use super::stage::{gated, Log, Stage};
use super::*;
use crate::testing::Gate;

/// What a panic that unwound out of `work` said.
fn panicked(work: impl FnOnce()) -> String {
    let panic = std::panic::catch_unwind(AssertUnwindSafe(work)).expect_err("it panics");
    panic_words(panic.as_ref())
}

/// What the work the executor runs next says as it panics.
fn runs_to_a_panic(stage: &Stage) -> String {
    panicked(|| stage.run())
}

#[test]
fn work_that_panics_after_a_wait_lets_go_of_its_participant_and_the_next_waiter_takes_its_turn() {
    let stage = Stage::by_hand();
    let hold = Rc::new(Hold::default());
    let (log, gate) = (Log::default(), Gate::default());
    let (spawn, held, opened) = (Rc::clone(&stage.executor), Rc::clone(&hold), gate.clone());
    // A step holds the participant, and panics once its gate opens.
    stage.finish(async move {
        held.try_exclusive(&*spawn, async move {
            opened.wait().await;
            panic!("a step failed");
        })
        .await
        .expect("free");
    });
    assert!(hold.held());
    // A human's operation waits its turn for the participant.
    let (spawn, held, operation) = (Rc::clone(&stage.executor), Rc::clone(&hold), log.clone());
    stage.executor.spawn(Box::pin(async move {
        held.exclusive(&*spawn, async move { operation.push("operation") })
            .await
            .await;
    }));
    stage.run();
    assert!(log.taken().is_empty(), "it waits for the step");
    gate.open();
    assert_eq!(runs_to_a_panic(&stage), "a step failed");
    // The panic let go of the participant, and what was woken went on.
    stage.run();
    assert_eq!(log.taken(), ["operation"]);
    assert!(
        !hold.held(),
        "held by nothing: the work that held it is gone"
    );
}

#[test]
fn work_that_panics_before_its_first_wait_unwinds_to_its_caller_and_holds_nothing() {
    let stage = Stage::by_hand();
    let hold = Rc::new(Hold::default());
    let (spawn, held) = (Rc::clone(&stage.executor), Rc::clone(&hold));
    let said = panicked(|| {
        stage.finish(async move {
            held.try_exclusive(&*spawn, async { panic!("a first poll") })
                .await;
        });
    });
    assert_eq!(said, "a first poll");
    assert!(!hold.held());
    // Free for the next operation, which begins at once.
    let (spawn, held) = (Rc::clone(&stage.executor), Rc::clone(&hold));
    let answer = stage.finish(async move { held.exclusive(&*spawn, async { 5 }).await.await });
    assert_eq!(answer, Some(5));
}

#[test]
fn a_waiter_s_work_that_panics_in_its_turn_lets_go_of_the_participant_too() {
    let stage = Stage::by_hand();
    let hold = Rc::new(Hold::default());
    let (log, gate) = (Log::default(), Gate::default());
    let (spawn, held, step, opened) = (
        Rc::clone(&stage.executor),
        Rc::clone(&hold),
        log.clone(),
        gate.clone(),
    );
    stage.finish(async move {
        held.try_exclusive(&*spawn, gated(step, opened, "step", "stepped"))
            .await
            .expect("free");
    });
    // The first waiter's work panics when it is handed the participant; the second goes after.
    let (spawn, held) = (Rc::clone(&stage.executor), Rc::clone(&hold));
    stage.executor.spawn(Box::pin(async move {
        held.exclusive(&*spawn, async { panic!("the first operation failed") })
            .await
            .await;
    }));
    let (spawn, held, second) = (Rc::clone(&stage.executor), Rc::clone(&hold), log.clone());
    stage.executor.spawn(Box::pin(async move {
        held.exclusive(&*spawn, async move { second.push("second") })
            .await
            .await;
    }));
    stage.run();
    gate.open();
    assert_eq!(runs_to_a_panic(&stage), "the first operation failed");
    // The second took its turn; the first one's waiter, woken by the failure, ends with an error.
    assert_eq!(
        runs_to_a_panic(&stage),
        "the work waited for panicked: the first operation failed"
    );
    stage.run();
    assert_eq!(log.taken(), ["step", "stepped", "second"]);
    assert!(!hold.held());
}

#[test]
fn a_launch_going_on_apart_that_panics_lets_go_of_its_participant() {
    let stage = Stage::by_hand();
    let hold = Rc::new(Hold::default());
    let gate = Gate::default();
    let (spawn, held, opened) = (Rc::clone(&stage.executor), Rc::clone(&hold), gate.clone());
    stage.finish(async move {
        held.act(&*spawn, async move {
            opened.wait().await;
            panic!("a launch failed");
        })
        .await;
    });
    assert!(hold.held(), "held while it goes on");
    gate.open();
    assert_eq!(runs_to_a_panic(&stage), "a launch failed");
    assert!(!hold.held());
    assert!(!hold.acting.get());
}

#[test]
fn whoever_waits_for_work_that_panicked_ends_with_an_error_and_does_not_wait_for_ever() {
    let stage = Stage::by_hand();
    let (log, gate) = (Log::default(), Gate::default());
    let (spawn, opened) = (Rc::clone(&stage.executor), gate.clone());
    let kept = Rc::new(RefCell::new(None));
    let keep = Rc::clone(&kept);
    stage.finish(async move {
        let begun = begin(&*spawn, async move {
            opened.wait().await;
            panic!("the work failed");
        })
        .await;
        *keep.borrow_mut() = Some(begun);
    });
    let waiting = log.clone();
    stage.executor.spawn(Box::pin(async move {
        let begun: Begun<()> = kept.borrow_mut().take().expect("begun");
        begun.await;
        waiting.push("answered");
    }));
    stage.run();
    gate.open();
    // The work's own panic unwinds where the work runs...
    assert_eq!(runs_to_a_panic(&stage), "the work failed");
    // ...and the waiter, woken by it, ends with an error of its own.
    let said = runs_to_a_panic(&stage);
    assert_eq!(said, "the work waited for panicked: the work failed");
    assert!(
        log.taken().is_empty(),
        "it never took the work's answer for one"
    );
}

#[test]
fn a_waiter_that_comes_after_the_work_panicked_is_told_at_once() {
    let stage = Stage::by_hand();
    let gate = Gate::default();
    let (spawn, opened) = (Rc::clone(&stage.executor), gate.clone());
    let kept = Rc::new(RefCell::new(None));
    let keep = Rc::clone(&kept);
    stage.finish(async move {
        let begun = begin(&*spawn, async move {
            opened.wait().await;
            panic!("already gone");
        })
        .await;
        *keep.borrow_mut() = Some(begun);
    });
    gate.open();
    assert_eq!(runs_to_a_panic(&stage), "already gone");
    let begun: Begun<()> = kept.borrow_mut().take().expect("begun");
    let said = panicked(|| {
        stage.finish(begun);
    });
    assert_eq!(said, "the work waited for panicked: already gone");
}

#[test]
fn where_the_executor_ends_the_work_that_panicked_its_participant_goes_free_and_the_next_waiter_goes_on(
) {
    for stage in Stage::isolating() {
        let hold = Rc::new(Hold::default());
        let (log, gate) = (Log::default(), Gate::default());
        let (spawn, held, opened) = (Rc::clone(&stage.executor), Rc::clone(&hold), gate.clone());
        stage.finish(async move {
            held.try_exclusive(&*spawn, async move {
                opened.wait().await;
                panic!("a step failed");
            })
            .await
            .expect("free");
        });
        let (spawn, held, operation) = (Rc::clone(&stage.executor), Rc::clone(&hold), log.clone());
        stage.executor.spawn(Box::pin(async move {
            held.exclusive(&*spawn, async move { operation.push("operation") })
                .await
                .await;
        }));
        stage.run();
        assert!(hold.held());
        gate.open();
        stage.run();
        assert_eq!(log.taken(), ["operation"]);
        assert!(!hold.held());
        assert_eq!(stage.executor.waiting(), 0, "nothing is left waiting");
        // The participant is there for the next operation.
        let (spawn, held) = (Rc::clone(&stage.executor), Rc::clone(&hold));
        let answer = stage.finish(async move { held.exclusive(&*spawn, async { 9 }).await.await });
        assert_eq!(answer, Some(9));
    }
}

#[test]
fn where_the_executor_ends_the_work_that_panicked_whoever_waits_for_its_answer_ends_too() {
    for stage in Stage::isolating() {
        let (log, gate) = (Log::default(), Gate::default());
        let (spawn, opened) = (Rc::clone(&stage.executor), gate.clone());
        let waiting = log.clone();
        stage.finish(async move {
            let begun = begin(&*spawn, async move {
                opened.wait().await;
                panic!("the work failed");
            })
            .await;
            begun.await;
            waiting.push("answered");
        });
        assert_eq!(
            stage.executor.waiting(),
            2,
            "the work, and the one that waits"
        );
        gate.open();
        stage.run();
        assert_eq!(
            stage.executor.waiting(),
            0,
            "the waiter ended with its error, and did not wait for ever"
        );
        assert!(log.taken().is_empty());
    }
}

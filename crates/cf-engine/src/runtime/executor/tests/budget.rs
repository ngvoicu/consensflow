//! The engine's work runs outside tokio's cooperative budget
//! ([`crate::runtime::budget`]). A ready channel's answer costs the task that
//! polls it one of the 128 operations a poll of a tokio task is given, and
//! once they are spent the answer is not read, though it is there, and the
//! work's wake is deferred to tokio, behind the tasks queued ahead of it. So
//! the work must neither stop for another task's spent budget nor spend it:
//! on the driver, in a drain a task of tokio's own calls, and in the first part
//! of work begun.

use std::rc::Rc;
use std::task::{Context, Poll, Waker};

use tokio::sync::oneshot;
use tokio::task::coop::{has_budget_remaining, poll_proceed};

use crate::runtime::stage::{Log, Stage};
use crate::runtime::{begin, Spawn};
use crate::testing::Gate;

/// More answers than the budget of 128 operations a poll of a tokio task has.
const MORE_THAN_THE_BUDGET: usize = 300;

/// Spends what is left of the budget of the tokio task this is called in.
fn spend_the_budget() {
    let mut context = Context::from_waker(Waker::noop());
    for _ in 0..1_000 {
        match poll_proceed(&mut context) {
            Poll::Ready(spent) => spent.made_progress(),
            Poll::Pending => return,
        }
    }
    panic!("a budget that does not run out: this is not a task of tokio's");
}

/// What is left of the budget of the tokio task this is called in, counted
/// without spending it.
fn budget_left() -> usize {
    let mut context = Context::from_waker(Waker::noop());
    let mut taken = Vec::new();
    while taken.len() < 1_000 {
        match poll_proceed(&mut context) {
            Poll::Ready(unit) => taken.push(unit),
            Poll::Pending => break,
        }
    }
    let left = taken.len();
    // Each unit gives the budget back as it was when it was taken: the last
    // taken first, so that the first gives the original.
    while let Some(unit) = taken.pop() {
        drop(unit);
    }
    left
}

/// Work that takes `count` answers that are ready, one after the other, and
/// then says `end`: a channel's answer is ready as soon as it is sent.
async fn taking_ready_answers(count: usize, log: Log, end: &'static str) {
    for _ in 0..count {
        let (answer, answered) = oneshot::channel();
        answer.send(()).expect("the receiver is here");
        answered.await.expect("the answer was sent");
    }
    log.push(end);
}

#[test]
fn a_burst_of_ready_answers_is_taken_by_the_driver_in_one_drain_before_tokios_next_task() {
    let stage = Stage::local_set();
    let (log, gate) = (Log::default(), Gate::default());
    // A task of tokio's own, which the work wakes before its burst: it goes on
    // when the driver gives the thread back, and not before.
    let (heard, woken) = (log.clone(), gate.clone());
    drop(stage.local().set.spawn_local(async move {
        woken.wait().await;
        heard.push("the other task");
    }));
    stage.run();
    let (burst, opens) = (log.clone(), gate.clone());
    stage.executor.spawn(Box::pin(async move {
        opens.open();
        taking_ready_answers(MORE_THAN_THE_BUDGET, burst, "the burst").await;
    }));
    stage.run();
    assert_eq!(
        log.taken(),
        ["the burst", "the other task"],
        "the burst ended inside the drain, as Node's microtasks did"
    );
}

#[test]
fn a_drain_a_task_whose_budget_is_spent_calls_takes_the_answer_that_is_there() {
    let stage = Stage::local_set();
    let (log, executor) = (Log::default(), Rc::clone(&stage.executor));
    let said = log.clone();
    stage.local().block_on(async move {
        let (answer, answered) = oneshot::channel();
        answer.send(()).expect("the receiver is here");
        let took = said.clone();
        executor.spawn(Box::pin(async move {
            answered.await.expect("the answer was sent");
            took.push("took it");
        }));
        spend_the_budget();
        assert!(!has_budget_remaining(), "the task has nothing left");
        executor.drain();
        said.push("the drain returned");
    });
    assert_eq!(log.taken(), ["took it", "the drain returned"]);
}

#[test]
fn a_drain_takes_a_burst_of_ready_answers_and_spends_none_of_the_budget_of_the_task_that_calls_it()
{
    let stage = Stage::local_set();
    let (log, executor) = (Log::default(), Rc::clone(&stage.executor));
    let said = log.clone();
    stage.local().block_on(async move {
        let before = budget_left();
        assert!(
            (1..1_000).contains(&before),
            "the task has a budget to spend: {before}"
        );
        executor.spawn(Box::pin(taking_ready_answers(
            MORE_THAN_THE_BUDGET,
            said.clone(),
            "the burst",
        )));
        executor.drain();
        said.push("the drain returned");
        assert_eq!(budget_left(), before, "the work spent none of it");
    });
    assert_eq!(log.taken(), ["the burst", "the drain returned"]);
}

#[test]
fn work_begun_in_a_task_whose_budget_is_spent_takes_the_answer_that_is_there_in_its_first_part() {
    let stage = Stage::local_set();
    let executor = Rc::clone(&stage.executor);
    stage.local().block_on(async move {
        let (answer, answered) = oneshot::channel();
        answer.send(7).expect("the receiver is here");
        spend_the_budget();
        let begun = begin(&*executor, async move {
            answered.await.expect("the answer was sent")
        })
        .await;
        assert!(begun.ended(), "it did not wait for what was ready");
        assert_eq!(begun.await, 7);
        assert_eq!(executor.waiting(), 0, "and left nothing to the executor");
    });
}

#[test]
fn work_begun_takes_a_burst_of_ready_answers_in_its_first_part_and_spends_none_of_the_budget() {
    let stage = Stage::local_set();
    let (log, executor) = (Log::default(), Rc::clone(&stage.executor));
    let said = log.clone();
    stage.local().block_on(async move {
        let before = budget_left();
        let begun = begin(
            &*executor,
            taking_ready_answers(MORE_THAN_THE_BUDGET, said.clone(), "the burst"),
        )
        .await;
        said.push("begun");
        assert!(begun.ended());
        assert_eq!(budget_left(), before, "the work spent none of it");
    });
    assert_eq!(log.taken(), ["the burst", "begun"]);
}

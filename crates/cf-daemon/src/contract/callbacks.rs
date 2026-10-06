//! Timers and the records' answers: what the executor is given from outside.
//!
//! A timer that elapses and an answer a worker thread sends wake the work that
//! waits for them directly, from tokio's timers and from the thread, where no
//! drain is. In Node each was a callback of its own, and the microtasks it
//! began ran to their end before the next callback: of two that came at once,
//! the first one's chain was over before the second's began, and an answer
//! that came while microtasks ran waited for them. The daemon makes them
//! callbacks ([`DaemonSpawn::arrival`]); these tests give it two at once, a
//! timer with the exit of a window or a request, and an answer in the middle
//! of a drain (the exit's, the request's first part, another chain's), and
//! read the order of the chains, which is Node's when each is whole. The
//! engine's own chains on timers and answers are in [`super::looks`].

use std::rc::Rc;
use std::time::Duration;

use cf_engine::runtime::{next_turn, LocalWork};
use cf_harness::seams::Time;
use serde_json::{json, Value};

use super::client::{head, Client};
use super::rig::{Pieces, Transport};
use super::standing::{look, Standing};
use super::{assert_whole, chain, scene, settle, Order, TURNS};
use crate::api::answer::Answer;
use crate::api::context::Closing;
use crate::api::{Api, Handler};
use crate::host::watch_exits;
use crate::seams::DaemonTime;
use crate::testing::{worked, Worked};

#[tokio::test(start_paused = true)]
async fn two_timers_that_elapse_together_each_run_their_chain_whole() {
    scene(async {
        let Worked { spawn, .. } = worked();
        let time = Rc::new(DaemonTime::new(Rc::clone(&spawn)));
        let order = Order::default();
        for name in ["a", "b"] {
            let (order, time) = (Rc::clone(&order), Rc::clone(&time));
            spawn.apart("a chain failed", async move {
                time.sleep(Duration::from_millis(10)).await;
                chain(order, name, TURNS).await;
            });
        }
        // Both are armed. When the clock moves both elapse in the same moment,
        // and tokio wakes both before either is run.
        spawn.drain();
        tokio::time::advance(Duration::from_millis(10)).await;
        settle().await;
        assert_whole(&order, [("a", TURNS), ("b", TURNS)]);
    })
    .await;
}

#[tokio::test]
async fn a_timer_and_an_exit_that_are_ready_in_one_poll_each_run_their_chain_whole() {
    scene(async {
        let mut pieces = Pieces::new(Transport::Socket).await;
        let order = Order::default();
        // The exit's handler begins a chain of the engine's work, which the
        // reader drains after its read; the timer's work is a chain of its own.
        let exited = Rc::clone(&order);
        drop(watch_exits(
            &pieces.bridge,
            Rc::clone(&pieces.spawn),
            move |_| {
                let rest: LocalWork = Box::pin(chain(Rc::clone(&exited), "exit", TURNS));
                Some(rest)
            },
        ));
        let time = Rc::new(DaemonTime::new(Rc::clone(&pieces.spawn)));
        let slept = Rc::clone(&order);
        pieces.spawn.apart("a chain failed", async move {
            time.sleep(Duration::from_millis(30)).await;
            chain(slept, "timer", TURNS).await;
        });
        settle().await;
        // The exit is written to the socket, and the thread is held until the
        // timer has elapsed too: when tokio next looks, the socket is ready
        // and the timer is due in the same poll, the reader's wake first, as
        // tokio dispatches its sockets before it fires its timers.
        let exit = pieces
            .host
            .exit(&json!({ "id": "p1-zeus", "generation": 1 }));
        pieces.host.write(&exit).await;
        std::thread::sleep(Duration::from_millis(100));
        settle().await;
        assert_whole(&order, [("exit", TURNS), ("timer", TURNS)]);
    })
    .await;
}

#[tokio::test]
async fn a_timer_and_a_request_that_are_ready_in_one_poll_each_run_their_chain_whole() {
    scene(async {
        let pieces = Pieces::new(Transport::Memory).await;
        let order = Order::default();
        // The request's handler is a chain from its first part, which the
        // connection's task begins and drains; the timer's work is another.
        let ran = Rc::clone(&order);
        let handler: Rc<Handler> = Rc::new(move |_| {
            let order = Rc::clone(&ran);
            Box::pin(async move {
                chain(order, "request", TURNS).await;
                Ok(Answer::ok(Value::Null))
            })
        });
        let api = Api::start(handler, Closing::new(), Rc::clone(&pieces.spawn))
            .await
            .expect("the front listens");
        let time = Rc::new(DaemonTime::new(Rc::clone(&pieces.spawn)));
        let slept = Rc::clone(&order);
        pieces.spawn.apart("a chain failed", async move {
            time.sleep(Duration::from_millis(30)).await;
            chain(slept, "timer", TURNS).await;
        });
        pieces.spawn.drain();
        let mut client = Client::connect(&api).await;
        settle().await;
        // The request is written, and the thread is held until the timer has
        // elapsed too: the connection's wake and the timer's come in one poll,
        // the connection's first.
        client.send(head("/x", 0).as_bytes()).await;
        std::thread::sleep(Duration::from_millis(100));
        settle().await;
        assert_whole(&order, [("timer", TURNS), ("request", TURNS)]);
        assert_eq!(client.reply().await.0, 200);
        api.close().await;
    })
    .await;
}

#[tokio::test]
async fn two_answers_a_worker_thread_sends_together_each_run_their_chain_whole() {
    scene(async {
        let Worked { spawn, .. } = worked();
        let standing = Rc::new(Standing::default());
        let records = standing.over(&spawn);
        let order = Order::default();
        for name in ["a", "b"] {
            let (order, records) = (Rc::clone(&order), Rc::clone(&records));
            spawn.apart("a chain failed", async move {
                look(&records).await;
                chain(order, name, TURNS).await;
            });
        }
        spawn.drain();
        // Both answers are sent while the thread of the engine is held: when
        // it goes on, both are there.
        standing.answer(2);
        settle().await;
        assert_whole(&order, [("a", TURNS), ("b", TURNS)]);
    })
    .await;
}

#[tokio::test]
async fn an_answer_a_thread_sends_while_a_drain_runs_waits_for_the_drain_to_end() {
    scene(async {
        let Worked { spawn, .. } = worked();
        let standing = Rc::new(Standing::default());
        let records = standing.over(&spawn);
        let order = Order::default();
        let (waiting, asked) = (Rc::clone(&records), Rc::clone(&order));
        spawn.apart("a chain failed", async move {
            look(&waiting).await;
            chain(asked, "b", TURNS).await;
        });
        // A long chain, at whose third turn the worker answers the look: in
        // the middle of the drain that runs the chain.
        let (answering, long) = (Rc::clone(&standing), Rc::clone(&order));
        spawn.apart("a chain failed", async move {
            for turn in 0..6 {
                long.borrow_mut().push(format!("a {turn}"));
                if turn == 2 {
                    answering.answer(1);
                }
                next_turn().await;
            }
        });
        spawn.drain();
        settle().await;
        assert_whole(&order, [("a", 6), ("b", TURNS)]);
    })
    .await;
}

#[tokio::test]
async fn an_answer_a_thread_sends_during_the_drain_after_an_exit_waits_for_its_end() {
    scene(async {
        let mut pieces = Pieces::new(Transport::Memory).await;
        let standing = Rc::new(Standing::default());
        let records = standing.over(&pieces.spawn);
        let order = Order::default();
        let (waiting, asked) = (Rc::clone(&records), Rc::clone(&order));
        pieces.spawn.apart("a chain failed", async move {
            look(&waiting).await;
            chain(asked, "b", TURNS).await;
        });
        pieces.spawn.drain();
        // The exit's handler begins a long chain, at whose third turn the
        // worker answers the look: in the drain the reader runs after its read.
        let (answering, long) = (Rc::clone(&standing), Rc::clone(&order));
        drop(watch_exits(
            &pieces.bridge,
            Rc::clone(&pieces.spawn),
            move |_| {
                let (answering, long) = (Rc::clone(&answering), Rc::clone(&long));
                let rest: LocalWork = Box::pin(async move {
                    for turn in 0..6 {
                        long.borrow_mut().push(format!("a {turn}"));
                        if turn == 2 {
                            answering.answer(1);
                        }
                        next_turn().await;
                    }
                });
                Some(rest)
            },
        ));
        let exit = pieces
            .host
            .exit(&json!({ "id": "p1-zeus", "generation": 1 }));
        pieces.host.write(&exit).await;
        settle().await;
        assert_whole(&order, [("a", 6), ("b", TURNS)]);
    })
    .await;
}

#[tokio::test]
async fn an_answer_a_thread_sends_during_the_first_part_of_a_request_waits_for_its_end() {
    scene(async {
        let pieces = Pieces::new(Transport::Memory).await;
        let standing = Rc::new(Standing::default());
        let records = standing.over(&pieces.spawn);
        let order = Order::default();
        let (waiting, asked) = (Rc::clone(&records), Rc::clone(&order));
        pieces.spawn.apart("a chain failed", async move {
            look(&waiting).await;
            chain(asked, "b", TURNS).await;
        });
        pieces.spawn.drain();
        // The request's handler is a long chain, at whose third turn the worker
        // answers the look: in the drain that ends the request's first part.
        let (answering, long) = (Rc::clone(&standing), Rc::clone(&order));
        let handler: Rc<Handler> = Rc::new(move |_| {
            let (answering, long) = (Rc::clone(&answering), Rc::clone(&long));
            Box::pin(async move {
                for turn in 0..6 {
                    long.borrow_mut().push(format!("a {turn}"));
                    if turn == 2 {
                        answering.answer(1);
                    }
                    next_turn().await;
                }
                Ok(Answer::ok(Value::Null))
            })
        });
        let api = Api::start(handler, Closing::new(), Rc::clone(&pieces.spawn))
            .await
            .expect("the front listens");
        let mut client = Client::connect(&api).await;
        client.send(head("/x", 0).as_bytes()).await;
        assert_eq!(client.reply().await.0, 200);
        settle().await;
        assert_whole(&order, [("a", 6), ("b", TURNS)]);
        api.close().await;
    })
    .await;
}

#[tokio::test]
async fn a_wait_that_is_dropped_leaves_no_callback_and_no_task_behind() {
    scene(async {
        let Worked { spawn, .. } = worked();
        let before = Rc::strong_count(&spawn);
        let mut waiting = spawn.arrival(Box::pin(std::future::pending::<()>()));
        // Polled once, it is armed: its relay holds the executor.
        let waker = std::task::Waker::noop();
        assert!(std::future::Future::poll(
            waiting.as_mut(),
            &mut std::task::Context::from_waker(waker)
        )
        .is_pending());
        assert!(Rc::strong_count(&spawn) > before, "the relay is there");
        drop(waiting);
        settle().await;
        assert_eq!(Rc::strong_count(&spawn), before, "it went with the wait");
    })
    .await;
}

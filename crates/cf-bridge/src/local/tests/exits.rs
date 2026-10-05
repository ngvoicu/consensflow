//! The exit rule. A window's exit can arrive before the answer to the open of
//! its pane, and the engine must know of it when the answer is: so an event's
//! handler runs on the reader, in frame order, before the next frame is read;
//! what it changes, it changes before it returns, and the rest of its work
//! goes on a task of its own that the reader never waits for.
//!
//! These scenarios poll the connection by hand. A `LocalSet` runs the tasks
//! spawned while a frame was handled before it lets whatever waited for the
//! next frame go on, so from a task that waits for the answer the work an
//! exit started is always seen as run; only polling in step tells a handler
//! that ran in place from one that was put off.

use std::cell::{Cell, RefCell};
use std::pin::pin;
use std::rc::Rc;
use std::task::{Context, Poll, Waker};

use cf_proto::bridge::Role;
use serde_json::json;
use tokio::io::{duplex, AsyncWriteExt};
use tokio::sync::oneshot;

use super::wire::{frame, lonely, run, wait_for, within};
use crate::local::BridgeBuilder;

fn poll_once<F: std::future::Future>(future: std::pin::Pin<&mut F>) -> Poll<F::Output> {
    future.poll(&mut Context::from_waker(Waker::noop()))
}

#[test]
fn an_exit_written_back_to_back_with_the_answer_to_an_open_is_handled_before_the_open_resolves() {
    run(async {
        let (mut peer, input) = duplex(64 * 1024);
        let (output, _from_bridge) = duplex(64 * 1024);
        let (bridge, connection) = BridgeBuilder::new(Role::Daemon).connect(input, output);
        let mut connection = pin!(connection);

        let exited = Rc::new(Cell::new(false));
        let rest_ran = Rc::new(Cell::new(false));
        let (exit_flag, rest_flag) = (Rc::clone(&exited), Rc::clone(&rest_ran));
        bridge.on_event("pane.exit", move |_| {
            // What the exit changes, changed before the handler returns...
            exit_flag.set(true);
            // ...and the rest of its work, run apart.
            let rest_flag = Rc::clone(&rest_flag);
            tokio::task::spawn_local(async move { rest_flag.set(true) });
        });

        let open = bridge.request("pane.open", json!({ "id": "p1" }), None);
        let mut open = pin!(open);
        let both = format!(
            "{}\n{}\n",
            frame(
                "evt",
                "r-7",
                "pane.exit",
                json!({ "id": "p1", "generation": 1 })
            ),
            frame("res", "n-1", "pane.open", json!({ "ok": true }))
        );
        peer.write_all(both.as_bytes()).await.unwrap();

        // The reader takes both frames in one go.
        assert!(poll_once(connection.as_mut()).is_pending());
        assert!(exited.get(), "the exit's handler has run");
        assert!(!rest_ran.get(), "and the work it started has not");
        assert_eq!(
            poll_once(open.as_mut()),
            Poll::Ready(Ok(json!({ "ok": true }))),
            "the open's answer is in"
        );
        assert!(exited.get() && !rest_ran.get());

        // The rest runs apart, on a task of its own.
        wait_for(|| rest_ran.get()).await;
    });
}

#[test]
fn an_exit_that_comes_after_the_answer_is_handled_after_it() {
    run(async {
        let (mut peer, input) = duplex(64 * 1024);
        let (output, _from_bridge) = duplex(64 * 1024);
        let (bridge, connection) = BridgeBuilder::new(Role::Daemon).connect(input, output);
        let mut connection = pin!(connection);
        let exited = Rc::new(Cell::new(false));
        let flag = Rc::clone(&exited);
        bridge.on_event("pane.exit", move |_| flag.set(true));
        let open = bridge.request("pane.open", json!({}), None);
        let mut open = pin!(open);

        peer.write_all(format!("{}\n", frame("res", "n-1", "pane.open", json!({}))).as_bytes())
            .await
            .unwrap();
        assert!(poll_once(connection.as_mut()).is_pending());
        assert!(!exited.get());
        assert!(poll_once(open.as_mut()).is_ready());

        peer.write_all(format!("{}\n", frame("evt", "r-1", "pane.exit", json!({}))).as_bytes())
            .await
            .unwrap();
        assert!(poll_once(connection.as_mut()).is_pending());
        assert!(exited.get());
    });
}

#[test]
fn an_exit_handler_runs_before_the_next_frame_is_dispatched() {
    run(async {
        let (bridge, mut wire) = lonely();
        let for_the_handler = bridge.clone();
        bridge.on_event("pane.exit", move |_| {
            for_the_handler.event("exit.handled", json!({}));
        });
        let both = format!(
            "{}\n{}\n",
            frame("evt", "r-1", "pane.exit", json!({})),
            frame("req", "r-2", "nobody", json!({}))
        );
        wire.send_text(&both).await;
        let frames = wire.wait_for_frames(2).await;
        assert_eq!(frames[0]["op"], "exit.handled");
        assert_eq!(
            frames[1],
            frame(
                "res",
                "r-2",
                "nobody",
                json!({ "ok": false, "error": "unknown-op" })
            )
        );
        bridge.close();
    });
}

#[test]
fn the_work_an_exit_handler_starts_is_never_waited_for_by_the_reader() {
    run(async {
        let (bridge, mut wire) = lonely();
        let log: Rc<RefCell<Vec<&str>>> = Rc::default();
        let (release, gate) = oneshot::channel::<()>();
        let gate = RefCell::new(Some(gate));
        let handler_log = Rc::clone(&log);
        bridge.on_event("pane.exit", move |_| {
            handler_log.borrow_mut().push("exit handled");
            let (gate, log) = (gate.borrow_mut().take(), Rc::clone(&handler_log));
            tokio::task::spawn_local(async move {
                if let Some(gate) = gate {
                    let _ = gate.await;
                }
                log.borrow_mut().push("the rest done");
            });
        });
        let open = bridge.request("pane.open", json!({}), None);
        let both = format!(
            "{}\n{}\n",
            frame("evt", "r-1", "pane.exit", json!({})),
            frame("res", "n-1", "pane.open", json!({ "ok": true }))
        );
        wire.send_text(&both).await;
        // The answer after the exit is dispatched while the exit's rest waits.
        assert_eq!(within(open).await, Ok(json!({ "ok": true })));
        assert_eq!(*log.borrow(), ["exit handled"]);
        release.send(()).unwrap();
        wait_for(|| log.borrow().len() == 2).await;
        assert_eq!(*log.borrow(), ["exit handled", "the rest done"]);
    });
}

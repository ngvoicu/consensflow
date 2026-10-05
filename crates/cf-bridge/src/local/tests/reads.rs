//! `BridgeBuilder::after_read`: told once the frames of one read are handled
//! and before the reader reads again, which is where the daemon runs, to
//! their end, the work those frames woke.

use std::cell::RefCell;
use std::future::pending;
use std::rc::Rc;

use cf_proto::bridge::Role;
use serde_json::{json, Value};

use super::wire::{frame, lonely_with, quiet, run, Wire};
use crate::local::{Bridge, BridgeBuilder};

type Log = Rc<RefCell<Vec<String>>>;

/// A bridge that writes into `log` each `tick` it hears and each read it has
/// handled, with the test for its peer.
fn watched(log: &Log) -> (Bridge, Wire) {
    let told = Rc::clone(log);
    let (bridge, wire) = lonely_with(
        BridgeBuilder::new(Role::Daemon)
            .after_read(move || told.borrow_mut().push("read".to_owned())),
    );
    let heard = Rc::clone(log);
    bridge.on_event("tick", move |body| {
        heard.borrow_mut().push(format!("tick {}", body["n"]));
    });
    (bridge, wire)
}

fn tick(n: u32) -> Value {
    frame("evt", &format!("r-{n}"), "tick", json!({ "n": n }))
}

/// Frames as the peer writes them: a line each.
fn lines(frames: &[Value]) -> String {
    frames.iter().map(|frame| format!("{frame}\n")).collect()
}

#[test]
fn it_is_told_once_after_all_the_frames_of_a_read_and_not_between_them() {
    run(async {
        let log = Log::default();
        let (_bridge, mut wire) = watched(&log);
        wire.send_text(&lines(&[tick(1), tick(2), tick(3)])).await;
        quiet().await;
        assert_eq!(*log.borrow(), ["tick 1", "tick 2", "tick 3", "read"]);
    });
}

#[test]
fn each_read_is_told_after_its_own_frames_and_before_the_next_is_read() {
    run(async {
        let log = Log::default();
        let (_bridge, mut wire) = watched(&log);
        wire.send(tick(1)).await;
        quiet().await;
        wire.send(tick(2)).await;
        quiet().await;
        assert_eq!(*log.borrow(), ["tick 1", "read", "tick 2", "read"]);
    });
}

#[test]
fn a_read_with_no_whole_frame_is_told_too_and_the_frame_it_began_is_handled_when_it_ends() {
    run(async {
        let log = Log::default();
        let (_bridge, mut wire) = watched(&log);
        let whole = lines(&[tick(1)]);
        let (head, tail) = whole.split_at(10);
        wire.send_text(head).await;
        quiet().await;
        assert_eq!(*log.borrow(), ["read"]);
        wire.send_text(tail).await;
        quiet().await;
        assert_eq!(*log.borrow(), ["read", "tick 1", "read"]);
    });
}

#[test]
fn a_request_handler_has_had_its_first_poll_by_then_and_is_not_waited_for() {
    run(async {
        let log = Log::default();
        let (bridge, mut wire) = watched(&log);
        let asked = Rc::clone(&log);
        bridge.on("ask", move |_, _| {
            asked.borrow_mut().push("asked".to_owned());
            async {
                pending::<()>().await;
                Ok(json!({}))
            }
        });
        let request = frame("req", "r-9", "ask", json!({}));
        wire.send_text(&lines(&[request, tick(1)])).await;
        quiet().await;
        assert_eq!(*log.borrow(), ["asked", "tick 1", "read"]);
        assert_eq!(wire.text(), "", "it waits, and the reader went on");
    });
}

#[test]
fn a_read_whose_frames_closed_the_bridge_is_told_once_for_the_frames_before_the_close() {
    run(async {
        let log = Log::default();
        let (bridge, mut wire) = watched(&log);
        let closing = bridge.clone();
        bridge.on_event("close", move |_| closing.close());
        let close = frame("evt", "r-2", "close", json!({}));
        wire.send_text(&lines(&[tick(1), close, tick(3)])).await;
        quiet().await;
        assert!(bridge.closed());
        assert_eq!(*log.borrow(), ["tick 1", "read"]);
    });
}

#[test]
fn the_end_of_the_input_handled_nothing_and_is_not_told() {
    run(async {
        let log = Log::default();
        let (bridge, mut wire) = watched(&log);
        wire.end();
        quiet().await;
        assert!(bridge.closed());
        assert!(log.borrow().is_empty());
    });
}

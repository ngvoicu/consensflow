//! How a bridge ends, and what it lets go of: the streams, the handlers, the
//! task that runs it.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use cf_proto::bridge::Role;
use serde_json::json;
use tokio::io::{duplex, AsyncReadExt, DuplexStream};

use super::wire::{collector, frame, lonely, lonely_with, quiet, run, wait_for, within};
use crate::local::{BridgeBuilder, Subscription};
use crate::BridgeError;

/// Everything the bridge writes, to the end of its output.
async fn until_the_output_ends(output: &mut DuplexStream) -> String {
    let mut text = String::new();
    within(output.read_to_string(&mut text))
        .await
        .expect("the output is text");
    text
}

#[test]
fn closing_refuses_what_waits_with_eof_and_ends_the_output() {
    run(async {
        let (bridge, wire) = lonely();
        let pending = bridge.request("hangs", json!({}), None);
        bridge.close();
        assert_eq!(within(pending).await, Err(BridgeError::Eof));
        assert!(bridge.closed());
        assert!(!bridge.event("late", json!({})));
        wait_for(|| wire.output_ended()).await;
        assert_eq!(
            within(bridge.request("late", json!({}), None)).await,
            Err(BridgeError::Eof)
        );
    });
}

#[test]
fn closing_twice_does_nothing_the_second_time() {
    run(async {
        let (errors, on_error) = collector();
        let (bridge, wire) = lonely_with(BridgeBuilder::new(Role::Daemon).on_error(on_error));
        bridge.close();
        bridge.close();
        wait_for(|| wire.output_ended()).await;
        assert!(errors.borrow().is_empty());
    });
}

#[test]
fn closing_writes_what_was_queued_before_it_and_then_ends_the_output() {
    run(async {
        let (to_bridge, input) = duplex(64 * 1024);
        let (output, mut from_bridge) = duplex(64 * 1024);
        let (bridge, connection) = BridgeBuilder::new(Role::Daemon).connect(input, output);
        bridge.event("one", json!({}));
        bridge.event("two", json!({}));
        bridge.close();
        tokio::task::spawn_local(connection);
        let text = until_the_output_ends(&mut from_bridge).await;
        let ops: Vec<_> = text
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap()["op"].clone())
            .collect();
        assert_eq!(ops, [json!("one"), json!("two")]);
        drop(to_bridge);
    });
}

#[test]
fn a_failure_writes_what_was_queued_before_it_and_then_ends_the_output() {
    run(async {
        let (to_bridge, input) = duplex(64 * 1024);
        let (output, mut from_bridge) = duplex(64 * 1024);
        let (bridge, connection) = BridgeBuilder::new(Role::Daemon).connect(input, output);
        bridge.event("one", json!({}));
        bridge.event("two", json!({}));
        bridge.inner.fail(BridgeError::Io("boom".to_owned()));
        assert!(!bridge.event("three", json!({})));
        tokio::task::spawn_local(connection);
        let text = until_the_output_ends(&mut from_bridge).await;
        assert_eq!(text.lines().count(), 2);
        assert_eq!(
            within(bridge.request("late", json!({}), None)).await,
            Err(BridgeError::Io("boom".to_owned()))
        );
        drop(to_bridge);
    });
}

#[test]
fn a_connection_that_is_dropped_closes_the_bridge() {
    run(async {
        let (to_bridge, input) = duplex(64 * 1024);
        let (output, _from_bridge) = duplex(64 * 1024);
        let (bridge, connection) = BridgeBuilder::new(Role::Daemon).connect(input, output);
        let running = tokio::task::spawn_local(connection);
        let pending = bridge.request("hangs", json!({}), None);
        quiet().await;
        assert!(!bridge.closed());
        running.abort();
        assert_eq!(within(pending).await, Err(BridgeError::Eof));
        assert!(bridge.closed());
        drop(to_bridge);
    });
}

#[test]
fn an_event_handler_that_panics_ends_the_reader_and_with_it_the_bridge() {
    run(async {
        let (bridge, mut wire) = lonely();
        bridge.on_event("bug", |_| panic!("a bug in a handler"));
        let pending = bridge.request("hangs", json!({}), None);
        wire.send(frame("evt", "r-1", "bug", json!({}))).await;
        assert_eq!(within(pending).await, Err(BridgeError::Eof));
        assert!(bridge.closed());
    });
}

#[test]
fn a_request_handler_that_panics_before_its_first_wait_ends_the_bridge_the_same_way() {
    run(async {
        let (bridge, mut wire) = lonely();
        bridge.on(
            "bug",
            |_, _| -> std::future::Ready<Result<serde_json::Value, String>> {
                panic!("a bug in a handler")
            },
        );
        let pending = bridge.request("hangs", json!({}), None);
        wire.send(frame("req", "r-1", "bug", json!({}))).await;
        assert_eq!(within(pending).await, Err(BridgeError::Eof));
        assert!(bridge.closed());
    });
}

#[test]
fn a_request_handler_that_panics_after_a_wait_loses_its_own_answer_and_nothing_else() {
    run(async {
        let (bridge, mut wire) = lonely();
        bridge.on("bug", |_, _| async {
            tokio::task::yield_now().await;
            panic!("a bug in a handler");
            #[allow(unreachable_code)]
            Ok(json!({}))
        });
        bridge.on("ping", |_, _| async { Ok(json!({ "ok": true })) });
        wire.send(frame("req", "r-1", "bug", json!({}))).await;
        wire.send(frame("req", "r-2", "ping", json!({}))).await;
        let frames = wire.wait_for_frames(1).await;
        quiet().await;
        assert_eq!(frames[0]["id"], "r-2");
        assert_eq!(
            wire.frames().len(),
            1,
            "the peer's own deadline settles r-1"
        );
        assert!(!bridge.closed());
    });
}

#[test]
fn the_connection_is_finished_once_the_bridge_is_closed() {
    run(async {
        let (_to_bridge, input) = duplex(64 * 1024);
        let (output, _from_bridge) = duplex(64 * 1024);
        let (bridge, connection) = BridgeBuilder::new(Role::Daemon).connect(input, output);
        let running = tokio::task::spawn_local(connection);
        quiet().await;
        assert!(!running.is_finished());
        bridge.close();
        within(running).await.unwrap();
    });
}

#[test]
fn after_the_input_ends_the_connection_goes_on_until_every_handle_is_gone() {
    run(async {
        let (to_bridge, input) = duplex(64 * 1024);
        let (output, _from_bridge) = duplex(64 * 1024);
        let (bridge, connection) = BridgeBuilder::new(Role::Daemon).connect(input, output);
        let running = tokio::task::spawn_local(connection);
        drop(to_bridge);
        wait_for(|| bridge.closed()).await;
        quiet().await;
        assert!(!running.is_finished(), "the output is still the bridge's");
        drop(bridge);
        within(running).await.unwrap();
    });
}

#[test]
fn the_handlers_are_let_go_when_the_bridge_closes() {
    run(async {
        let (bridge, _wire) = lonely();
        let held = Rc::new(());
        let (for_request, for_event) = (Rc::clone(&held), Rc::clone(&held));
        bridge.on("op", move |_, _| {
            let _held = Rc::clone(&for_request);
            async { Ok(json!({})) }
        });
        bridge.on_event("tick", move |_| {
            let _held = &for_event;
        });
        assert_eq!(Rc::strong_count(&held), 3);
        bridge.close();
        assert_eq!(Rc::strong_count(&held), 1);
    });
}

#[test]
fn a_handler_may_call_the_bridge_back_while_it_is_called() {
    run(async {
        let (bridge, mut wire) = lonely();
        let again = Rc::new(Cell::new(0));
        let (for_handler, counted) = (bridge.clone(), Rc::clone(&again));
        let own: Rc<RefCell<Option<Subscription>>> = Rc::default();
        let slot = Rc::clone(&own);
        let subscription = bridge.on_event("go", move |_| {
            counted.set(counted.get() + 1);
            for_handler.on("added.while.handling", |_, _| async { Ok(json!("new")) });
            for_handler.on_event("added.too", |_| {});
            drop(for_handler.request("asked.while.handling", json!({}), None));
            for_handler.event("told.while.handling", json!({}));
            let mine = slot.borrow_mut().take();
            if let Some(mine) = mine {
                mine.off();
            }
        });
        *own.borrow_mut() = Some(subscription);
        let calls = format!(
            "{}\n{}\n{}\n",
            frame("evt", "r-1", "go", json!({})),
            frame("evt", "r-2", "go", json!({})),
            frame("req", "r-3", "added.while.handling", json!({}))
        );
        wire.send_text(&calls).await;
        let frames = wire.wait_for_frames(3).await;
        assert_eq!(again.get(), 1, "it took itself away the first time");
        assert_eq!(
            frames[2],
            frame("res", "r-3", "added.while.handling", json!("new"))
        );
    });
}

#[test]
fn closing_from_inside_a_handler_stops_the_reader_after_that_frame() {
    run(async {
        let (errors, on_error) = collector();
        let (bridge, mut wire) = lonely_with(BridgeBuilder::new(Role::Daemon).on_error(on_error));
        let after = Rc::new(Cell::new(false));
        let (for_handler, flag) = (bridge.clone(), Rc::clone(&after));
        bridge.on_event("stop", move |_| for_handler.close());
        bridge.on_event("after", move |_| flag.set(true));
        // The rest of the chunk is not looked at once the bridge has closed:
        // not heard, and its junk not reported.
        let rest = format!(
            "{}\n{}\n",
            frame("evt", "r-2", "after", json!({})),
            "junk that would be reported"
        );
        let all = format!("{}\n{rest}", frame("evt", "r-1", "stop", json!({})));
        wire.send_text(&all).await;
        wait_for(|| bridge.closed()).await;
        quiet().await;
        assert!(!after.get());
        assert!(errors.borrow().is_empty(), "{:?}", errors.borrow());
    });
}

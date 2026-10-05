//! `Bridge frame limits` and `Bridge input size limit`: a frame over the limit
//! is refused in both directions, a line that is not a whole frame is
//! reported and skipped, and the transport keeps going or fails explicitly.

use std::cell::Cell;
use std::rc::Rc;

use cf_proto::bridge::Role;
use serde_json::json;

use super::wire::{collector, frame, lonely, lonely_with, quiet, run, wait_for, within};
use crate::local::BridgeBuilder;
use crate::BridgeError;

fn too_large() -> serde_json::Value {
    json!({ "ok": false, "error": "too-large" })
}

fn limited(bytes: usize) -> BridgeBuilder {
    BridgeBuilder::new(Role::Daemon).max_frame_bytes(bytes)
}

#[test]
fn refuses_an_outgoing_frame_over_maxframebytes_and_never_writes_it() {
    run(async {
        let (bridge, wire) = lonely_with(limited(64));
        let answer = within(bridge.request("big", json!({ "text": "x".repeat(200) }), None)).await;
        assert_eq!(answer, Ok(too_large()));
        assert_eq!(wire.text(), "");
        assert!(!bridge.event("big", json!({ "text": "y".repeat(200) })));
        quiet().await;
        assert_eq!(wire.text(), "");
    });
}

#[test]
fn a_refused_request_still_takes_its_id() {
    run(async {
        let (bridge, wire) = lonely_with(limited(64));
        within(bridge.request("big", json!({ "text": "x".repeat(200) }), None))
            .await
            .unwrap();
        let _pending = bridge.request("ok", json!({}), None);
        assert_eq!(wire.wait_for_frames(1).await[0]["id"], "n-2");
    });
}

#[test]
fn answers_an_incoming_oversized_request_with_too_large() {
    run(async {
        // The budget fits the small refusal but not the incoming frame.
        let (bridge, mut wire) = lonely_with(limited(100));
        bridge.on("big", |_, _| async { Ok(json!({ "ok": true })) });
        wire.send(frame(
            "req",
            "r-1",
            "big",
            json!({ "text": "x".repeat(200) }),
        ))
        .await;
        assert_eq!(
            wire.wait_for_frames(1).await[0],
            frame("res", "r-1", "big", too_large())
        );
    });
}

#[test]
fn an_oversized_request_is_answered_without_calling_its_handler() {
    run(async {
        let called = Rc::new(Cell::new(false));
        let (bridge, mut wire) = lonely_with(limited(100));
        let flag = Rc::clone(&called);
        bridge.on("big", move |_, _| {
            flag.set(true);
            async { Ok(json!({ "ok": true })) }
        });
        wire.send(frame(
            "req",
            "r-1",
            "big",
            json!({ "text": "x".repeat(200) }),
        ))
        .await;
        wire.wait_for_frames(1).await;
        assert!(!called.get());
    });
}

#[test]
fn refuses_to_answer_an_oversized_request_without_a_valid_op() {
    run(async {
        let (errors, on_error) = collector();
        let (bridge, mut wire) = lonely_with(limited(100).on_error(on_error));
        bridge.on("ping", |_, _| async { Ok(json!({ "ok": true })) });
        // No valid op to copy into a response: report, write nothing, stay open.
        let long = json!({ "text": "x".repeat(200) });
        wire.send(json!({ "v": 1, "id": "r-1", "kind": "req", "body": long }))
            .await;
        wire.send(json!({ "v": 1, "id": "r-2", "kind": "req", "op": 42, "body": long }))
            .await;
        wait_for(|| errors.borrow().len() == 2).await;
        assert!(errors.borrow()[0].contains("maxFrameBytes"));
        assert!(errors.borrow()[1].contains("maxFrameBytes"));
        assert_eq!(wire.text(), "");
        assert!(!bridge.closed());
        wire.send(frame("req", "r-3", "ping", json!({}))).await;
        assert_eq!(
            wire.wait_for_frames(1).await[0]["body"],
            json!({ "ok": true })
        );
    });
}

#[test]
fn rejects_a_response_without_a_body_and_keeps_the_request_pending() {
    run(async {
        let (errors, on_error) = collector();
        let (bridge, mut wire) = lonely_with(BridgeBuilder::new(Role::Daemon).on_error(on_error));
        let pending = bridge.request("echo", json!({}), None);
        wire.send(json!({ "v": 1, "id": "n-1", "kind": "res", "op": "echo" }))
            .await;
        wait_for(|| errors.borrow().len() == 1).await;
        assert!(errors.borrow()[0].contains("malformed"));
        wire.send(frame("res", "n-1", "echo", json!({ "ok": true })))
            .await;
        assert_eq!(within(pending).await, Ok(json!({ "ok": true })));
    });
}

#[test]
fn a_response_whose_body_is_null_has_a_body() {
    run(async {
        let (bridge, mut wire) = lonely();
        let pending = bridge.request("echo", json!({}), None);
        wire.send(frame("res", "n-1", "echo", json!(null))).await;
        assert_eq!(within(pending).await, Ok(json!(null)));
    });
}

#[test]
fn rejects_requests_and_events_without_a_body() {
    run(async {
        let (errors, on_error) = collector();
        let (bridge, mut wire) = lonely_with(BridgeBuilder::new(Role::Daemon).on_error(on_error));
        let pinged = Rc::new(Cell::new(0));
        let noted = Rc::new(Cell::new(0));
        let (ping, note) = (Rc::clone(&pinged), Rc::clone(&noted));
        bridge.on("ping", move |_, _| {
            ping.set(ping.get() + 1);
            async { Ok(json!({ "ok": true })) }
        });
        bridge.on_event("note", move |_| note.set(note.get() + 1));
        wire.send(json!({ "v": 1, "id": "r-1", "kind": "req", "op": "ping" }))
            .await;
        wire.send(json!({ "v": 1, "id": "r-2", "kind": "evt", "op": "note" }))
            .await;
        wait_for(|| errors.borrow().len() == 2).await;
        assert_eq!((pinged.get(), noted.get()), (0, 0));
        assert_eq!(wire.text(), "");
    });
}

#[test]
fn reports_a_malformed_oversized_request_and_answers_what_follows() {
    run(async {
        let (errors, on_error) = collector();
        let (bridge, mut wire) = lonely_with(limited(100).on_error(on_error));
        bridge.on("ping", |_, _| async { Ok(json!({ "ok": true })) });
        wire.send(json!({
            "v": 2, "id": "r-1", "kind": "req", "op": "big", "body": { "text": "x".repeat(200) }
        }))
        .await;
        wait_for(|| errors.borrow().len() == 1).await;
        assert!(errors.borrow()[0].contains("maxFrameBytes"));
        wire.send(frame("req", "r-2", "ping", json!({}))).await;
        assert_eq!(
            wire.wait_for_frames(1).await[0],
            frame("res", "r-2", "ping", json!({ "ok": true }))
        );
    });
}

#[test]
fn never_settles_a_pending_request_from_a_malformed_oversized_response() {
    run(async {
        let (errors, on_error) = collector();
        let (bridge, mut wire) = lonely_with(limited(100).on_error(on_error));
        let pending = bridge.request("echo", json!({}), None);
        let settled = Rc::new(Cell::new(false));
        let flag = Rc::clone(&settled);
        let watching = tokio::task::spawn_local(async move {
            let _ = pending.await;
            flag.set(true);
        });
        wire.send(json!({
            "v": 2, "id": "n-1", "kind": "res", "op": "echo", "body": { "text": "x".repeat(200) }
        }))
        .await;
        wait_for(|| errors.borrow().len() == 1).await;
        assert!(errors.borrow()[0].contains("maxFrameBytes"));
        quiet().await;
        assert!(!settled.get());
        wire.send(frame("res", "n-1", "echo", json!({ "ok": true })))
            .await;
        within(watching).await.unwrap();
        assert!(settled.get());
    });
}

#[test]
fn settles_a_pending_request_with_too_large_from_a_well_formed_oversized_response() {
    run(async {
        let (bridge, mut wire) = lonely_with(limited(100));
        let pending = bridge.request("echo", json!({}), None);
        wire.send(frame(
            "res",
            "n-1",
            "echo",
            json!({ "text": "x".repeat(200) }),
        ))
        .await;
        assert_eq!(within(pending).await, Ok(too_large()));
    });
}

#[test]
fn reports_an_oversized_event_and_hears_nothing_of_it() {
    run(async {
        let (errors, on_error) = collector();
        let (bridge, mut wire) = lonely_with(limited(100).on_error(on_error));
        let heard = Rc::new(Cell::new(false));
        let flag = Rc::clone(&heard);
        bridge.on_event("note", move |_| flag.set(true));
        wire.send(frame(
            "evt",
            "r-1",
            "note",
            json!({ "text": "x".repeat(200) }),
        ))
        .await;
        wait_for(|| errors.borrow().len() == 1).await;
        assert!(errors.borrow()[0].contains("maxFrameBytes"));
        assert!(!heard.get());
        assert_eq!(wire.text(), "");
    });
}

#[test]
fn an_oversized_frame_in_the_wrong_namespace_is_reported_and_not_answered() {
    run(async {
        let (errors, on_error) = collector();
        let (_bridge, mut wire) = lonely_with(limited(100).on_error(on_error));
        wire.send(frame(
            "req",
            "n-1",
            "big",
            json!({ "text": "x".repeat(200) }),
        ))
        .await;
        wait_for(|| errors.borrow().len() == 1).await;
        assert_eq!(wire.text(), "");
    });
}

#[test]
fn answers_too_large_when_a_handler_response_would_not_fit() {
    run(async {
        let (bridge, mut wire) = lonely_with(limited(100));
        bridge.on("big", |_, _| async {
            Ok(json!({ "text": "x".repeat(200) }))
        });
        wire.send(frame("req", "r-1", "big", json!({}))).await;
        assert_eq!(
            wire.wait_for_frames(1).await[0],
            frame("res", "r-1", "big", too_large())
        );
    });
}

#[test]
fn fails_the_transport_when_even_the_too_large_response_cannot_fit() {
    run(async {
        let (errors, on_error) = collector();
        let (bridge, mut wire) = lonely_with(limited(60).on_error(on_error));
        let pending = bridge.request("hangs", json!({}), None);
        // The incoming request is oversized, and the bounded too-large refusal
        // does not fit the 60-byte budget either: the peer would hear nothing,
        // so the transport fails instead of leaving it waiting.
        wire.send(frame(
            "req",
            "r-1",
            "big",
            json!({ "text": "x".repeat(200) }),
        ))
        .await;
        let result = within(pending).await;
        assert!(
            matches!(&result, Err(BridgeError::MalformedFrame(why)) if why.contains("cannot fit maxFrameBytes")),
            "{result:?}"
        );
        assert_eq!(
            wire.frames(),
            [frame("req", "n-1", "hangs", json!({}))],
            "only the request made before is on the output"
        );
        assert!(bridge.closed());
        wait_for(|| !errors.borrow().is_empty()).await;
        assert!(errors.borrow()[0].contains("maxFrameBytes"));
        wait_for(|| wire.output_ended()).await;
    });
}

#[test]
fn refuses_an_unterminated_line_over_maxframebytes_and_recovers() {
    run(async {
        let (errors, on_error) = collector();
        let (bridge, mut wire) = lonely_with(limited(100).on_error(on_error));
        bridge.on("ping", |_, _| async { Ok(json!({ "ok": true })) });
        wire.send_text(&"x".repeat(300)).await;
        wait_for(|| !errors.borrow().is_empty()).await;
        assert!(errors.borrow()[0].contains("maxFrameBytes"));
        assert_eq!(wire.text(), "");
        assert!(!bridge.closed());
        // The rest of the over-long line is discarded through its newline...
        wire.send_text(&"y".repeat(50)).await;
        wire.send_text("\n").await;
        // ...and the next frame is served whole.
        wire.send(frame("req", "r-1", "ping", json!({}))).await;
        assert_eq!(
            wire.wait_for_frames(1).await[0]["body"],
            json!({ "ok": true })
        );
        assert_eq!(errors.borrow().len(), 1, "an overflow is reported once");
    });
}

#[test]
fn bounds_fragmented_overflow_across_chunks() {
    run(async {
        let (errors, on_error) = collector();
        let (bridge, mut wire) = lonely_with(limited(100).on_error(on_error));
        bridge.on("ping", |_, _| async { Ok(json!({ "ok": true })) });
        for _ in 0..10 {
            wire.send_text(&"x".repeat(30)).await;
            quiet().await;
        }
        wait_for(|| !errors.borrow().is_empty()).await;
        assert!(!bridge.closed());
        wire.send_text("\n").await;
        wire.send(frame("req", "r-2", "ping", json!({}))).await;
        assert_eq!(
            wire.wait_for_frames(1).await[0]["body"],
            json!({ "ok": true })
        );
        assert_eq!(errors.borrow().len(), 1, "an overflow is reported once");
    });
}

#[test]
fn two_overlong_lines_are_each_reported_once() {
    run(async {
        let (errors, on_error) = collector();
        let (_bridge, mut wire) = lonely_with(limited(100).on_error(on_error));
        for _ in 0..2 {
            wire.send_text(&"x".repeat(300)).await;
            quiet().await;
            wire.send_text("tail\n").await;
            quiet().await;
        }
        assert_eq!(errors.borrow().len(), 2);
    });
}

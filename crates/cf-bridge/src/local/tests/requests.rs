//! `Bridge request/response over two streams` and `Bridge default request
//! deadline`, and the rules of a request that those sentences hold.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use cf_proto::bridge::Role;
use serde_json::json;
use tokio::time::Instant;

use super::wire::{collector, frame, lonely, lonely_with, pair, quiet, run, wait_for, within};
use crate::local::{BridgeBuilder, DEFAULT_DEADLINE, DEFAULT_MAX_FRAME_BYTES};
use crate::BridgeError;

#[test]
fn resolves_a_request_with_what_the_peer_handler_returns() {
    run(async {
        let (a, b) = pair();
        b.on("add", |_, body| async move {
            Ok(json!({ "sum": body["x"].as_i64().unwrap_or_default() + 1 }))
        });
        assert_eq!(
            within(a.request("add", json!({ "x": 1 }), None)).await,
            Ok(json!({ "sum": 2 }))
        );
    });
}

#[test]
fn turns_a_throwing_handler_into_ok_false_error() {
    run(async {
        let (a, b) = pair();
        b.on("boom", |_, _| std::future::ready(Err("nope".to_owned())));
        assert_eq!(
            within(a.request("boom", json!({}), None)).await,
            Ok(json!({ "ok": false, "error": "nope" }))
        );
    });
}

#[test]
fn turns_an_async_rejection_into_ok_false_error() {
    run(async {
        let (a, b) = pair();
        b.on("later", |_, _| async {
            tokio::task::yield_now().await;
            Err("async nope".to_owned())
        });
        assert_eq!(
            within(a.request("later", json!({}), None)).await,
            Ok(json!({ "ok": false, "error": "async nope" }))
        );
    });
}

#[test]
fn stops_calling_a_request_handler_after_unsubscribe() {
    run(async {
        let (a, b) = pair();
        let calls = Rc::new(Cell::new(0));
        let counted = Rc::clone(&calls);
        let off = b.on("temp", move |_, _| {
            counted.set(counted.get() + 1);
            async { Ok(json!({ "ok": true })) }
        });
        assert_eq!(
            within(a.request("temp", json!({}), None)).await,
            Ok(json!({ "ok": true }))
        );
        off.off();
        assert_eq!(
            within(a.request("temp", json!({}), None)).await,
            Ok(json!({ "ok": false, "error": "unknown-op" }))
        );
        assert_eq!(calls.get(), 1);
    });
}

#[test]
fn a_later_handler_replaces_the_one_before_it_and_the_first_one_leaving_does_not_take_it_away() {
    run(async {
        let (a, b) = pair();
        let first = b.on("op", |_, _| async { Ok(json!("first")) });
        b.on("op", |_, _| async { Ok(json!("second")) });
        assert_eq!(
            within(a.request("op", json!({}), None)).await,
            Ok(json!("second"))
        );
        first.off();
        assert_eq!(
            within(a.request("op", json!({}), None)).await,
            Ok(json!("second"))
        );
    });
}

#[test]
fn answers_an_unknown_op_instead_of_hanging() {
    run(async {
        let (a, _b) = pair();
        assert_eq!(
            within(a.request("missing", json!({}), None)).await,
            Ok(json!({ "ok": false, "error": "unknown-op" }))
        );
    });
}

#[test]
fn namespaces_originated_ids_n_n() {
    run(async {
        let (bridge, mut wire) = lonely();
        let first = bridge.request("echo", json!({ "a": 1 }), None);
        let sent = wire.wait_for_frames(1).await;
        assert_eq!(sent[0], frame("req", "n-1", "echo", json!({ "a": 1 })));
        wire.send(frame("res", "n-1", "echo", json!({ "ok": true })))
            .await;
        assert_eq!(within(first).await, Ok(json!({ "ok": true })));

        let second = bridge.request("echo", json!({}), None);
        let sent = wire.wait_for_frames(2).await;
        assert_eq!(sent[1]["id"], "n-2");
        wire.end();
        assert_eq!(within(second).await, Err(BridgeError::Eof));
    });
}

#[test]
fn a_host_end_namespaces_its_ids_r_n_and_accepts_the_daemons() {
    run(async {
        let (host, mut wire) = lonely_with(BridgeBuilder::new(Role::Host));
        host.on("ping", |_, _| async { Ok(json!({ "ok": true })) });
        let pending = host.request("echo", json!({}), None);
        wire.send(frame("req", "n-1", "ping", json!({}))).await;
        let sent = wire.wait_for_frames(2).await;
        assert_eq!(sent[0]["id"], "r-1");
        assert_eq!(sent[1], frame("res", "n-1", "ping", json!({ "ok": true })));
        wire.send(frame("res", "r-1", "echo", json!({ "ok": 1 })))
            .await;
        assert_eq!(within(pending).await, Ok(json!({ "ok": 1 })));
    });
}

#[test]
fn a_frame_goes_out_with_its_keys_in_the_order_node_wrote_them() {
    run(async {
        let (bridge, wire) = lonely();
        let _pending = bridge.request("echo", json!({ "a": 1 }), None);
        bridge.event("note", json!({ "b": [1, 2] }));
        wire.wait_for_frames(2).await;
        assert_eq!(
            wire.text(),
            concat!(
                r#"{"v":1,"id":"n-1","kind":"req","op":"echo","body":{"a":1}}"#,
                "\n",
                r#"{"v":1,"id":"n-2","kind":"evt","op":"note","body":{"b":[1,2]}}"#,
                "\n"
            )
        );
    });
}

#[test]
fn does_not_settle_a_request_from_a_wrong_namespace_or_mismatched_response_op() {
    run(async {
        let (errors, on_error) = collector();
        let (bridge, mut wire) = lonely_with(BridgeBuilder::new(Role::Daemon).on_error(on_error));
        let pending = bridge.request("echo", json!({}), None);
        wire.wait_for_frames(1).await;
        wire.send(frame("res", "r-1", "echo", json!({ "wrong": "namespace" })))
            .await;
        wire.send(frame("res", "n-1", "wrong.echo", json!({ "wrong": "op" })))
            .await;
        wait_for(|| errors.borrow().len() == 2).await;
        wire.send(frame("res", "n-1", "echo", json!({ "ok": true })))
            .await;
        assert_eq!(within(pending).await, Ok(json!({ "ok": true })));
        assert!(errors.borrow()[0].contains("wrong namespace"));
        assert!(errors.borrow()[1].contains("wrong.echo"));
    });
}

#[test]
fn resolves_ok_false_error_deadline_past_the_deadline() {
    run(async {
        let (bridge, _wire) = lonely();
        let started = Instant::now();
        let answer =
            within(bridge.request("never", json!({}), Some(Duration::from_millis(20)))).await;
        assert_eq!(answer, Ok(json!({ "ok": false, "error": "deadline" })));
        let waited = started.elapsed();
        assert!(
            waited >= Duration::from_millis(20) && waited < Duration::from_millis(22),
            "{waited:?}"
        );
    });
}

#[test]
fn ignores_a_response_arriving_after_its_deadline() {
    run(async {
        let (errors, on_error) = collector();
        let (bridge, mut wire) = lonely_with(BridgeBuilder::new(Role::Daemon).on_error(on_error));
        let answer =
            within(bridge.request("never", json!({}), Some(Duration::from_millis(20)))).await;
        assert_eq!(answer, Ok(json!({ "ok": false, "error": "deadline" })));
        wire.send(frame("res", "n-1", "never", json!({ "late": true })))
            .await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(errors.borrow().is_empty());
        assert!(!bridge.closed());

        let pending = bridge.request("echo", json!({}), None);
        wire.wait_for_frames(2).await;
        wire.send(frame("res", "n-2", "echo", json!({ "ok": true })))
            .await;
        assert_eq!(within(pending).await, Ok(json!({ "ok": true })));
    });
}

#[test]
fn an_answer_past_its_deadline_is_no_answer_though_nobody_awaited_the_request_meanwhile() {
    run(async {
        let (bridge, mut wire) = lonely();
        let pending = bridge.request("slow", json!({}), Some(Duration::from_millis(10)));
        wire.wait_for_frames(1).await;
        // The deadline passes while the bridge runs and nobody looks at the
        // request; then its answer comes, and is read.
        tokio::time::sleep(Duration::from_millis(30)).await;
        wire.send(frame("res", "n-1", "slow", json!({ "late": true })))
            .await;
        quiet().await;
        assert_eq!(
            within(pending).await,
            Ok(json!({ "ok": false, "error": "deadline" }))
        );
    });
}

#[test]
fn an_answer_in_time_is_the_answer_though_the_request_is_awaited_past_its_deadline() {
    run(async {
        let (bridge, mut wire) = lonely();
        let pending = bridge.request("quick", json!({}), Some(Duration::from_millis(10)));
        wire.wait_for_frames(1).await;
        wire.send(frame("res", "n-1", "quick", json!({ "ok": true })))
            .await;
        quiet().await;
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert_eq!(within(pending).await, Ok(json!({ "ok": true })));
    });
}

#[test]
fn a_request_given_up_before_its_answer_is_forgotten_and_the_answer_dropped() {
    run(async {
        let (errors, on_error) = collector();
        let (bridge, mut wire) = lonely_with(BridgeBuilder::new(Role::Daemon).on_error(on_error));
        drop(bridge.request("echo", json!({}), None));
        wire.wait_for_frames(1).await;
        wire.send(frame("res", "n-1", "echo", json!({ "ok": true })))
            .await;
        quiet().await;
        assert!(errors.borrow().is_empty());
        assert!(!bridge.closed());
    });
}

#[test]
fn a_request_stops_waiting_once_answered_given_up_or_past_its_deadline() {
    run(async {
        let (bridge, mut wire) = lonely();
        let answered = bridge.request("a", json!({}), None);
        let given_up = bridge.request("b", json!({}), None);
        let late = bridge.request("c", json!({}), Some(Duration::from_millis(10)));
        assert_eq!(bridge.inner.waiting(), 3);

        wire.send(frame("res", "n-1", "a", json!({}))).await;
        within(answered).await.unwrap();
        assert_eq!(bridge.inner.waiting(), 2);
        drop(given_up);
        assert_eq!(bridge.inner.waiting(), 1);
        within(late).await.unwrap();
        assert_eq!(bridge.inner.waiting(), 0);
    });
}

#[test]
fn a_deadline_too_far_for_the_clock_is_no_deadline_and_the_answer_still_comes() {
    run(async {
        let (bridge, mut wire) = lonely();
        let pending = bridge.request("echo", json!({}), Some(Duration::MAX));
        wire.wait_for_frames(1).await;
        wire.send(frame("res", "n-1", "echo", json!({ "ok": true })))
            .await;
        assert_eq!(within(pending).await, Ok(json!({ "ok": true })));
    });
}

#[test]
fn a_deadline_of_nothing_is_past_at_once() {
    run(async {
        let (bridge, _wire) = lonely();
        assert_eq!(
            within(bridge.request("never", json!({}), Some(Duration::ZERO))).await,
            Ok(json!({ "ok": false, "error": "deadline" }))
        );
    });
}

#[test]
fn the_deadline_counts_from_the_call_and_not_from_the_first_poll() {
    run(async {
        let (bridge, _wire) = lonely();
        let started = Instant::now();
        let pending = bridge.request("never", json!({}), Some(Duration::from_millis(100)));
        tokio::time::sleep(Duration::from_millis(80)).await;
        assert_eq!(
            within(pending).await,
            Ok(json!({ "ok": false, "error": "deadline" }))
        );
        let waited = started.elapsed();
        assert!(waited < Duration::from_millis(105), "{waited:?}");
    });
}

#[test]
fn settles_an_unanswered_request_on_the_default_deadline_and_keeps_working() {
    run(async {
        let (bridge, mut wire) = lonely_with(
            BridgeBuilder::new(Role::Daemon)
                .max_frame_bytes(100)
                .default_deadline(Duration::from_millis(50)),
        );
        let started = Instant::now();
        let pending = bridge.request("hangs", json!({}), None);
        // An oversized, fragmented response: discarded through its newline, so
        // the request never settles from the wire and the default deadline does.
        let response = frame("res", "n-1", "hangs", json!({ "text": "x".repeat(200) })).to_string();
        wire.send_text(&response[..60]).await;
        quiet().await;
        wire.send_text(&response[60..]).await;
        assert_eq!(
            within(pending).await,
            Ok(json!({ "ok": false, "error": "deadline" }))
        );
        let waited = started.elapsed();
        assert!(
            waited >= Duration::from_millis(50) && waited < Duration::from_millis(52),
            "{waited:?}"
        );
        wire.send_text("\n").await;
        let next = bridge.request("ping", json!({}), None);
        wire.wait_for_frames(2).await;
        wire.send(frame("res", "n-2", "ping", json!({ "ok": true })))
            .await;
        assert_eq!(within(next).await, Ok(json!({ "ok": true })));
    });
}

#[test]
fn defaults_maxframebytes_to_1_mib_and_the_request_deadline_to_30_s() {
    assert_eq!(DEFAULT_MAX_FRAME_BYTES, 1024 * 1024);
    assert_eq!(DEFAULT_DEADLINE, Duration::from_secs(30));
    run(async {
        let (bridge, wire) = lonely();

        // A frame of exactly the limit is written, one byte over is refused.
        let bare = frame("req", "n-1", "big", json!(""));
        let overhead = bare.to_string().len();
        let at_the_limit = json!("x".repeat(DEFAULT_MAX_FRAME_BYTES - overhead));
        let over = json!("x".repeat(DEFAULT_MAX_FRAME_BYTES - overhead + 1));
        let started = Instant::now();
        let waiting = bridge.request("big", at_the_limit, None);
        assert_eq!(
            within(bridge.request("big", over, None)).await,
            Ok(json!({ "ok": false, "error": "too-large" }))
        );
        wait_for(|| wire.text().len() == DEFAULT_MAX_FRAME_BYTES + 1).await;

        // No deadline given: the request waits 30 s.
        assert_eq!(
            within(waiting).await,
            Ok(json!({ "ok": false, "error": "deadline" }))
        );
        let waited = started.elapsed();
        assert!(
            waited >= DEFAULT_DEADLINE && waited < DEFAULT_DEADLINE + Duration::from_millis(5),
            "{waited:?}"
        );
    });
}

//! `Bridge transport failure`: what a bridge does when its output breaks, and
//! what it does not do when one request alone fails.

use std::cell::Cell;
use std::rc::Rc;

use cf_proto::bridge::Role;
use serde_json::json;

use super::wire::{collector, lonely_over, pair, pair_of, quiet, run, wait_for, within};
use crate::local::BridgeBuilder;
use crate::BridgeError;

fn pipe_broke() -> BridgeError {
    BridgeError::Io("pipe broke".to_owned())
}

#[test]
fn rejects_pending_requests_when_the_output_errors_with_input_still_open() {
    run(async {
        let (errors, on_error) = collector();
        let (fatal, on_fatal) = collector();
        let broken = Rc::new(Cell::new(false));
        let builder = BridgeBuilder::new(Role::Daemon)
            .on_error(on_error)
            .on_fatal(on_fatal);
        let (bridge, _wire) = lonely_over(builder, &broken);
        let pending = bridge.request("hangs", json!({}), None);
        quiet().await;
        // The peer is still there, writing nothing: it is the output that
        // breaks, and the next frame written finds it out.
        broken.set(true);
        assert!(bridge.event("tick", json!({})));
        assert_eq!(within(pending).await, Err(pipe_broke()));
        assert!(bridge.closed());
        assert_eq!(*errors.borrow(), ["bridge I/O error: pipe broke"]);
        assert_eq!(*fatal.borrow(), ["bridge I/O error: pipe broke"]);
        assert_eq!(
            within(bridge.request("late", json!({}), None)).await,
            Err(pipe_broke())
        );
    });
}

#[test]
fn rejects_a_request_whose_write_throws_synchronously() {
    run(async {
        let broken = Rc::new(Cell::new(true));
        let (bridge, _wire) = lonely_over(BridgeBuilder::new(Role::Daemon), &broken);
        // The write is the writer's, a moment after the request is made: the
        // request is refused with what broke, as if it had failed on the spot.
        assert_eq!(
            within(bridge.request("x", json!({}), None)).await,
            Err(pipe_broke())
        );
        assert!(bridge.closed());
    });
}

#[test]
fn a_failure_is_told_once_and_the_fatal_handler_is_told_the_same() {
    run(async {
        let (errors, on_error) = collector();
        let (fatal, on_fatal) = collector();
        let broken = Rc::new(Cell::new(true));
        let builder = BridgeBuilder::new(Role::Daemon)
            .on_error(on_error)
            .on_fatal(on_fatal);
        let (bridge, _wire) = lonely_over(builder, &broken);
        bridge.event("one", json!({}));
        bridge.event("two", json!({}));
        wait_for(|| bridge.closed()).await;
        quiet().await;
        assert_eq!((errors.borrow().len(), fatal.borrow().len()), (1, 1));
    });
}

#[test]
fn a_failing_handler_fails_its_request_and_neither_end_of_the_transport() {
    run(async {
        let (a, b) = pair();
        b.on("circular", |_, _| async {
            Err("this one failed".to_owned())
        });
        assert_eq!(
            within(a.request("circular", json!({}), None)).await,
            Ok(json!({ "ok": false, "error": "this one failed" }))
        );
        // A per-request failure, not a transport failure: both sides stay up.
        assert!(!a.closed() && !b.closed());
        b.on("ping", |_, _| async { Ok(json!({ "ok": true })) });
        assert_eq!(
            within(a.request("ping", json!({}), None)).await,
            Ok(json!({ "ok": true }))
        );
    });
}

#[test]
fn a_fatal_failure_ends_the_output_so_the_peer_observes_eof() {
    run(async {
        let (errors, on_error) = collector();
        let (fatal, on_fatal) = collector();
        // `a` takes any frame; `b` takes 60 bytes, which is more than a
        // request for `hangs` and less than the bounded refusal it would
        // answer an oversized request with.
        let (a, b) = pair_of(
            BridgeBuilder::new(Role::Daemon),
            BridgeBuilder::new(Role::Host)
                .max_frame_bytes(60)
                .on_error(on_error)
                .on_fatal(on_fatal),
        );
        b.on("hangs", |_, _| std::future::pending());
        let pending = a.request("hangs", json!({}), None);
        quiet().await;
        // An oversized request into `b`, whose bounded refusal cannot fit
        // either: `b` fails fatally. Nothing here touches the input of `a`: its
        // end must arrive as the consequence of `b` ending its output.
        let oversized = a.request("big", json!({ "text": "x".repeat(200) }), None);
        assert_eq!(within(pending).await, Err(BridgeError::Eof));
        assert_eq!(within(oversized).await, Err(BridgeError::Eof));
        assert!(a.closed() && b.closed());
        assert!(errors.borrow()[0].contains("cannot fit maxFrameBytes"));
        assert!(fatal.borrow()[0].contains("cannot fit maxFrameBytes"));
    });
}

#[test]
fn a_request_made_after_the_failure_is_refused_with_its_cause_and_not_with_eof() {
    run(async {
        let broken = Rc::new(Cell::new(true));
        let (bridge, _wire) = lonely_over(BridgeBuilder::new(Role::Daemon), &broken);
        bridge.event("tick", json!({}));
        wait_for(|| bridge.closed()).await;
        for _ in 0..2 {
            assert_eq!(
                within(bridge.request("late", json!({}), None)).await,
                Err(pipe_broke())
            );
        }
    });
}

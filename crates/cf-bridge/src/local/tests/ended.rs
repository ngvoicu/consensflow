//! `Bridge::ended`: the one place that says a bridge ended and why, which the
//! daemon stops on. The end of the input is told to no callback, so a bridge
//! that ended must be heard whenever it is asked, once or by many.

use std::cell::Cell;
use std::future::Future;
use std::pin::pin;
use std::rc::Rc;
use std::task::{Context, Waker};

use cf_proto::bridge::Role;
use serde_json::json;
use tokio::io::duplex;

use super::wire::{lonely, lonely_over, quiet, run, within};
use crate::local::{BridgeBuilder, Ended};
use crate::BridgeError;

/// Whether `future` is done at a poll now, which only the clock and the
/// other tasks could change.
fn done<F: Future>(future: F) -> bool {
    pin!(future)
        .poll(&mut Context::from_waker(Waker::noop()))
        .is_ready()
}

#[test]
fn a_bridge_that_is_running_has_not_ended() {
    run(async {
        let (bridge, _wire) = lonely();
        quiet().await;
        assert!(!done(bridge.ended()));
    });
}

#[test]
fn the_end_of_the_input_is_the_input() {
    run(async {
        let (bridge, mut wire) = lonely();
        let ended = tokio::task::spawn_local(bridge.ended());
        quiet().await;
        assert!(!ended.is_finished());
        wire.end();
        assert_eq!(within(ended).await.unwrap(), Ended::Input);
        assert!(bridge.closed());
    });
}

#[test]
fn closing_it_is_closed_and_the_output_ends_with_it() {
    run(async {
        let (bridge, wire) = lonely();
        bridge.close();
        assert_eq!(within(bridge.ended()).await, Ended::Closed);
        quiet().await;
        assert!(wire.output_ended());
    });
}

#[test]
fn a_transport_that_fails_is_the_failure_with_its_error_and_the_fatal_hears_it_too() {
    run(async {
        let told: Rc<Cell<u32>> = Rc::default();
        let counted = Rc::clone(&told);
        let broken = Rc::new(Cell::new(false));
        let (bridge, _wire) = lonely_over(
            BridgeBuilder::new(Role::Daemon).on_fatal(move |_| counted.set(counted.get() + 1)),
            &broken,
        );
        broken.set(true);
        assert!(bridge.event("late", json!({})), "the frame is queued");
        let Ended::Failed(BridgeError::Io(words)) = within(bridge.ended()).await else {
            panic!("not a failure of the transport");
        };
        assert!(words.contains("pipe broke"), "{words}");
        assert_eq!(told.get(), 1);
    });
}

#[test]
fn the_first_way_it_ended_is_the_one_it_says() {
    run(async {
        let broken = Rc::new(Cell::new(false));
        let (bridge, mut wire) = lonely_over(BridgeBuilder::new(Role::Daemon), &broken);
        wire.end();
        assert_eq!(within(bridge.ended()).await, Ended::Input);
        // A close and a failure after it say nothing new.
        bridge.close();
        bridge.inner.fail(BridgeError::Io("late".to_owned()));
        assert_eq!(within(bridge.ended()).await, Ended::Input);
    });
}

#[test]
fn an_end_that_came_is_told_at_once_to_everyone_who_asks_after_it() {
    run(async {
        let (bridge, mut wire) = lonely();
        let before = tokio::task::spawn_local(bridge.ended());
        wire.end();
        assert_eq!(within(before).await.unwrap(), Ended::Input);
        assert!(
            done(bridge.ended()),
            "asked after, answered without waiting"
        );
        let (one, other) = (bridge.ended(), bridge.ended());
        assert_eq!(within(one).await, Ended::Input);
        assert_eq!(within(other).await, Ended::Input);
    });
}

#[test]
fn what_asks_owns_no_part_of_the_bridge_so_a_bridge_that_is_gone_ended_as_closed() {
    run(async {
        let (to_bridge, input) = duplex(1024);
        let (output, _from_bridge) = duplex(1024);
        let (bridge, connection) = BridgeBuilder::new(Role::Daemon).connect(input, output);
        let ended = bridge.ended();
        drop(connection);
        drop(bridge);
        assert_eq!(within(ended).await, Ended::Closed);
        drop(to_bridge);
    });
}

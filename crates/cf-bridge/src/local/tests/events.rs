//! `Bridge events`: what the peer tells and is not answered, and what this end
//! tells the peer.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use cf_proto::bridge::Role;
use serde_json::{json, Value};

use super::wire::{frame, lonely, lonely_over, pair, quiet, run, wait_for};
use crate::local::{BridgeBuilder, Subscription};

type Seen = Rc<RefCell<Vec<Value>>>;

fn recorder(seen: &Seen) -> impl Fn(&Value) + 'static {
    let seen = Rc::clone(seen);
    move |body| seen.borrow_mut().push(body.clone())
}

#[test]
fn delivers_outgoing_events_to_on_event_handlers() {
    run(async {
        let (a, b) = pair();
        let seen = Seen::default();
        b.on_event("tick", recorder(&seen));
        assert!(a.event("tick", json!({ "n": 1 })));
        wait_for(|| seen.borrow().len() == 1).await;
        assert_eq!(*seen.borrow(), [json!({ "n": 1 })]);
    });
}

#[test]
fn stops_calling_an_event_handler_after_unsubscribe() {
    run(async {
        let (a, b) = pair();
        let seen = Seen::default();
        let off = b.on_event("tick", recorder(&seen));
        a.event("tick", json!({ "n": 1 }));
        wait_for(|| seen.borrow().len() == 1).await;
        off.off();
        a.event("tick", json!({ "n": 2 }));
        quiet().await;
        assert_eq!(*seen.borrow(), [json!({ "n": 1 })]);
    });
}

#[test]
fn does_not_skip_subscribers_when_a_handler_unsubscribes_mid_event() {
    run(async {
        let (a, b) = pair();
        let seen_a = Seen::default();
        let seen_b = Seen::default();
        let own: Rc<RefCell<Option<Subscription>>> = Rc::default();
        let (record_a, slot) = (recorder(&seen_a), Rc::clone(&own));
        let off_a = b.on_event("tick", move |body| {
            record_a(body);
            let mine = slot.borrow_mut().take();
            if let Some(mine) = mine {
                mine.off();
            }
        });
        *own.borrow_mut() = Some(off_a);
        b.on_event("tick", recorder(&seen_b));
        a.event("tick", json!({ "n": 1 }));
        a.event("tick", json!({ "n": 2 }));
        wait_for(|| seen_b.borrow().len() == 2).await;
        assert_eq!(*seen_a.borrow(), [json!({ "n": 1 })]);
        assert_eq!(*seen_b.borrow(), [json!({ "n": 1 }), json!({ "n": 2 })]);
    });
}

#[test]
fn calls_every_handler_of_an_event_in_the_order_they_were_added() {
    run(async {
        let (a, b) = pair();
        let calls = Rc::new(RefCell::new(Vec::new()));
        for name in ["first", "second", "third"] {
            let calls = Rc::clone(&calls);
            b.on_event("tick", move |_| calls.borrow_mut().push(name));
        }
        a.event("tick", json!({}));
        wait_for(|| calls.borrow().len() == 3).await;
        assert_eq!(*calls.borrow(), ["first", "second", "third"]);
    });
}

#[test]
fn hands_events_to_their_handlers_in_the_order_they_arrived() {
    run(async {
        let (a, b) = pair();
        let seen = Seen::default();
        b.on_event("tick", recorder(&seen));
        for n in 0..200 {
            a.event("tick", json!({ "n": n }));
        }
        wait_for(|| seen.borrow().len() == 200).await;
        let order: Vec<i64> = seen
            .borrow()
            .iter()
            .filter_map(|body| body["n"].as_i64())
            .collect();
        assert_eq!(order, (0..200).collect::<Vec<_>>());
    });
}

#[test]
fn an_event_nobody_handles_is_dropped_without_a_word() {
    run(async {
        let (a, b) = pair();
        assert!(a.event("nobody.listens", json!({})));
        quiet().await;
        assert!(!a.closed() && !b.closed());
    });
}

#[test]
fn refuses_events_after_close() {
    run(async {
        let (bridge, _wire) = lonely();
        bridge.close();
        assert!(!bridge.event("tick", json!({})));
        assert!(bridge.closed());
    });
}

#[test]
fn returns_false_when_the_event_write_itself_fails() {
    run(async {
        let broken = Rc::new(Cell::new(true));
        let (bridge, _wire) = lonely_over(BridgeBuilder::new(Role::Daemon), &broken);
        // The frame is queued; it is the writer that finds the output broken,
        // and the events after it are refused.
        assert!(bridge.event("tick", json!({})));
        wait_for(|| bridge.closed()).await;
        assert!(!bridge.event("tick", json!({})));
    });
}

#[test]
fn an_event_the_peer_sends_is_heard_in_the_same_frame_order_as_it_was_written() {
    run(async {
        let (bridge, mut wire) = lonely();
        let seen = Seen::default();
        bridge.on_event("tick", recorder(&seen));
        let both = format!(
            "{}\n{}\n",
            frame("evt", "r-1", "tick", json!({ "n": 1 })),
            frame("evt", "r-2", "tick", json!({ "n": 2 }))
        );
        wire.send_text(&both).await;
        wait_for(|| seen.borrow().len() == 2).await;
        assert_eq!(*seen.borrow(), [json!({ "n": 1 }), json!({ "n": 2 })]);
        assert_eq!(wire.text(), "", "an event is never answered");
    });
}

#[test]
fn a_host_end_hears_the_daemons_events_and_the_daemon_the_hosts() {
    run(async {
        let (a, b) = pair();
        let (heard_by_a, heard_by_b) = (Seen::default(), Seen::default());
        a.on_event("from.host", recorder(&heard_by_a));
        b.on_event("from.daemon", recorder(&heard_by_b));
        b.event("from.host", json!(1));
        a.event("from.daemon", json!(2));
        wait_for(|| !heard_by_a.borrow().is_empty() && !heard_by_b.borrow().is_empty()).await;
        assert_eq!(*heard_by_a.borrow(), [json!(1)]);
        assert_eq!(*heard_by_b.borrow(), [json!(2)]);
    });
}

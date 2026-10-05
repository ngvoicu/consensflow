//! `Bridge dispatch never blocks on a handler`, and where a handler starts: on
//! the reader, before the next frame.

use std::cell::RefCell;
use std::rc::Rc;

use serde_json::json;
use tokio::sync::oneshot;

use super::wire::{frame, lonely, pair, quiet, run, wait_for, within};

type Log = Rc<RefCell<Vec<&'static str>>>;

fn logging(log: &Log, entry: &'static str) {
    log.borrow_mut().push(entry);
}

#[test]
fn serves_a_nested_reverse_request_while_the_outer_handler_is_awaiting() {
    run(async {
        let (a, b) = pair();
        a.on("inner", |_, _| async { Ok(json!({ "v": 42 })) });
        b.on("outer", |b, _| async move {
            let got = b
                .request("inner", json!({}), None)
                .await
                .map_err(|error| error.to_string())?;
            Ok(json!({ "got": got }))
        });
        assert_eq!(
            within(a.request("outer", json!({}), None)).await,
            Ok(json!({ "got": { "v": 42 } }))
        );
    });
}

#[test]
fn dispatches_other_frames_while_one_handler_is_still_pending() {
    run(async {
        let (a, b) = pair();
        let (release, gate) = oneshot::channel::<()>();
        let gate = RefCell::new(Some(gate));
        b.on("slow", move |_, _| {
            let gate = gate.borrow_mut().take();
            async move {
                if let Some(gate) = gate {
                    let _ = gate.await;
                }
                Ok(json!({ "ok": true }))
            }
        });
        b.on("fast", |_, _| async { Ok(json!({ "ok": "fast" })) });
        let slow = a.request("slow", json!({}), None);
        assert_eq!(
            within(a.request("fast", json!({}), None)).await,
            Ok(json!({ "ok": "fast" }))
        );
        release.send(()).unwrap();
        assert_eq!(within(slow).await, Ok(json!({ "ok": true })));
    });
}

#[test]
fn a_handler_runs_to_its_first_wait_before_the_next_frame_is_read() {
    run(async {
        let (bridge, mut wire) = lonely();
        let log = Log::default();
        let (slow_log, tick_log) = (Rc::clone(&log), Rc::clone(&log));
        bridge.on("slow", move |_, _| {
            let log = Rc::clone(&slow_log);
            async move {
                logging(&log, "slow, before its first wait");
                tokio::task::yield_now().await;
                logging(&log, "slow, after it");
                Ok(json!({}))
            }
        });
        bridge.on_event("tick", move |_| logging(&tick_log, "the event after it"));
        let both = format!(
            "{}\n{}\n",
            frame("req", "r-1", "slow", json!({})),
            frame("evt", "r-2", "tick", json!({}))
        );
        wire.send_text(&both).await;
        wire.wait_for_frames(1).await;
        assert_eq!(
            *log.borrow(),
            [
                "slow, before its first wait",
                "the event after it",
                "slow, after it"
            ]
        );
    });
}

#[test]
fn a_handler_that_is_done_at_once_answers_before_the_next_frame_is_dispatched() {
    run(async {
        let (bridge, mut wire) = lonely();
        bridge.on("fast", |_, _| async { Ok(json!({ "ok": true })) });
        let both = format!(
            "{}\n{}\n",
            frame("req", "r-1", "fast", json!({})),
            frame("req", "r-2", "nobody", json!({}))
        );
        wire.send_text(&both).await;
        let frames = wire.wait_for_frames(2).await;
        assert_eq!(frames[0]["id"], "r-1");
        assert_eq!(frames[1]["id"], "r-2");
    });
}

#[test]
fn a_handler_that_waits_is_answered_when_it_is_done_and_the_others_were_answered_before() {
    run(async {
        let (bridge, mut wire) = lonely();
        let (release, gate) = oneshot::channel::<()>();
        let gate = RefCell::new(Some(gate));
        bridge.on("slow", move |_, _| {
            let gate = gate.borrow_mut().take();
            async move {
                if let Some(gate) = gate {
                    let _ = gate.await;
                }
                Ok(json!("slow"))
            }
        });
        let both = format!(
            "{}\n{}\n",
            frame("req", "r-1", "slow", json!({})),
            frame("req", "r-2", "nobody", json!({}))
        );
        wire.send_text(&both).await;
        assert_eq!(wire.wait_for_frames(1).await[0]["id"], "r-2");
        release.send(()).unwrap();
        wait_for(|| wire.frames().len() == 2).await;
        assert_eq!(wire.frames()[1], frame("res", "r-1", "slow", json!("slow")));
    });
}

#[test]
fn a_handler_still_running_when_the_input_ends_is_not_answered() {
    run(async {
        let (bridge, mut wire) = lonely();
        let (release, gate) = oneshot::channel::<()>();
        let gate = RefCell::new(Some(gate));
        bridge.on("slow", move |_, _| {
            let gate = gate.borrow_mut().take();
            async move {
                if let Some(gate) = gate {
                    let _ = gate.await;
                }
                Ok(json!("late"))
            }
        });
        wire.send(frame("req", "r-1", "slow", json!({}))).await;
        quiet().await;
        wire.end();
        wait_for(|| bridge.closed()).await;
        release.send(()).unwrap();
        quiet().await;
        assert_eq!(wire.text(), "");
    });
}

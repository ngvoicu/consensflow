//! The engine's turns and the bridge's frames, as Node ordered them: the
//! frames of one read are handled together, and what they woke, a chain of
//! the engine's turns included, is run to its end before the next read, which
//! is where the daemon's bridge drains the executor ([`daemon_bridge`]).

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use cf_bridge::local::{Bridge, Subscription};
use cf_engine::runtime::{next_turn, LocalWork};
use serde_json::json;
use tokio::task::LocalSet;

use crate::host::watch_exits;
use crate::seams::DaemonSpawn;
use crate::testing::{bridge_pair_over, worked, Worked};

/// What the daemon's end heard, and what the engine's work did, in order.
type Order = Rc<RefCell<Vec<String>>>;

/// The daemon's end hears a `ping`, and a `pane.exit` whose work is a chain
/// of five turns, each said in `order` as it is handled.
fn hearing(daemon: &Bridge, spawn: &Rc<DaemonSpawn>, order: &Order) -> Subscription {
    let pinged = Rc::clone(order);
    daemon.on("ping", move |_, _| {
        pinged.borrow_mut().push("ping".to_owned());
        async { Ok(json!({ "ok": true })) }
    });
    let exited = Rc::clone(order);
    watch_exits(daemon, Rc::clone(spawn), move |pane| {
        exited.borrow_mut().push(format!("exit {}", pane.id));
        let chain = Rc::clone(&exited);
        let work: LocalWork = Box::pin(async move {
            for turn in 0..5 {
                chain.borrow_mut().push(format!("turn {turn}"));
                next_turn().await;
            }
        });
        Some(work)
    })
}

const CHAIN: [&str; 6] = [
    "exit p1-chief",
    "turn 0",
    "turn 1",
    "turn 2",
    "turn 3",
    "turn 4",
];

#[tokio::test]
async fn a_chain_of_the_engines_turns_a_frame_began_ends_before_the_next_frame_is_handled() {
    LocalSet::new()
        .run_until(async {
            let Worked { spawn, .. } = worked();
            let (daemon, app) = bridge_pair_over(&spawn);
            let order = Order::default();
            let _hearing = hearing(&daemon, &spawn, &order);
            app.event("pane.exit", json!({ "id": "p1-chief", "generation": 1 }));
            // A frame too long for one read: the daemon reads it after the
            // read that held the exit, which the next read finds waiting.
            let padding = "x".repeat(70_000);
            app.request(
                "ping",
                json!({ "padding": padding }),
                Some(Duration::from_secs(5)),
            )
            .await
            .unwrap();
            let mut expected = CHAIN.to_vec();
            expected.push("ping");
            assert_eq!(*order.borrow(), expected);
        })
        .await;
}

#[tokio::test]
async fn the_frames_of_one_read_are_all_handled_before_the_work_they_woke_as_node_handled_a_data_event(
) {
    LocalSet::new()
        .run_until(async {
            let Worked { spawn, .. } = worked();
            let (daemon, app) = bridge_pair_over(&spawn);
            let order = Order::default();
            let _hearing = hearing(&daemon, &spawn, &order);
            // Written together, the two frames are one read.
            app.event("pane.exit", json!({ "id": "p1-chief", "generation": 1 }));
            app.request("ping", json!({}), Some(Duration::from_secs(5)))
                .await
                .unwrap();
            assert_eq!(
                *order.borrow(),
                [
                    "exit p1-chief",
                    "ping",
                    "turn 0",
                    "turn 1",
                    "turn 2",
                    "turn 3",
                    "turn 4"
                ]
            );
        })
        .await;
}

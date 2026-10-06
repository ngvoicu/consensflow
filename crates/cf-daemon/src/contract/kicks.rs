//! The pass loop's kick. In Node a kick was `setImmediate(run)`: the pass
//! began from a later turn of the event loop, after the callbacks the turn
//! already held and the microtasks they began, and never inside the call that
//! kicked, nor inside the microtasks that call was in. A handler kicks the
//! loop from the engine's work, which the executor runs in a drain: a pass
//! begun there would see what the handler had written so far and not what its
//! chain still writes, and an API's answer would be queued after the pass's
//! first effects.
//!
//! The loop's own end of a pass is such a place too: a pass that was kicked
//! while it ran is followed by one more, which begins from a later turn and not
//! inside the drain that ended the first.

use std::cell::RefCell;
use std::rc::Rc;

use cf_engine::runtime::next_turn;
use cf_ledger::NewNote;
use tokio::sync::Notify;

use super::rig::{Pieces, Rig, Transport};
use super::{chain, scene, settle, Order, TURNS};
use crate::console::Console;
use crate::pass::{Pass, PassLoop};
use crate::testing::Said;

/// The pass that is the engine's own: the dispatcher's, as the daemon runs it.
fn engine_pass(rig: &Rig) -> Pass {
    let dispatcher = Rc::clone(&rig.dispatcher);
    Box::new(move || {
        let dispatcher = Rc::clone(&dispatcher);
        Box::pin(async move { dispatcher.pass().await.map_err(|failed| failed.to_string()) })
    })
}

#[tokio::test(start_paused = true)]
async fn a_kick_made_inside_a_drain_begins_its_pass_after_the_drain_and_the_chain_in_it() {
    scene(async {
        let mut rig = Rig::new().await;
        let project = rig.open_project(&["zeus"]).await;
        let said = Said::default();
        let passes = PassLoop::start(
            engine_pass(&rig),
            Rc::clone(&rig.pieces.spawn),
            Rc::new(Console::to(said, || {})),
        );
        // A handler's work: it writes a task and kicks the loop, goes on for
        // a chain of turns, and then writes a note: all of it in one drain.
        let (ledger, kicking) = (Rc::clone(&rig.ledger), passes.clone());
        rig.pieces.spawn.apart("a handler failed", async move {
            ledger
                .borrow_mut()
                .create_task(
                    project.id,
                    &cf_ledger::NewTask {
                        from: "chief".to_owned(),
                        to: Some("zeus".to_owned()),
                        body: "Parser".to_owned(),
                        ..cf_ledger::NewTask::default()
                    },
                )
                .expect("a task");
            kicking.kick();
            for _ in 0..TURNS {
                next_turn().await;
            }
            ledger
                .borrow_mut()
                .note(
                    project.id,
                    &NewNote {
                        from: None,
                        to: "chief".to_owned(),
                        body: "the handler's chain ended".to_owned(),
                        task: None,
                    },
                )
                .expect("a note");
        });
        rig.pieces.spawn.drain();
        let kinds =
            |rig: &Rig| -> Vec<String> { rig.events().into_iter().map(|(kind, _)| kind).collect() };
        assert_eq!(
            kinds(&rig),
            ["task.created", "message.sent"],
            "the pass has not begun: the drain ended with the chain, whole"
        );
        // From the next turn of the loop the pass begins, and sees the task.
        rig.quiet().await;
        let kinds = kinds(&rig);
        assert_eq!(&kinds[..2], ["task.created", "message.sent"]);
        assert!(
            kinds[2..].iter().any(|kind| kind == "delivery.begun"),
            "the pass ran after the handler's chain: {kinds:?}"
        );
        passes.stop().await;
    })
    .await;
}

#[tokio::test(start_paused = true)]
async fn a_pass_kicked_while_it_ran_is_followed_by_one_that_begins_after_the_drain_that_ended_it() {
    scene(async {
        let pieces = Pieces::new(Transport::Memory).await;
        let order = Order::default();
        let gate = Rc::new(Notify::new());
        let passes_run = Rc::new(RefCell::new(0_u32));
        let (said, opened, count) = (Rc::clone(&order), Rc::clone(&gate), Rc::clone(&passes_run));
        let passes = PassLoop::start(
            Box::new(move || {
                let (said, opened, count) =
                    (Rc::clone(&said), Rc::clone(&opened), Rc::clone(&count));
                Box::pin(async move {
                    *count.borrow_mut() += 1;
                    let this = *count.borrow();
                    said.borrow_mut().push(format!("pass {this} begins"));
                    if this == 1 {
                        opened.notified().await;
                    }
                    said.borrow_mut().push(format!("pass {this} ends"));
                    Ok(())
                })
            }),
            Rc::clone(&pieces.spawn),
            Rc::new(Console::to(Said::default(), || {})),
        );
        passes.kick();
        settle().await;
        // Kicked while the first pass runs.
        passes.kick();
        settle().await;
        assert_eq!(*order.borrow(), ["pass 1 begins"]);
        // The first pass ends inside a drain, at the first turn of a chain
        // that goes on after it.
        let (letting, chaining) = (Rc::clone(&gate), Rc::clone(&order));
        pieces.spawn.apart("a chain failed", async move {
            letting.notify_one();
            chain(chaining, "x", TURNS).await;
        });
        pieces.spawn.drain();
        assert_eq!(
            *order.borrow(),
            [
                "pass 1 begins",
                "x 0",
                "pass 1 ends",
                "x 1",
                "x 2",
                "x 3",
                "x 4"
            ],
            "the pass ended in the drain, and the next did not begin there"
        );
        settle().await;
        assert_eq!(
            order.borrow()[7..],
            ["pass 2 begins", "pass 2 ends"],
            "one more, from the next turn"
        );
        assert_eq!(*passes_run.borrow(), 2, "two kicks made one more");
        passes.stop().await;
    })
    .await;
}

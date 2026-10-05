//! A pass is the engine's work: begun where its timer or its kick came, its
//! first part done there, and what that part woke run to its end before
//! anything else gets a turn, as Node's microtasks ran before the next
//! callback of its event loop.

use std::cell::RefCell;
use std::future::pending;

use cf_engine::runtime::next_turn;
use tokio::sync::Notify;
use tokio::time::sleep;

use super::*;

type Order = Rc<RefCell<Vec<String>>>;

/// A pass whose first part wakes `next` (a task of tokio's own that says
/// "next callback" when it runs) and then begins a chain of five turns of the
/// engine's work, each said in `order`, and waits for good.
fn waking_a_chain(
    rig: &Rig,
    order: &Order,
    next: &Rc<Notify>,
) -> impl Fn() -> Pin<Box<dyn Future<Output = Result<(), String>>>> + 'static {
    let (spawn, order, next) = (Rc::clone(&rig.spawn), Rc::clone(order), Rc::clone(next));
    move || {
        let (spawn, order, next) = (Rc::clone(&spawn), Rc::clone(&order), Rc::clone(&next));
        Box::pin(async move {
            order.borrow_mut().push("pass".to_owned());
            // Woken before the chain is begun: the other callback is ahead of
            // the executor's driver, and goes first unless the loop drains.
            next.notify_one();
            let chain = Rc::clone(&order);
            spawn.apart("a chain failed", async move {
                for turn in 0..5 {
                    chain.borrow_mut().push(format!("turn {turn}"));
                    next_turn().await;
                }
            });
            pending::<()>().await;
            Ok(())
        })
    }
}

/// A task of tokio's own that waits to be woken, and says "next callback".
async fn next_callback(order: &Order) -> Rc<Notify> {
    let (woken, said) = (Rc::new(Notify::new()), Rc::clone(order));
    let waiting = Rc::clone(&woken);
    drop(tokio::task::spawn_local(async move {
        waiting.notified().await;
        said.borrow_mut().push("next callback".to_owned());
    }));
    turns().await;
    woken
}

const THE_CHAIN: [&str; 7] = [
    "pass",
    "turn 0",
    "turn 1",
    "turn 2",
    "turn 3",
    "turn 4",
    "next callback",
];

#[tokio::test(start_paused = true)]
async fn a_pass_a_kick_begins_does_its_first_part_there_and_what_it_woke_is_run_before_the_next_callback(
) {
    scene(async {
        let rig = rig();
        let order = Order::default();
        let next = next_callback(&order).await;
        let loop_ = rig.start(waking_a_chain(&rig, &order, &next));
        loop_.kick();
        turns().await;
        assert_eq!(*order.borrow(), THE_CHAIN);
        loop_.stop().await;
    })
    .await;
}

#[tokio::test(start_paused = true)]
async fn a_pass_a_timer_begins_does_its_first_part_there_and_what_it_woke_is_run_before_the_next_callback(
) {
    scene(async {
        let rig = rig();
        let order = Order::default();
        let next = next_callback(&order).await;
        let loop_ = rig.start(waking_a_chain(&rig, &order, &next));
        sleep(PASS + Duration::from_millis(1)).await;
        assert_eq!(*order.borrow(), THE_CHAIN);
        loop_.stop().await;
    })
    .await;
}

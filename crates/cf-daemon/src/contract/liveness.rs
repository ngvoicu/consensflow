//! The driver's liveness (`Executor::driver`, spawned by the daemon on its
//! local set). Work woken from outside a drain, by a thread that is not the
//! engine's, is run by the driver and by nothing else: if the wake is lost,
//! the work waits for ever, for the daemon has no other traffic to drain it
//! on. A worker thread's answer with nothing after it, and wakes that land as
//! a drain runs and ends, must both end in the work running.
//!
//! The second has no seam to make it happen at the moment it matters (a wake
//! between the drain's last look at its queue and its giving up its place:
//! the executor closes that window by looking once more with the drain let go
//! of, and a wake's push and check take longer than the drain's two steps
//! there, so no test can land in it). So it is crowded instead: a thread
//! streams wakes for a random few microseconds while the drain they start
//! runs, and the last of each stream must be taken, over many rounds, under a
//! deadline. A driver that is not woken, or that is not registered, fails it
//! in its first round; it is a regression guard for the protocol around the
//! window, not a proof of the window.

use std::cell::Cell;
use std::future::poll_fn;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Poll, Waker};
use std::time::{Duration, Instant};

use tokio::sync::oneshot;

use super::scene;
use crate::testing::{worked, Worked};

/// How long a lost wake is waited for: a driver that is alive answers in
/// microseconds, and the daemon's own tests take seconds.
const DEADLINE: Duration = Duration::from_secs(10);

#[tokio::test]
async fn an_answer_from_another_thread_with_no_traffic_after_it_is_run_by_the_driver() {
    scene(async {
        let Worked { spawn, .. } = worked();
        let (answer, answered) = oneshot::channel::<u8>();
        let done = Rc::new(tokio::sync::Notify::new());
        let (said, ended) = (Rc::new(Cell::new(0)), Rc::clone(&done));
        let kept = Rc::clone(&said);
        spawn.apart("a wait failed", async move {
            kept.set(answered.await.expect("the answer comes"));
            ended.notify_one();
        });
        spawn.drain();
        // The engine's thread is idle, with nothing to do and nothing coming:
        // the answer comes from a thread of its own, and is the only thing
        // that happens.
        let sender = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            answer.send(7).expect("the work waits for it");
        });
        tokio::time::timeout(DEADLINE, done.notified())
            .await
            .expect("the driver ran what a thread woke");
        assert_eq!(said.get(), 7);
        sender.join().expect("the thread ended");
    })
    .await;
}

#[tokio::test]
async fn wakes_streamed_from_another_thread_while_drains_run_and_end_are_never_lost() {
    const ROUNDS: usize = 5_000;
    scene(async {
        let Worked { spawn, .. } = worked();
        // One piece of work that waits for a flag, and takes it when it runs.
        let flag = Arc::new(AtomicBool::new(false));
        let waker: Arc<Mutex<Option<Waker>>> = Arc::default();
        let (taking, registering) = (Arc::clone(&flag), Arc::clone(&waker));
        spawn.apart("a wait failed", async move {
            loop {
                poll_fn(|context| {
                    *registering.lock().expect("the waker's slot") = Some(context.waker().clone());
                    if taking.swap(false, Ordering::SeqCst) {
                        Poll::Ready(())
                    } else {
                        Poll::Pending
                    }
                })
                .await;
            }
        });
        // Run once, so that it is waiting, its waker where the thread finds it.
        spawn.drain();
        let (told, finished) = oneshot::channel();
        let (setting, finding) = (Arc::clone(&flag), Arc::clone(&waker));
        let waking = std::thread::spawn(move || {
            let work = finding
                .lock()
                .expect("the waker's slot")
                .clone()
                .expect("the work registered its waker");
            let mut chance: u64 = 0x2545_f491_4f6c_dd1d;
            let mut random = move || {
                chance ^= chance << 13;
                chance ^= chance >> 7;
                chance ^= chance << 17;
                chance
            };
            for round in 0..ROUNDS {
                // A stream of wakes, each with its flag, for a few microseconds:
                // the drain the first one starts is running as most of them
                // land, and the stream ends at a moment of its own among the
                // drain's last, which is not the thread's to choose.
                let stream = Duration::from_nanos(random() % 40_000);
                let began = Instant::now();
                while began.elapsed() < stream {
                    setting.store(true, Ordering::SeqCst);
                    work.wake_by_ref();
                    for step in 0..random() % 40 {
                        std::hint::black_box(step);
                    }
                }
                // The last wake came with its flag: it is taken, or it was lost.
                setting.store(true, Ordering::SeqCst);
                work.wake_by_ref();
                let waited = Instant::now();
                let mut spins = 0_u32;
                while setting.load(Ordering::SeqCst) {
                    if waited.elapsed() > Duration::from_secs(5) {
                        let _ = told.send(Err(round));
                        return;
                    }
                    // A short spin, then the core is given up: on a machine
                    // with one, the engine's thread is waiting for it.
                    spins += 1;
                    if spins > 100 {
                        std::thread::yield_now();
                    }
                }
            }
            let _ = told.send(Ok(()));
        });
        let ended = tokio::time::timeout(DEADLINE, finished)
            .await
            .expect("the rounds ended")
            .expect("the thread told how they ended");
        assert_eq!(ended, Ok(()), "a wake was lost at this round");
        waking.join().expect("the thread ended");
    })
    .await;
}

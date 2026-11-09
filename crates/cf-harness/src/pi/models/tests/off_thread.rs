//! F4 of the second reading of T-1270: the read of Pi's folder is on a thread of its own. The
//! daemon runs one thread for every project's requests, timers and deliveries, and a file that
//! does not open (a stalled network mount) held all of them for as long as it stalled: a 3 s
//! stall was a 3.035 s gap in the daemon's heartbeat, with a bound of 1 s on the read.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::standin::{answer, StandIn};
use super::*;
use crate::pi::setup;
use crate::seams::SystemTime;

/// The words of a folder that was not read in time.
const LATE: &str = "Pi's folder could not be read in time";

/// A stall of `ms` where a read of the folder begins, counted.
fn stalled(ms: u64) -> Arc<AtomicUsize> {
    let began = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&began);
    setup::before_read(move || {
        counted.fetch_add(1, Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(ms));
    });
    began
}

#[test]
fn the_thread_that_asked_goes_on_while_pi_s_folder_is_read_and_its_wait_is_bounded() {
    let pi = StandIn::new(&[], &[]);
    pi.child(&[answer("models", &[])], Ends::Asked);
    stalled(5_000);
    let mut driver = pi.begin(false);
    let began = Instant::now();
    let settled = driver.run();
    // The stall is five seconds; a thread that asked and was held would be held for all of them.
    assert!(
        began.elapsed() < Duration::from_millis(2_000),
        "the thread that asked was held {:?} by a file that did not open",
        began.elapsed()
    );
    assert!(settled.is_empty(), "the words wait for the folder's read");
    assert!(
        pi.time.fire_next(i64::MAX),
        "the wait for the folder is armed"
    );
    let why = driver.run().remove(0).1.unwrap_err();
    assert!(why.starts_with("Pi lists no model"), "{why}");
    assert!(why.contains(LATE), "{why}");
    assert!(
        !why.contains("no provider is set up"),
        "a folder that was not read is never called empty: {why}"
    );
}

#[test]
fn a_folder_still_being_read_is_read_by_no_second_thread() {
    let pi = StandIn::new(&[], &[]);
    pi.child(&[answer("models", &[])], Ends::Asked);
    pi.child(&[answer("models", &[])], Ends::Asked);
    let began = stalled(5_000);
    let mut first = pi.begin(false);
    assert!(first.run().is_empty());
    let mut second = pi.begin(false);
    let began_second = Instant::now();
    let why = second.run().remove(0).1.unwrap_err();
    assert!(
        began_second.elapsed() < Duration::from_millis(2_000),
        "the second ask waited {:?}",
        began_second.elapsed()
    );
    assert!(why.contains(LATE), "{why}");
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(
        began.load(Ordering::SeqCst),
        1,
        "one read of the folder at a time: a stalled one is not read again beside itself"
    );
}

/// The review's probe, in a runtime of one thread as the daemon's: a task that beats every 20 ms
/// goes on beating while a read of the folder stalls for 5 s (its bound is 2 s: the read ends
/// at it, and the thread that asked was never held).
#[test]
fn the_heartbeat_of_the_thread_that_asked_stays_on_time_while_the_folder_stalls() {
    let pi = StandIn::new(&[], &[]);
    pi.child(&[answer("models", &[])], Ends::Asked);
    stalled(5_000);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let local = tokio::task::LocalSet::new();
    let beats = Rc::new(RefCell::new(Vec::<Instant>::new()));
    let why = runtime.block_on(local.run_until(async {
        let heart = tokio::task::spawn_local({
            let beats = Rc::clone(&beats);
            async move {
                loop {
                    beats.borrow_mut().push(Instant::now());
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            }
        });
        tokio::time::sleep(Duration::from_millis(100)).await;
        let read = listed(&pi.env, &*pi.processes, &SystemTime, false, Settle::No).await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        heart.abort();
        read.unwrap_err()
    }));
    let beats = beats.borrow();
    let worst = beats
        .windows(2)
        .map(|pair| pair[1] - pair[0])
        .max()
        .unwrap_or_default();
    assert!(
        worst < Duration::from_millis(1_000),
        "the heartbeat of the thread that asked missed {worst:?} while a file did not open"
    );
    assert!(
        why.contains(LATE),
        "a folder that stalls is not waited for past its bound: {why}"
    );
}

/// A read that is stalled holds the one read there is, and ends: once its thread has gone, the
/// folder is read again (a mount that comes back is not given up on for ever).
#[test]
fn the_folder_is_read_again_once_the_stalled_read_has_ended() {
    let pi = StandIn::new(&[], &[]);
    pi.child(&[answer("models", &[])], Ends::Asked);
    pi.child(&[answer("models", &[])], Ends::Asked);
    let began = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&began);
    setup::before_read(move || {
        // Only the first read stalls.
        if counted.fetch_add(1, Ordering::SeqCst) == 0 {
            std::thread::sleep(Duration::from_millis(300));
        }
    });
    let waiting_for_it = Instant::now();
    let mut first = pi.begin(false);
    assert!(first.run().is_empty());
    assert!(pi.time.fire_next(i64::MAX), "the wait is armed");
    let why = settled(&mut first).unwrap_err();
    assert!(why.contains(LATE), "{why}");
    while setup::reading_now() {
        assert!(
            waiting_for_it.elapsed() < Duration::from_secs(10),
            "the stalled read never ended"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let why = settled(&mut pi.begin(false)).unwrap_err();
    assert!(
        why.contains("no provider is set up"),
        "the folder was read, and holds nothing: {why}"
    );
    assert_eq!(
        began.load(Ordering::SeqCst),
        2,
        "two reads, one after the other"
    );
}

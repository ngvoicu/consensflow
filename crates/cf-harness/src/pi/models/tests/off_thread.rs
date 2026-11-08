//! F4 of the second reading of T-1270: the read of Pi's folder is on a thread of its own. The
//! daemon runs one thread for every project's requests, timers and deliveries, and a file that
//! does not open (a stalled network mount) held all of them for as long as it stalled: a 3 s
//! stall was a 3.035 s gap in the daemon's heartbeat, with a bound of 1 s on the read.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
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

/// A read of the folder that is held where it begins until the folder has been asked for `asks`
/// times, so that the asks overlap the read; only the first read of the test is held. The count
/// of the reads begun.
fn held_until_asked(asks: usize) -> Arc<AtomicUsize> {
    let began = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&began);
    setup::before_read(move || {
        if counted.fetch_add(1, Ordering::SeqCst) > 0 {
            return;
        }
        let waiting = Instant::now();
        while setup::asks() < asks && waiting.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(1));
        }
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

/// A read that is stalled holds the one read there is: an ask that comes while it is on its thread
/// starts no thread beside it and waits for it, for a bound of its own. It is told the folder was
/// not read in time only when its own bound has passed (a mount that never answers is a real
/// timeout), never because another ask was there first.
#[test]
fn a_folder_still_being_read_is_read_by_no_second_thread() {
    let pi = StandIn::new(&[], &[]);
    pi.child(&[answer("models", &[])], Ends::Asked);
    pi.child(&[answer("models", &[])], Ends::Asked);
    let release = Arc::new(AtomicBool::new(false));
    let began = Arc::new(AtomicUsize::new(0));
    let (counted, released) = (Arc::clone(&began), Arc::clone(&release));
    setup::before_read(move || {
        counted.fetch_add(1, Ordering::SeqCst);
        let waiting = Instant::now();
        while !released.load(Ordering::SeqCst) && waiting.elapsed() < Duration::from_secs(30) {
            std::thread::sleep(Duration::from_millis(5));
        }
    });
    let mut first = pi.begin(false);
    assert!(first.run().is_empty());
    let mut second = pi.begin(false);
    assert!(
        second.run().is_empty(),
        "the second ask waits for the read in flight: it is not told the folder was late at once"
    );
    // Each ask has a bound of its own, armed when it asked.
    assert!(pi.time.fire_next(i64::MAX), "the first ask's wait is armed");
    assert!(
        pi.time.fire_next(i64::MAX),
        "the second ask's wait is armed"
    );
    let first = settled(&mut first).unwrap_err();
    let second = settled(&mut second).unwrap_err();
    assert!(first.contains(LATE), "{first}");
    assert!(second.contains(LATE), "{second}");
    // The reading thread may not have begun when the asks gave up: wait for it, then give a
    // second one the time it would take to begin.
    let beginning = Instant::now();
    while began.load(Ordering::SeqCst) == 0 {
        assert!(
            beginning.elapsed() < Duration::from_secs(10),
            "the read never began"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(
        began.load(Ordering::SeqCst),
        1,
        "one read of the folder at a time: a stalled one is not read again beside itself"
    );
    release.store(true, Ordering::SeqCst);
    let ending = Instant::now();
    while setup::reading_now() {
        assert!(
            ending.elapsed() < Duration::from_secs(10),
            "the read never ended"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// T-1281: the lookup of the default model (the chief's gone-model fallback) reads Pi's list
/// outside the daemon's turn for the harness, so it can overlap a read of the list. It was told
/// the folder was not read in time, in 14 ms, though the folder answers in 2.
#[test]
fn a_default_model_lookup_that_overlaps_a_listing_is_told_what_the_folder_holds() {
    let pi = StandIn::new(&[], &[]);
    pi.child(&[answer("models", &[])], Ends::Asked);
    pi.child(&[answer("models", &[])], Ends::Asked);
    held_until_asked(2);
    let (listing, default) = eventually(Box::pin(async {
        tokio::join!(
            listed(&pi.env, &*pi.processes, &*pi.time, false),
            default_model(&pi.env, &*pi.processes, &*pi.time),
        )
    }));
    let (listing, default) = (listing.unwrap_err(), default.unwrap_err());
    for (what, why) in [("listing", &listing), ("default", &default)] {
        assert!(why.contains("no provider is set up"), "{what}: {why}");
        assert!(!why.contains(LATE), "{what}: {why}");
    }
    assert_eq!(setup::reads(), 1, "the one read answered both");
}

/// Two asks for the same folder that overlap are answered by the one read, whole.
#[test]
fn two_asks_for_the_folder_that_overlap_take_the_answer_of_the_one_read() {
    let pi = StandIn::new(&[("auth.json", r#"{"openai": {"type": "api_key"}}"#)], &[]);
    held_until_asked(2);
    let (first, second) = eventually(Box::pin(async {
        tokio::join!(
            setup::inspect(&*pi.time, &pi.env),
            setup::inspect(&*pi.time, &pi.env),
        )
    }));
    assert!(!first.late && !second.late, "{first:?} / {second:?}");
    assert_eq!(first.logins, ["openai"]);
    assert_eq!(second, first, "the same answer");
    assert_eq!(setup::reads(), 1);
}

/// An ask for another folder takes no other folder's answer: it waits for the read in flight to
/// end (its thread is the one read there is) and reads its own.
#[test]
fn an_ask_for_another_folder_takes_no_answer_that_is_not_its_own() {
    let pi = StandIn::new(&[("auth.json", r#"{"one": {}}"#)], &[]);
    let other = tempfile::tempdir().unwrap();
    std::fs::write(other.path().join("auth.json"), r#"{"two": {}}"#).unwrap();
    let elsewhere = pi
        .env
        .clone()
        .with_var("PI_CODING_AGENT_DIR", other.path().to_str().unwrap());
    held_until_asked(2);
    let (first, second) = eventually(Box::pin(async {
        tokio::join!(
            setup::inspect(&*pi.time, &pi.env),
            setup::inspect(&*pi.time, &elsewhere),
        )
    }));
    assert!(!first.late && !second.late, "{first:?} / {second:?}");
    assert_eq!(first.logins, ["one"]);
    assert_eq!(second.logins, ["two"]);
    assert_eq!(setup::reads(), 2, "each folder was read once");
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
        let read = listed(&pi.env, &*pi.processes, &SystemTime, false).await;
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

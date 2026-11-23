//! The output clock and the screen of one window (N17): the window is not quiet while bytes the
//! pty gave are not yet in its screen, a paste's Enter never goes then, a counts-only snapshot
//! waits for no lock, and a snapshot with the screen's lines takes counts and lines together.

use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;

use super::snapshot_of;
use crate::arbiter::{ArbiterError, Clock, EnterTiming, InputArbiter, OutputClock, OutputCount};
use crate::pty::{PaneError, PaneInputWriter, PaneKey};
use crate::screen::{FeedStage, PaneScreen};

/// A clock that moves only when the test moves it.
pub(super) struct Manual {
    base: Instant,
    elapsed: Mutex<Duration>,
}

impl Manual {
    pub(super) fn new() -> Arc<Self> {
        Arc::new(Self {
            base: Instant::now(),
            elapsed: Mutex::new(Duration::ZERO),
        })
    }

    pub(super) fn advance(&self, by: Duration) {
        *self.elapsed.lock().unwrap() += by;
    }
}

impl Clock for Manual {
    fn now(&self) -> Instant {
        self.base + *self.elapsed.lock().unwrap()
    }

    fn rest(&self, duration: Duration) {
        self.advance(duration);
    }
}

/// A window that records what it is written and says nothing.
#[derive(Default)]
struct Recording(Mutex<Vec<Vec<u8>>>);

impl PaneInputWriter for Recording {
    fn write(&self, _pane: &PaneKey, bytes: &[u8]) -> Result<(), PaneError> {
        self.0.lock().unwrap().push(bytes.to_vec());
        Ok(())
    }
}

/// Holds the feed at `stage`: tells the receiver it is there, and waits for the sender's word.
pub(super) fn hold_at(screen: &PaneScreen, stage: FeedStage) -> (Receiver<()>, Sender<()>) {
    let (held, held_at) = channel();
    let (release, released) = channel::<()>();
    let (held, released) = (Mutex::new(held), Mutex::new(released));
    let once = std::sync::atomic::AtomicBool::new(false);
    screen.hook.set(move |at| {
        // The first feed to come to `stage` is held; the others go through.
        if at == stage && !once.swap(true, std::sync::atomic::Ordering::SeqCst) {
            held.lock().unwrap().send(()).unwrap();
            let _ = released
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(20));
        }
    });
    (held_at, release)
}

fn window() -> (
    Arc<InputArbiter>,
    PaneKey,
    Arc<OutputClock>,
    Arc<PaneScreen>,
) {
    let clock = Manual::new();
    let arbiter = Arc::new(InputArbiter::with_clock(EnterTiming::fixed(0), clock));
    let key = PaneKey::new("counted", 1);
    let printed = arbiter.register(&key).expect("register");
    (arbiter, key, printed, Arc::new(PaneScreen::new(24, 80)))
}

/// A window is not quiet while the pty has given bytes the screen does not yet show, however
/// the clocks stand; quiet counts from the moment the screen has every byte.
#[test]
fn a_window_is_not_quiet_while_bytes_it_printed_are_not_in_its_screen() {
    let clock = Manual::new();
    let arbiter = InputArbiter::with_clock(EnterTiming::fixed(0), clock.clone());
    let key = PaneKey::new("pending", 1);
    let printed = arbiter.register(&key).expect("register");
    let screen = PaneScreen::new(24, 80);
    printed.counts().took(5);
    screen.feed_noting(b"ab", &printed);
    clock.advance(Duration::from_secs(60));
    let snapshot = arbiter.snapshot(&key).unwrap();
    assert_eq!(snapshot.output_quiet_ms, Some(0), "3 of 5 bytes not drawn");
    assert_eq!((snapshot.output_read, snapshot.output_drawn), (5, 2));
    screen.feed_noting(b"cde", &printed);
    clock.advance(Duration::from_millis(40));
    let snapshot = arbiter.snapshot(&key).unwrap();
    assert_eq!(
        snapshot.output_quiet_ms,
        Some(40),
        "quiet from the last feed"
    );
    assert_eq!((snapshot.output_read, snapshot.output_drawn), (5, 5));
}

/// The Enter's wait, with the pty's bytes pending, is not over at its cap: it waits a grace
/// more, and then the Enter goes, once, as main's did: no uncertain answer, so no recovery
/// that sends another Enter or pastes the message again (T-2139 finding 8).
#[test]
fn the_enter_wait_holds_at_its_cap_while_bytes_are_pending_and_then_sends_one_enter() {
    let clock = Manual::new();
    let wait = EnterTiming {
        least_ms: 0,
        drawn_after_ms: 0,
        quiet_ms: 120,
        most_ms: 2_000,
    };
    let arbiter = InputArbiter::with_clock(wait, clock.clone());
    let key = PaneKey::new("cap", 1);
    let printed = arbiter.register(&key).expect("register");
    PaneScreen::new(24, 80).feed_noting(b"echo", &printed);
    printed.counts().took(8);
    let writer = Recording::default();
    let before = clock.now();
    arbiter
        .write_paste_via(&writer, &key, b"body", None)
        .expect("the paste is answered as admitted, not as uncertain");
    assert!(
        clock.now() - before >= Duration::from_millis(5_000),
        "the cap and its grace were waited"
    );
    let written = writer.0.lock().unwrap().clone();
    assert_eq!(written.len(), 2, "the paste and one Enter: {written:?}");
    assert!(written[0].starts_with(b"\x1b[200~"));
    assert_eq!(written[1], b"\r");
    let snapshot = arbiter.snapshot(&key).unwrap();
    assert!(!snapshot.input_failed);
    assert!(snapshot.paste_quiet_ms.is_some());
}

/// The wait itself, on the clock: a window whose bytes arrive in the screen goes on to its
/// quiet; one never caught up ends at the cap and the grace.
#[test]
fn the_wait_ends_when_the_screen_has_caught_up_and_is_quiet() {
    let clock = Manual::new();
    let wait = EnterTiming {
        least_ms: 0,
        drawn_after_ms: 0,
        quiet_ms: 120,
        most_ms: 2_000,
    };
    let arbiter = InputArbiter::with_clock(wait, clock.clone());
    let key = PaneKey::new("quiet", 1);
    let printed = arbiter.register(&key).expect("register");
    let screen = PaneScreen::new(24, 80);
    printed.counts().took(1);
    screen.feed_noting(b"e", &printed);
    let paste_at = clock.now();
    clock.advance(Duration::from_millis(1));
    wait.wait_released_on(&*clock, &printed, paste_at);
    assert!(clock.now() - paste_at >= Duration::from_millis(120));
}

/// A counts-only snapshot waits for no lock: with the feed held inside the screen's lock it
/// still answers at once, with the bytes it has (note 7 of T-2132: it waited 299 ms at the
/// median under a flood).
#[test]
fn a_counts_only_snapshot_does_not_wait_for_the_screen_lock() {
    let (arbiter, key, printed, screen) = window();
    printed.counts().took(8);
    let (held, release) = hold_at(&screen, FeedStage::Locked);
    let feeding = {
        let (screen, printed) = (Arc::clone(&screen), Arc::clone(&printed));
        thread::spawn(move || screen.feed_noting(b"12345678", &printed))
    };
    held.recv_timeout(Duration::from_secs(5))
        .expect("the feed is held in the lock");
    let mut took = Vec::new();
    for _ in 0..50 {
        let at = Instant::now();
        let snapshot = snapshot_of(&arbiter, &key, Some(Arc::clone(&screen)), None, None).unwrap();
        took.push(at.elapsed());
        assert_eq!(
            (
                snapshot["outputRead"].as_u64(),
                snapshot["outputDrawn"].as_u64()
            ),
            (Some(8), Some(0))
        );
        assert_eq!(snapshot["outputQuietMs"], 0);
    }
    took.sort();
    release.send(()).unwrap();
    feeding.join().unwrap();
    assert!(
        took[took.len() / 2] < Duration::from_millis(1)
            && took[took.len() - 1] < Duration::from_millis(250),
        "median {:?}, worst {:?}",
        took[took.len() / 2],
        took[took.len() - 1]
    );
}

/// A snapshot with the screen's lines takes counts and lines together: with the feed held
/// inside the lock it waits; its drawn count is of the screen it read.
#[test]
fn a_snapshot_with_lines_says_drawn_as_the_screen_it_read() {
    let (arbiter, key, printed, screen) = window();
    printed.counts().took(3);
    let (held, release) = hold_at(&screen, FeedStage::Locked);
    let feeding = {
        let (screen, printed) = (Arc::clone(&screen), Arc::clone(&printed));
        thread::spawn(move || screen.feed_noting(b"abc", &printed))
    };
    held.recv_timeout(Duration::from_secs(5)).expect("held");
    let (told, snapshots) = channel();
    let reading = {
        let (arbiter, key, screen) = (Arc::clone(&arbiter), key.clone(), Arc::clone(&screen));
        thread::spawn(move || {
            told.send(snapshot_of(&arbiter, &key, Some(screen), None, Some(3)).unwrap())
                .unwrap();
        })
    };
    assert!(
        snapshots.recv_timeout(Duration::from_millis(300)).is_err(),
        "it waits for the lock"
    );
    release.send(()).unwrap();
    feeding.join().unwrap();
    reading.join().unwrap();
    let snapshot: Value = snapshots.recv().unwrap();
    assert_eq!(
        (
            snapshot["outputRead"].as_u64(),
            snapshot["outputDrawn"].as_u64()
        ),
        (Some(3), Some(3))
    );
    assert_eq!(snapshot["bottom"][0], "abc");
}

/// A reader that has the screen's lines and not yet the counts holds the feed back: a read
/// that comes in between is in neither.
#[test]
fn a_reader_between_the_lines_and_the_counts_keeps_the_next_read_out_of_both() {
    let (arbiter, key, printed, screen) = window();
    printed.counts().took(1);
    screen.feed_noting(b"a", &printed);
    let (paused, resume) = hold_at(&screen, FeedStage::Snapshot);
    let reading = {
        let (arbiter, key, screen) = (Arc::clone(&arbiter), key.clone(), Arc::clone(&screen));
        thread::spawn(move || snapshot_of(&arbiter, &key, Some(screen), None, Some(3)).unwrap())
    };
    paused
        .recv_timeout(Duration::from_secs(5))
        .expect("the reader has the lines");
    printed.counts().took(1);
    let (done, fed) = channel();
    let feeding = {
        let (screen, printed) = (Arc::clone(&screen), Arc::clone(&printed));
        thread::spawn(move || {
            screen.feed_noting(b"b", &printed);
            done.send(()).unwrap();
        })
    };
    let fed_meanwhile = fed.recv_timeout(Duration::from_millis(300)).is_ok();
    resume.send(()).unwrap();
    let snapshot = reading.join().unwrap();
    feeding.join().unwrap();
    assert!(
        !fed_meanwhile,
        "a read was fed between the screen's lines and the counts"
    );
    // The lines are "a": the drawn count is 1; the read count stands as it was then (2).
    assert_eq!(snapshot["bottom"][0], "a");
    assert_eq!(snapshot["outputDrawn"], 1);
}

#[test]
fn the_counts_settle_where_they_stand_when_nobody_takes_the_output_in() {
    let counts = OutputCount::default();
    counts.took(9);
    counts.drew(4);
    assert!(counts.pending());
    counts.give_up();
    assert!(!counts.pending());
    assert_eq!(counts.both(), (9, 9));
}

/// A pane whose feed panicked admits nothing: a paste or a claim against the screen the panic
/// left is refused as a failed input is (the pane is then ended: `stream_to_page`).
#[test]
fn a_pane_whose_feed_broke_admits_no_paste_and_no_claim() {
    let (arbiter, key, printed, _screen) = window();
    printed.counts().break_feed();
    let writer = Recording::default();
    let answer = arbiter.write_paste_via(&writer, &key, b"body", None);
    assert!(
        matches!(answer, Err(ArbiterError::InputFailed)),
        "{answer:?}"
    );
    assert!(writer.0.lock().unwrap().is_empty(), "nothing was written");
    assert!(matches!(
        arbiter.claim(&key),
        Err(ArbiterError::InputFailed)
    ));
}

/// N17 (T-2139): a snapshot samples the two counts once, drawn first and read after it. A
/// stream that goes 10/9, then 11/9, then 11/10 while it is sampled is pending throughout: the
/// pair is read 11 and 9 (or more of read), never 10 and 10.
#[test]
fn a_snapshot_samples_the_counts_drawn_first_and_once() {
    let clock = Manual::new();
    let arbiter = InputArbiter::with_clock(EnterTiming::fixed(0), clock.clone());
    let key = PaneKey::new("sampled", 1);
    let printed = arbiter.register(&key).expect("register");
    printed.counts().took(10);
    printed.counts().drew(9);
    let counts = Arc::clone(printed.counts());
    crate::arbiter::BETWEEN_LOADS.with(|slot| {
        slot.set(Some(Box::new(move || {
            counts.took(1);
            counts.drew(1);
        })))
    });
    let snapshot = arbiter.snapshot(&key).unwrap();
    assert!(
        snapshot.output_read > snapshot.output_drawn,
        "pending throughout, read as {}/{}",
        snapshot.output_read,
        snapshot.output_drawn
    );
    assert_eq!(snapshot.output_quiet_ms, Some(0));
}

/// The same, the stream moving after the first sampling: the snapshot's pair is the one
/// sampling's (10 read and 9 drawn), not the read of one and the drawn of another (10/10).
#[test]
fn a_snapshot_takes_both_counts_from_one_sampling() {
    let clock = Manual::new();
    let arbiter = InputArbiter::with_clock(EnterTiming::fixed(0), clock.clone());
    let key = PaneKey::new("sampled-once", 1);
    let printed = arbiter.register(&key).expect("register");
    printed.counts().took(10);
    printed.counts().drew(9);
    let counts = Arc::clone(printed.counts());
    crate::arbiter::AFTER_LOADS.with(|slot| {
        slot.set(Some(Box::new(move || {
            counts.took(1);
            counts.drew(1);
        })))
    });
    let snapshot = arbiter.snapshot(&key).unwrap();
    assert_eq!(
        (snapshot.output_read, snapshot.output_drawn),
        (10, 9),
        "the pair of one sampling"
    );
}

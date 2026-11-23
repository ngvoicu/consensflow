//! The output clock and the screen of one window: while a read of its output is taken into the
//! screen the window is not quiet, a snapshot takes the clocks and the screen together, and a
//! panic in the feed leaves no clock stuck (N17).

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;

use super::snapshot_of;
use crate::arbiter::{Clock, EnterTiming, InputArbiter, OutputClock};
use crate::pty::{PaneError, PaneInputWriter, PaneKey};
use crate::screen::{FeedStage, PaneScreen};

/// A clock that moves only when the test moves it.
struct Manual {
    base: Instant,
    elapsed: Mutex<Duration>,
}

impl Manual {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            base: Instant::now(),
            elapsed: Mutex::new(Duration::ZERO),
        })
    }

    fn advance(&self, by: Duration) {
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

/// A window that takes the human's keys and says nothing.
struct Silent;

impl PaneInputWriter for Silent {
    fn write(&self, _pane: &PaneKey, _bytes: &[u8]) -> Result<(), PaneError> {
        Ok(())
    }
}

const BOX: &[u8] = b"\x1b[2J\x1b[H\xe2\x94\x80\xe2\x94\x80\xe2\x94\x80\r\n\xe2\x9d\xaf\r\n\xe2\x94\x80\xe2\x94\x80\xe2\x94\x80";
/// What a window draws when the human's `X` reaches its box.
const ECHO_OF_X: &[u8] = b"\x1b[2;3HX";

/// A window whose empty box was drawn, then a key of the human's 1,200 ms ago, and 1,500 ms
/// since the box: the moment a paste would be judged on.
struct Typed {
    arbiter: Arc<InputArbiter>,
    key: PaneKey,
    printed: Arc<OutputClock>,
    screen: Arc<PaneScreen>,
}

impl Typed {
    fn new() -> Self {
        let clock = Manual::new();
        let arbiter = Arc::new(InputArbiter::with_clock(
            EnterTiming::fixed(0),
            clock.clone(),
        ));
        let key = PaneKey::new("feeding", 1);
        let printed = arbiter.register(&key).expect("register");
        let screen = Arc::new(PaneScreen::new(24, 80));
        screen.feed_noting(BOX, &printed);
        clock.advance(Duration::from_millis(300));
        arbiter.write_typed_via(&Silent, &key, b"X").expect("typed");
        clock.advance(Duration::from_millis(1200));
        Self {
            arbiter,
            key,
            printed,
            screen,
        }
    }

    fn snapshot(&self) -> Value {
        snapshot_of(
            &self.arbiter,
            &self.key,
            Some(Arc::clone(&self.screen)),
            None,
            Some(3),
        )
        .expect("snapshot")
    }
}

/// Whether the adapter's reader would take the box for empty and let a paste go over what
/// the human typed: the key settled, the window printed since it, and its box showing none
/// of what was typed (`read_unsent` in cf-harness's `shared/input_box.rs`).
fn reads_empty_over_a_key(snapshot: &Value) -> bool {
    let quiet = |name: &str| snapshot[name].as_u64().expect(name);
    let drawn = snapshot["outputQuietMs"]
        .as_u64()
        .is_some_and(|printed| printed < quiet("typedQuietMs"));
    let shows_the_key = snapshot["bottom"]
        .as_array()
        .unwrap()
        .iter()
        .any(|line| line.as_str().unwrap().contains('X'));
    quiet("typedQuietMs") >= 1_000 && drawn && !shows_the_key
}

/// Holds the feed at `stage`: tells `held` it is there, and waits for `release`.
fn hold_at(screen: &PaneScreen, stage: FeedStage) -> (Receiver<()>, std::sync::mpsc::Sender<()>) {
    let (held, held_at) = channel();
    let (release, released) = channel::<()>();
    let (held, released) = (Mutex::new(held), Mutex::new(released));
    screen.hook.set(move |at| {
        if at == stage {
            held.lock().unwrap().send(()).unwrap();
            let _ = released
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(10));
        }
    });
    (held_at, release)
}

/// The paste's Enter, by the host's wait, goes after a read the host has read and the screen
/// does not yet show: the window is not quiet while the read is taken in, however long that
/// takes (T-2104's flood: a read of erases on the biggest screen takes longer than the 120 ms
/// the Enter waits for quiet when the machine is loaded).
#[test]
fn the_enter_goes_after_a_read_that_the_host_has_and_the_screen_does_not_yet_show() {
    let arbiter = InputArbiter::new(EnterTiming::fixed(0));
    let key = PaneKey::new("held-read", 1);
    let printed = arbiter.register(&key).expect("register");
    let screen = Arc::new(PaneScreen::new(24, 80));
    screen.feed_noting(b"echo", &printed);
    let (held, release) = hold_at(&screen, FeedStage::Locked);
    let taken = Arc::new(AtomicUsize::new(1));
    let feeding = {
        let (screen, printed, taken) = (
            Arc::clone(&screen),
            Arc::clone(&printed),
            Arc::clone(&taken),
        );
        thread::spawn(move || {
            screen.feed_noting(b"the read that is held", &printed);
            taken.fetch_add(1, Ordering::SeqCst);
        })
    };
    held.recv_timeout(Duration::from_secs(5))
        .expect("the read is held");
    // The wait of the app's Enter, but for a longer cap than the hold: 120 ms of quiet.
    let wait = EnterTiming {
        least_ms: 0,
        drawn_after_ms: 0,
        quiet_ms: 120,
        most_ms: 30_000,
    };
    let releasing = thread::spawn(move || {
        thread::sleep(Duration::from_millis(500));
        release.send(()).unwrap();
    });
    wait.wait_released(&printed, Instant::now() - Duration::from_millis(20));
    let taken_when_released = taken.load(Ordering::SeqCst);
    feeding.join().unwrap();
    releasing.join().unwrap();
    assert_eq!(
        taken_when_released, 2,
        "the Enter went while a read was held on its way into the screen"
    );
}

/// With the feed held where the host has the read and has not yet the screen's lock, or has
/// the lock and has not fed, a snapshot never shows the clock past the human's key with the
/// box before its echo: that is the reading a paste goes over unsent typing on.
#[test]
fn a_snapshot_never_has_the_clock_of_a_read_whose_echo_the_screen_does_not_show() {
    for stage in [FeedStage::Read, FeedStage::Locked] {
        let window = Typed::new();
        assert!(
            !reads_empty_over_a_key(&window.snapshot()),
            "{stage:?}: before"
        );
        let (held, release) = hold_at(&window.screen, stage);
        let feeding = {
            let (screen, printed) = (Arc::clone(&window.screen), Arc::clone(&window.printed));
            thread::spawn(move || screen.feed_noting(ECHO_OF_X, &printed))
        };
        held.recv_timeout(Duration::from_secs(5))
            .expect("the feed is held");
        let (told, snapshots) = channel();
        let reading = {
            let (arbiter, key, screen) = (
                Arc::clone(&window.arbiter),
                window.key.clone(),
                Arc::clone(&window.screen),
            );
            thread::spawn(move || {
                told.send(snapshot_of(&arbiter, &key, Some(screen), None, Some(3)).unwrap())
                    .unwrap();
            })
        };
        // Held at the lock, the reader waits for the feed; before it, it reads at once.
        match snapshots.recv_timeout(Duration::from_millis(300)) {
            Ok(snapshot) => {
                assert_eq!(
                    stage,
                    FeedStage::Read,
                    "only a feed not yet in the lock lets a reader by"
                );
                assert!(!reads_empty_over_a_key(&snapshot), "{stage:?}: {snapshot}");
            }
            Err(_) => assert_eq!(stage, FeedStage::Locked, "a reader waited at the lock"),
        }
        release.send(()).unwrap();
        feeding.join().unwrap();
        reading.join().unwrap();
        while let Ok(snapshot) = snapshots.try_recv() {
            assert!(
                !reads_empty_over_a_key(&snapshot),
                "{stage:?}: after: {snapshot}"
            );
        }
        assert!(
            !reads_empty_over_a_key(&window.snapshot()),
            "{stage:?}: the echo is drawn"
        );
    }
}

/// A reader that has the screen's lines and not yet the clocks holds the feed back: a read
/// that comes in between is in neither.
#[test]
fn a_reader_between_the_screens_lines_and_the_clocks_keeps_the_next_read_out_of_both() {
    let window = Typed::new();
    let (paused, resume) = hold_at(&window.screen, FeedStage::Snapshot);
    let reading = {
        let (arbiter, key, screen) = (
            Arc::clone(&window.arbiter),
            window.key.clone(),
            Arc::clone(&window.screen),
        );
        thread::spawn(move || snapshot_of(&arbiter, &key, Some(screen), None, Some(3)).unwrap())
    };
    paused
        .recv_timeout(Duration::from_secs(5))
        .expect("the reader has the lines");
    let (done, fed) = channel();
    let feeding = {
        let (screen, printed) = (Arc::clone(&window.screen), Arc::clone(&window.printed));
        thread::spawn(move || {
            screen.feed_noting(ECHO_OF_X, &printed);
            done.send(()).unwrap();
        })
    };
    // The reader is between its two reads: the feed waits for it, or the reader is wrong.
    let fed_meanwhile = fed.recv_timeout(Duration::from_millis(300)).is_ok();
    resume.send(()).unwrap();
    let snapshot = reading.join().unwrap();
    feeding.join().unwrap();
    assert!(!reads_empty_over_a_key(&snapshot), "{snapshot}");
    assert!(
        !fed_meanwhile,
        "a read was fed between the screen's lines and the clocks"
    );
}

/// A panic while a read is taken in: the window printed, and the screen may have changed
/// (it was partly written), as of the moment of the panic. The clock says that and then ages;
/// it is not left at zero for good, and the window's next read is taken in as before.
#[test]
fn a_panic_in_the_feed_leaves_the_window_printed_now_and_not_busy_for_good() {
    let clock = Manual::new();
    let arbiter = InputArbiter::with_clock(EnterTiming::fixed(0), clock.clone());
    let key = PaneKey::new("unwind", 1);
    let printed = arbiter.register(&key).expect("register");
    let screen = PaneScreen::new(24, 80);
    screen.hook.set(|stage| {
        assert_ne!(stage, FeedStage::Locked, "an injected panic in the feed");
    });
    let panicked = catch_unwind(AssertUnwindSafe(|| screen.feed_noting(b"x", &printed)));
    assert!(panicked.is_err());
    clock.advance(Duration::from_secs(24 * 3600));
    let snapshot = arbiter.snapshot(&key).expect("snapshot");
    assert_eq!(
        snapshot.output_quiet_ms,
        Some(24 * 3600 * 1000),
        "printed at the panic, then aged"
    );
    assert_eq!(
        snapshot.screen_quiet_ms,
        Some(24 * 3600 * 1000),
        "shown at the panic, then aged"
    );
    // The screen lock the panic poisoned reads as it does for the host's other readers.
    screen.hook.set(|_| {});
    assert!(screen.feed_noting(b"y", &printed));
    assert_eq!(arbiter.snapshot(&key).unwrap().output_quiet_ms, Some(0));
}

//! The output clock and the screen of one window (N17): the window is not quiet while bytes the
//! pty gave are not yet in its screen, a paste's Enter never goes then, a counts-only snapshot
//! waits for no lock, and a snapshot with the screen's lines takes counts and lines together.

use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;

use super::snapshot_of;
use crate::arbiter::{
    ArbiterError, At, Clock, EnterTiming, InputArbiter, OutputClock, OutputCount, Paste, Why,
};
use crate::pty::{PaneError, PaneInputWriter, PaneKey};
use crate::screen::{FeedStage, PaneScreen, Screen, Seen};
use cf_proto::panes::{Evidence, ReadEmpty};

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
    screen.feed_noting(b"ab", &printed, |_| {});
    clock.advance(Duration::from_secs(60));
    let snapshot = arbiter.snapshot(&key).unwrap();
    assert_eq!(snapshot.output_quiet_ms, Some(0), "3 of 5 bytes not drawn");
    assert_eq!((snapshot.output_read, snapshot.output_drawn), (5, 2));
    screen.feed_noting(b"cde", &printed, |_| {});
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
    PaneScreen::new(24, 80).feed_noting(b"echo", &printed, |_| {});
    printed.counts().took(8);
    let writer = Recording::default();
    let before = clock.now();
    arbiter
        .write_paste_plain(&writer, &key, b"body", None)
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
    screen.feed_noting(b"e", &printed, |_| {});
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
        thread::spawn(move || screen.feed_noting(b"12345678", &printed, |_| {}))
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
        thread::spawn(move || screen.feed_noting(b"abc", &printed, |_| {}))
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
    screen.feed_noting(b"a", &printed, |_| {});
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
            screen.feed_noting(b"b", &printed, |_| {});
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
    let answer = arbiter.write_paste_plain(&writer, &key, b"body", None);
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

/// A window that records what it is written, behind a screen: what the host checks a look
/// against.
struct Seeing {
    written: Mutex<Vec<Vec<u8>>>,
    screen: Arc<PaneScreen>,
}

impl Seeing {
    fn over(screen: &Arc<PaneScreen>) -> Self {
        Self {
            written: Mutex::default(),
            screen: Arc::clone(screen),
        }
    }

    fn written(&self) -> Vec<Vec<u8>> {
        self.written.lock().unwrap().clone()
    }
}

impl PaneInputWriter for Seeing {
    fn write(&self, _pane: &PaneKey, bytes: &[u8]) -> Result<(), PaneError> {
        self.written.lock().unwrap().push(bytes.to_vec());
        Ok(())
    }

    fn with_screen(&self, _key: &PaneKey, under: &mut dyn FnMut(Option<&Screen>)) {
        self.screen.with_screen(&mut |screen| under(Some(screen)));
    }
}

/// An empty Claude-shaped box with the cursor in it, as the window draws it.
const BOX: &str = "────────\r\n❯ \r\n────────\x1b[2;3H";

struct Boxed {
    clock: Arc<Manual>,
    arbiter: InputArbiter,
    key: PaneKey,
    printed: Arc<OutputClock>,
    screen: Arc<PaneScreen>,
    writer: Seeing,
}

impl Boxed {
    fn new() -> Self {
        let clock = Manual::new();
        let arbiter = InputArbiter::with_clock(EnterTiming::fixed(0), clock.clone());
        let key = PaneKey::new("boxed", 1);
        let printed = arbiter.register(&key).expect("register");
        let screen = Arc::new(PaneScreen::new(24, 80));
        let writer = Seeing::over(&screen);
        let window = Self {
            clock,
            arbiter,
            key,
            printed,
            screen,
            writer,
        };
        window.print(BOX);
        window
    }

    /// The window prints `text`: read, and drawn.
    fn print(&self, text: &str) -> bool {
        self.printed.counts().took(text.len());
        self.screen
            .feed_noting(text.as_bytes(), &self.printed, |_| {})
    }

    /// A paste that goes over the count on `by`, whatever the look's count of keys says.
    fn paste_by(&self, at: At, by: Evidence) -> Result<(), ArbiterError> {
        self.arbiter.write_paste_via(
            &self.writer,
            &self.key,
            Paste {
                body: b"body",
                at: Some(at),
                read_empty: Some(ReadEmpty {
                    typed: at.typed,
                    by,
                }),
                clear_first: None,
            },
        )
    }

    /// What a look carries of the screens kept at the person's keys (`unsentAt`).
    fn unsent_at(&self) -> Option<Value> {
        let answer = snapshot_of(
            &self.arbiter,
            &self.key,
            Some(Arc::clone(&self.screen)),
            None,
            Some(40),
        )
        .unwrap();
        answer.get("unsentAt").cloned()
    }

    fn type_key(&self, key: &str) {
        self.arbiter
            .write_typed_via(&self.writer, &self.key, key.as_bytes())
            .unwrap();
    }

    /// The look now: what a reader of the box is given, and the count of the person's keys.
    fn look(&self) -> At {
        let mut seen = None;
        self.writer
            .with_screen(&self.key, &mut |screen| seen = screen.map(Screen::seen));
        At {
            seen: seen.unwrap(),
            typed: self.arbiter.snapshot(&self.key).unwrap().typed,
        }
    }

    fn paste(&self, at: At, read_empty: bool) -> Result<(), ArbiterError> {
        self.paste_clearing(at, read_empty, None)
    }

    fn paste_clearing(
        &self,
        at: At,
        read_empty: bool,
        clear_first: Option<&[u8]>,
    ) -> Result<(), ArbiterError> {
        self.arbiter.write_paste_via(
            &self.writer,
            &self.key,
            Paste {
                body: b"body",
                at: Some(at),
                read_empty: read_empty.then_some(ReadEmpty {
                    typed: at.typed,
                    by: Evidence::Elsewhere,
                }),
                clear_first,
            },
        )
    }

    fn pasted(&self) -> usize {
        self.writer
            .written()
            .iter()
            .filter(|bytes| bytes.ends_with(b"\x1b[201~"))
            .count()
    }
}

fn why(refused: Result<(), ArbiterError>) -> Option<Why> {
    match refused {
        Err(ArbiterError::LookAgain(why)) => Some(why),
        _ => None,
    }
}

/// T-2160 1: the person typed `X`; the reader, seeing a faint `X` beyond the cursor, read the
/// box empty; the window's echo moved only the cursor past it. The cursor is in `seen`: refused,
/// and the window got the key and nothing more.
#[test]
fn a_cursor_only_echo_since_the_look_refuses_the_paste() {
    let window = Boxed::new();
    window.type_key("X");
    let at = window.look();
    assert!(
        !window.print("\x1b[2;4H"),
        "the grid's digest does not hold the cursor"
    );
    let refused = window.paste(at, true);
    assert_eq!(why(refused), Some(Why::BoxChanged));
    assert_eq!(window.writer.written(), [b"X".to_vec()]);
}

/// T-2160 1: a feed held after the pty gave its bytes, and the same drawn: pending, then the
/// box changed. The digest and the count of pending bytes are one check, under the screen's lock.
#[test]
fn a_look_is_refused_for_the_echo_held_before_the_screen_and_for_the_echo_drawn() {
    let window = Boxed::new();
    window.type_key("X");
    let at = window.look();
    window.printed.counts().took(1);
    assert_eq!(
        why(window.paste(at, true)),
        Some(Why::OutputPending),
        "pending"
    );
    assert!(window.screen.feed_noting(b"X", &window.printed, |_| {}));
    assert_eq!(why(window.paste(at, true)), Some(Why::BoxChanged), "drawn");
    assert_eq!(window.writer.written(), [b"X".to_vec()]);
}

/// T-2150 1: a window that repaints at rest (5 colour resets, the cursor put where it is) leaves
/// the look good.
#[test]
fn a_repaint_at_rest_since_the_look_leaves_it_good() {
    let window = Boxed::new();
    window.type_key("X");
    let at = window.look();
    for _ in 0..5 {
        assert!(!window.print("\x1b[0m\x1b[2;3H"));
    }
    window
        .paste(at, true)
        .expect("a repaint changes no line and no cursor");
    assert_eq!(window.pasted(), 1);
}

/// T-2160 1: a person's key since the look, typed and erased again, is refused: the count moved.
#[test]
fn a_key_typed_and_erased_since_the_look_refuses_the_paste() {
    let window = Boxed::new();
    let at = window.look();
    window.type_key("X");
    window.type_key("\x7f");
    assert!(matches!(window.paste(at, false), Err(ArbiterError::Unsent)));
    assert_eq!(window.pasted(), 0);
}

/// T-2160 1: a scroll, a wrap and a resize that change a line the reader would read refuse the
/// paste; a resize that changes none does not.
#[test]
fn a_wrap_a_scroll_and_a_resize_that_change_a_line_refuse_the_paste() {
    let window = Boxed::new();
    let at = window.look();
    window.screen.resize(24, 80);
    window
        .paste(at, false)
        .expect("a resize that moves no line");
    let at = window.look();
    window.print(&"w".repeat(120));
    assert_eq!(
        why(window.paste(at, false)),
        Some(Why::BoxChanged),
        "a wrap"
    );
    let at = window.look();
    window.screen.resize(24, 40);
    assert_eq!(
        why(window.paste(at, false)),
        Some(Why::BoxChanged),
        "a resize that joins other rows"
    );
    let at = window.look();
    window.print(&"\r\nline".repeat(40));
    assert_eq!(
        why(window.paste(at, false)),
        Some(Why::BoxChanged),
        "a scroll"
    );
}

/// A key that empties a stale paste is written with the body, in one write; a box that changed
/// writes neither.
#[test]
fn the_clearing_key_goes_with_the_paste_and_never_alone() {
    let window = Boxed::new();
    let at = window.look();
    window.paste_clearing(at, false, Some(b"\x15")).unwrap();
    assert_eq!(
        window.writer.written(),
        [b"\x15\x1b[200~body\x1b[201~".to_vec(), b"\r".to_vec()]
    );
    let at = window.look();
    window.print("\x1b[2;4H");
    window.paste_clearing(at, false, Some(b"\x15")).unwrap_err();
    assert_eq!(
        window.writer.written().len(),
        2,
        "nothing written when the box changed"
    );
}

/// A paste with no look (a daemon older than `seen`) is refused for the person's unsent text
/// alone.
#[test]
fn a_paste_with_no_look_is_refused_for_unsent_text_alone() {
    let window = Boxed::new();
    window.printed.counts().took(4);
    window
        .arbiter
        .write_paste_plain(&window.writer, &window.key, b"a", None)
        .unwrap();
    window.type_key("X");
    let refused = window
        .arbiter
        .write_paste_plain(&window.writer, &window.key, b"b", None);
    assert!(matches!(refused, Err(ArbiterError::Unsent)));
}

/// T-2160 3: the Enter of a look never goes while output is pending. It waits for the screen to
/// have what the pty gave and goes then; at the grace it is refused, never written.
#[test]
fn an_enter_of_a_look_waits_while_output_is_pending_and_is_refused_at_the_grace() {
    let window = Boxed::new();
    let at = window.look();
    window.printed.counts().took(4);
    thread::scope(|scope| {
        let entering = scope.spawn(|| window.arbiter.enter(&window.writer, &window.key, at));
        thread::sleep(Duration::from_millis(60));
        assert!(window.writer.written().is_empty(), "pending: no Enter");
        assert!(!window
            .screen
            .feed_noting(b"\x1b[0m", &window.printed, |_| {}));
        entering.join().unwrap().expect("drawn: the Enter goes");
    });
    assert_eq!(window.writer.written(), [b"\r".to_vec()]);

    window.printed.counts().took(1);
    thread::scope(|scope| {
        let entering = scope.spawn(|| window.arbiter.enter(&window.writer, &window.key, at));
        thread::sleep(Duration::from_millis(60));
        window.clock.advance(Duration::from_millis(3_000));
        let refused = entering.join().unwrap();
        assert_eq!(why(refused), Some(Why::OutputPending), "the grace: refused");
    });
    assert_eq!(
        window.writer.written().len(),
        1,
        "never written while pending"
    );
}

/// An Enter after the person typed, or over a box that changed, writes nothing.
#[test]
fn an_enter_of_a_look_is_refused_for_a_key_since_and_for_a_box_that_changed() {
    let window = Boxed::new();
    let at = window.look();
    window.type_key("X");
    let refused = window.arbiter.enter(&window.writer, &window.key, at);
    assert!(matches!(refused, Err(ArbiterError::Unsent)), "{refused:?}");
    window.type_key("\x7f");
    let written = window.writer.written().len();
    let at = window.look();
    window.print("\x1b[2;4H");
    let refused = window.arbiter.enter(&window.writer, &window.key, at);
    assert_eq!(why(refused), Some(Why::BoxChanged));
    assert_eq!(window.writer.written().len(), written, "nothing written");
}

/// The key that empties a box goes with the paste's one write, and a refusal writes neither: a
/// box that changed since the look gets no key of the engine's.
#[test]
fn a_refused_paste_writes_no_clearing_key() {
    let window = Boxed::new();
    let at = window.look();
    window.print("\x1b[2;4H");
    let refused = window.paste_clearing(at, false, Some(b"\x15"));
    assert_eq!(why(refused), Some(Why::BoxChanged));
    assert!(
        window.writer.written().is_empty(),
        "no key alone, and no paste"
    );
}

/// A paste that names evidence the host holds goes in over the person's unsent count and
/// clears the count and the kept screens; one that names none is refused for the unsent text;
/// one with a key of theirs since the look is refused all the same.
#[test]
fn a_paste_read_empty_at_the_looks_key_goes_in_over_the_count_and_clears_it() {
    let window = Boxed::new();
    window.type_key("X");
    let at = window.look();
    assert!(matches!(window.paste(at, false), Err(ArbiterError::Unsent)));
    assert_eq!(window.pasted(), 0);
    window.type_key("\x1b[D");
    assert!(
        matches!(window.paste(at, true), Err(ArbiterError::Unsent)),
        "a key since the look is no reason"
    );
    assert_eq!(window.pasted(), 0);
    let at = window.look();
    window
        .paste_by(at, Evidence::Erased)
        .expect("read empty over the count");
    assert_eq!(window.pasted(), 1);
    assert_eq!(window.unsent_at(), None, "forgotten with the count");
    let at = window.look();
    window
        .paste(at, false)
        .expect("the count was cleared by that paste");
    assert_eq!(window.pasted(), 2);
}

/// The host keeps a screen before each write that added a character, the same screen once, and
/// one before the first write since that added none; it forgets them when the count is zero
/// (T-2255).
#[test]
fn the_screens_kept_at_the_persons_keys_are_as_the_table_says() {
    let window = Boxed::new();
    window.type_key("a");
    let at = window.unsent_at().expect("a count above zero");
    assert_eq!(at["typedAt"].as_array().unwrap().len(), 1);
    assert_eq!(at["typedMore"], false);
    assert_eq!(at["typedAt"][0]["current"], true);
    assert!(at.get("pressedOver").is_none());
    assert_eq!(
        at["typedAt"][0]["bottom"][1], "❯",
        "the screen as it stood before the key"
    );
    // The window draws what it was given, then a second character: a screen of its own.
    window.print("\x1b[2;3Hab\x1b[2;5H");
    window.type_key("b");
    let at = window.unsent_at().unwrap();
    assert_eq!(at["typedAt"].as_array().unwrap().len(), 2);
    assert_eq!(at["typedAt"][1]["bottom"][1], "❯ ab");
    // The same screen again is not kept twice: it takes the newer one's `current`.
    window.printed.counts().took(1);
    window.type_key("c");
    let at = window.unsent_at().unwrap();
    assert_eq!(at["typedAt"].as_array().unwrap().len(), 2);
    assert_eq!(at["typedAt"][1]["current"], false);
    window.printed.counts().took(3);
    window
        .screen
        .feed_noting(b"\x1b[0m", &window.printed, |_| {});
    // A key that adds none keeps the screen before it, once, and takes no screen away.
    window.type_key("\x17");
    window.print("\x1b[2;3H\x1b[K");
    window.type_key("\x7f");
    let at = window.unsent_at().unwrap();
    assert_eq!(at["typedAt"].as_array().unwrap().len(), 2);
    assert_eq!(
        at["pressedOver"]["bottom"][1], "❯ ab",
        "the first such write's"
    );
    // A character forgets it.
    window.type_key("c");
    assert!(window.unsent_at().unwrap().get("pressedOver").is_none());
    // A write that leaves the count at zero forgets every screen.
    window.type_key("\x15");
    assert_eq!(window.unsent_at(), None);
    // A write that holds a character and other keys counts as one that adds.
    window.type_key("\x1b[Dd");
    let at = window.unsent_at().unwrap();
    assert_eq!(at["typedAt"].as_array().unwrap().len(), 1);
    assert!(at.get("pressedOver").is_none());
    // What the terminal reports of itself moves nothing.
    window.type_key("\x1b[I");
    assert!(window.unsent_at().unwrap().get("pressedOver").is_none());
    // 32 screens at the most: one more says so and is not kept.
    window.type_key("\x15");
    for key in 0..33 {
        window.print(&format!("\x1b[2;3H{key:02}"));
        window.type_key("z");
        let at = window.unsent_at().unwrap();
        assert_eq!(at["typedAt"].as_array().unwrap().len(), (key + 1).min(32));
        assert_eq!(at["typedMore"], key >= 32, "{key}");
    }
    window.type_key("\x15");
    window.type_key("a");
    assert_eq!(window.unsent_at().unwrap()["typedMore"], false);
}

/// A screen kept while the pane was behind is marked so, and admits nothing by itself:
/// "erased" asks the screen over the key, "elsewhere" the last screen or that one (T-2255).
#[test]
fn the_host_admits_evidence_only_where_it_holds_that_screen_current() {
    let behind = |window: &Boxed, key: &str| {
        window.printed.counts().took(1);
        window.type_key(key);
        window.printed.counts().took(3);
        window
            .screen
            .feed_noting(b"\x1b[0m", &window.printed, |_| {});
    };
    let refused = |window: &Boxed, by: Evidence| {
        let at = window.look();
        assert!(
            matches!(window.paste_by(at, by), Err(ArbiterError::Unsent)),
            "{by:?}"
        );
        assert_eq!(window.pasted(), 0);
    };
    // The only character's screen was behind.
    let window = Boxed::new();
    behind(&window, "X");
    assert_eq!(window.unsent_at().unwrap()["typedAt"][0]["current"], false);
    refused(&window, Evidence::Elsewhere);
    refused(&window, Evidence::Erased);
    // The screen over a key that added none was behind.
    window.type_key("\r");
    behind(&window, "Y");
    behind(&window, "\x17");
    assert_eq!(window.unsent_at().unwrap()["pressedOver"]["current"], false);
    refused(&window, Evidence::Erased);
    refused(&window, Evidence::Elsewhere);
    // The last of two screens was behind, the first current.
    window.type_key("\r");
    window.type_key("a");
    window.print("\x1b[2;3Ha");
    behind(&window, "b");
    refused(&window, Evidence::Elsewhere);
    // More characters than screens are kept of.
    window.type_key("\r");
    for key in 0..33 {
        window.print(&format!("\x1b[2;3H{key:02}"));
        window.type_key("z");
    }
    refused(&window, Evidence::Elsewhere);
    // The screen at the next key, current, stands for a last screen that was behind.
    window.type_key("\r");
    behind(&window, "v");
    window.type_key("\x1b");
    let at = window.look();
    window
        .paste_by(at, Evidence::Elsewhere)
        .expect("the screen at Escape was current");
    assert_eq!(window.pasted(), 1);
    // Two characters, each on a current screen.
    window.type_key("a");
    window.type_key("b");
    let at = window.look();
    window
        .paste_by(at, Evidence::Elsewhere)
        .expect("a screen for each key");
    assert_eq!(window.pasted(), 2);
}

/// Main's journal and N17's counts meet in one step (T-2198 note 7): a window that keeps a journal
/// takes a read into its screen, then into its journal, then notes the clock and counts it drawn,
/// all with the screen's lock held, so that no look sees a read in the screen and not in the
/// journal's offsets, nor counts it drawn before the journal has it.
#[test]
fn a_read_is_in_the_screen_and_the_journal_before_it_is_counted_drawn() {
    let arbiter = InputArbiter::new(EnterTiming::fixed(0));
    let key = PaneKey::new("journaled", 1);
    let printed = arbiter.register(&key).expect("register");
    let screen = PaneScreen::new(24, 80);
    let journal = crate::journal::Journal::new((24, 80));
    printed.counts().took(3);
    let mut between = None;
    screen.feed_noting(b"abc", &printed, |bytes| {
        between = Some((printed.counts().both().1, journal.end(), bytes.len()));
        journal.append(bytes);
    });
    assert_eq!(
        between,
        Some((0, 0, 3)),
        "inside the lock the read is not counted drawn, and the journal appends there"
    );
    assert_eq!((journal.end(), printed.counts().both()), (3, (3, 3)));
    // And the production step does both.
    printed.counts().took(2);
    super::took_in_journaled(&screen, &printed, &journal, b"de");
    assert_eq!((journal.end(), printed.counts().both()), (5, (5, 5)));
}

/// A page that holds its output back: the reader is held at the room, and what the window prints
/// stays at its terminal, where the host's question finds it, on a real terminal (a ConPTY on
/// Windows). The pane is behind though every byte read is drawn: the look says so, and a paste
/// and an Enter of a look are refused.
#[test]
fn a_reader_held_at_the_room_is_behind_on_a_real_terminal() {
    let _pty_guard = crate::pty::serial_pty_test();
    let panes = Arc::new(crate::pty::PaneTable::new());
    let key = PaneKey::new("room", 1);
    #[cfg(unix)]
    let argv: Vec<String> = [
        "/bin/sh",
        "-c",
        "head -c 65536 /dev/zero | tr '\\0' a; sleep 30",
    ]
    .map(str::to_owned)
    .to_vec();
    #[cfg(windows)]
    let argv = crate::pty::conpty_test::powershell("[Console]::Write('a' * 65536); Start-Sleep 30");
    let cwd = std::env::temp_dir();
    let streamed = panes
        .open_streamed_at(
            key.clone(),
            &cwd,
            &argv,
            crate::pty::PaneEnvironment::new(&Default::default(), &[]),
            portable_pty::PtySize {
                rows: 8,
                cols: 64,
                pixel_width: 0,
                pixel_height: 0,
            },
            4096,
        )
        .expect("open the program");
    let arbiter = InputArbiter::new(EnterTiming::fixed(0));
    let printed = arbiter
        .register_counted(&key, None, &streamed.counts)
        .expect("register");
    let screen = Arc::clone(&streamed.screen);
    let consumer = {
        let (screen, printed, output) =
            (Arc::clone(&screen), Arc::clone(&printed), streamed.output);
        thread::spawn(move || {
            while let Ok(message) = output.recv() {
                super::took_in(&screen, &printed, &message.bytes);
            }
        })
    };
    let look = || snapshot_of(&arbiter, &key, Some(Arc::clone(&screen)), None, Some(8)).unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    let held = loop {
        let now = look();
        if now["outputDrawn"].as_u64() >= Some(4096) && now["outputRead"] == now["outputDrawn"] {
            break now;
        }
        assert!(
            Instant::now() < deadline,
            "the room was never filled: {now}"
        );
        thread::sleep(Duration::from_millis(5));
    };
    assert_eq!(held["outputCaughtUp"], false, "{held}");
    let at = At {
        seen: Seen::parse(held["seen"].as_str().unwrap()).unwrap(),
        typed: 0,
    };
    let refused = arbiter.write_paste_via(
        panes.as_ref(),
        &key,
        Paste {
            body: b"DELIVERY",
            at: Some(at),
            read_empty: None,
            clear_first: None,
        },
    );
    assert_eq!(why(refused), Some(Why::OutputPending), "a paste");
    let refused = arbiter.enter(panes.as_ref(), &key, at);
    assert_eq!(why(refused), Some(Why::OutputPending), "an Enter");
    let _ = panes.kill(&key);
    let _ = consumer.join();
}

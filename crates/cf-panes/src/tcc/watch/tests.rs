//! The reader's supervisor (T-1194) on a scripted reader, a made-up table of windows and a
//! clock that only moves when the code waits: when it starts and ends the reader, what it
//! tells of a dialog and of a log it cannot read, and how it starts again.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use super::*;

const MEDIA_LIBRARY: &str = include_str!("../../../tests/fixtures/tcc/media-library-prompt.ndjson");
/// The stand-in app of the recording, and the `ls` it ran.
const APP: u32 = 22911;
const LS: u32 = 22913;

/// What a scripted reader does next.
#[derive(Clone)]
enum Step {
    Line(String),
    /// Nothing comes for this long.
    Quiet(Duration),
    /// The table of windows changes.
    Windows(usize),
    /// The supervisor is told to stop.
    Stop,
    End,
}

/// The test's world: one clock, one table of windows, what was told.
struct World {
    clock: Cell<Duration>,
    base: Instant,
    open: Cell<usize>,
    /// The window that holds the process of the recording.
    holds: Cell<bool>,
    told: RefCell<Vec<String>>,
    /// The scripts the readers started, one after the other, play.
    scripts: RefCell<VecDeque<Vec<Step>>>,
    started: Cell<u32>,
    ended_by_us: Cell<u32>,
    sleeps: RefCell<Vec<Duration>>,
    stop: AtomicBool,
    /// When this many sleeps have been made the test stops the supervisor.
    stop_after_sleeps: Cell<usize>,
}

impl World {
    fn new(open: usize, scripts: Vec<Vec<Step>>) -> Self {
        Self {
            clock: Cell::new(Duration::ZERO),
            base: Instant::now(),
            open: Cell::new(open),
            holds: Cell::new(true),
            told: RefCell::new(Vec::new()),
            scripts: RefCell::new(scripts.into()),
            started: Cell::new(0),
            ended_by_us: Cell::new(0),
            sleeps: RefCell::new(Vec::new()),
            stop: AtomicBool::new(false),
            stop_after_sleeps: Cell::new(usize::MAX),
        }
    }

    fn advance(&self, by: Duration) {
        self.clock.set(self.clock.get() + by);
    }

    fn run(&self) {
        let start = |app: u32| -> Result<Box<dyn Stream + '_>, String> {
            assert_eq!(app, APP);
            self.started.set(self.started.get() + 1);
            match self.scripts.borrow_mut().pop_front() {
                Some(script) => Ok(Box::new(Scripted {
                    world: self,
                    steps: script.into(),
                    asked: false,
                })),
                None => Err("no more readers in the script".to_owned()),
            }
        };
        supervise(APP, &start, self, self, self, &self.stop);
    }

    fn told(&self) -> Vec<String> {
        self.told.borrow().clone()
    }
}

struct Scripted<'a> {
    world: &'a World,
    steps: VecDeque<Step>,
    asked: bool,
}

impl Stream for Scripted<'_> {
    fn recv(&mut self, wait: Duration) -> Read {
        loop {
            if self.asked {
                return Read::End;
            }
            match self.steps.pop_front() {
                Some(Step::Line(line)) => return Read::Line(line),
                Some(Step::Quiet(for_how_long)) => {
                    let _ = wait;
                    self.world.advance(for_how_long);
                    return Read::Quiet;
                }
                Some(Step::Windows(open)) => self.world.open.set(open),
                Some(Step::Stop) => self.world.stop.store(true, Ordering::SeqCst),
                Some(Step::End) | None => return Read::End,
            }
        }
    }

    fn end(&mut self) {
        self.asked = true;
        self.world.ended_by_us.set(self.world.ended_by_us.get() + 1);
    }
}

impl Windows for World {
    fn open(&self) -> usize {
        self.open.get()
    }
    fn holding(&self, pid: u32) -> Option<PaneKey> {
        (pid == LS && self.holds.get()).then(|| PaneKey::new("p1-zeus", 2))
    }
    fn any(&self) -> Option<PaneKey> {
        (self.open.get() > 0).then(|| PaneKey::new("p1-zeus", 2))
    }
}

impl Tells for World {
    fn prompt(&self, window: Option<PaneKey>, prompt: &Prompt) {
        let window = window.map_or("no window".to_owned(), |key| {
            format!("{}#{}", key.id, key.generation)
        });
        self.told.borrow_mut().push(format!(
            "prompt {} {} {} in {window}",
            prompt.service, prompt.program, prompt.pid
        ));
    }
    fn unreadable(&self, window: Option<PaneKey>, said: &str) {
        let window = window.map_or("no window".to_owned(), |key| key.id);
        self.told
            .borrow_mut()
            .push(format!("unreadable {said:?} in {window}"));
    }
}

impl Time for World {
    fn now(&self) -> Instant {
        self.base + self.clock.get()
    }
    fn sleep(&self, how_long: Duration) {
        self.sleeps.borrow_mut().push(how_long);
        self.advance(how_long);
        assert!(
            self.sleeps.borrow().len() < 10_000,
            "the supervisor never ends"
        );
        if self.sleeps.borrow().len() >= self.stop_after_sleeps.get() {
            self.stop.store(true, Ordering::SeqCst);
        }
    }
}

fn banner() -> Step {
    Step::Line("Filtering the log data using \"subsystem == \\\"com.apple.TCC\\\"\"".to_owned())
}

fn recorded() -> Vec<Step> {
    MEDIA_LIBRARY
        .lines()
        .map(|line| Step::Line(line.to_owned()))
        .collect()
}

#[test]
fn a_dialog_in_the_apps_name_is_told_with_the_window_that_holds_its_process() {
    let mut script = vec![banner()];
    script.extend(recorded());
    script.push(Step::Quiet(Duration::from_secs(1)));
    script.push(Step::End);
    let world = World::new(1, vec![script]);
    world.stop_after_sleeps.set(1);
    world.run();

    assert_eq!(world.told(), ["prompt MediaLibrary ls 22913 in p1-zeus#2"]);
}

#[test]
fn a_process_no_window_holds_is_told_with_no_window() {
    let mut script = vec![banner()];
    script.extend(recorded());
    script.push(Step::End);
    let world = World::new(1, vec![script]);
    world.holds.set(false);
    world.stop_after_sleeps.set(1);
    world.run();

    assert_eq!(world.told(), ["prompt MediaLibrary ls 22913 in no window"]);
}

#[test]
fn nothing_is_read_while_no_window_is_open_and_a_reader_starts_when_one_opens() {
    let world = World::new(
        0,
        vec![vec![
            banner(),
            Step::Quiet(Duration::from_secs(1)),
            Step::End,
        ]],
    );
    // The windows are opened from the world's side after three idle polls.
    world.stop_after_sleeps.set(3);
    world.run();
    assert_eq!(world.started.get(), 0);
    assert_eq!(*world.sleeps.borrow(), [IDLE_POLL; 3]);

    let world = World::new(1, vec![vec![banner(), Step::End]]);
    world.stop_after_sleeps.set(1);
    world.run();
    assert_eq!(world.started.get(), 1);
}

#[test]
fn a_log_the_reader_is_refused_is_told_once_with_its_words_and_never_read_again() {
    let refused = vec![
        Step::Line("log: Must be admin to run 'stream' command".to_owned()),
        Step::End,
    ];
    let world = World::new(1, vec![refused, vec![banner(), Step::End]]);
    world.run();

    assert_eq!(
        world.told(),
        ["unreadable \"log: Must be admin to run 'stream' command\" in p1-zeus"]
    );
    assert_eq!(world.started.get(), 1, "no second reader");
}

#[test]
fn a_reader_that_cannot_be_started_is_told_unreadable_with_why() {
    let world = World::new(1, vec![]);
    world.run();

    assert_eq!(
        world.told(),
        ["unreadable \"no more readers in the script\" in p1-zeus"]
    );
}

#[test]
fn a_reader_that_ends_again_and_again_at_once_is_called_unreadable_after_three() {
    let quick = || vec![banner(), Step::End];
    let world = World::new(1, vec![quick(), quick(), quick(), quick()]);
    world.run();

    assert_eq!(world.started.get(), QUICK_ENDS);
    assert_eq!(world.told().len(), 1);
    assert!(
        world.told()[0].starts_with("unreadable"),
        "{:?}",
        world.told()
    );
}

#[test]
fn a_reader_that_ended_is_started_again_after_pauses_that_double_and_start_over_after_a_steady_run()
{
    let long = |seconds: u64| {
        vec![
            banner(),
            Step::Quiet(Duration::from_secs(seconds)),
            Step::End,
        ]
    };
    // Ends after 6 s (not quick), again, again, again, then one that ran 90 s, then another: the
    // pauses double, start over after the steady run, and the next failure doubles them again.
    let world = World::new(
        1,
        vec![long(6), long(6), long(6), long(6), long(90), long(6)],
    );
    world.stop_after_sleeps.set(6);
    world.run();

    let secs: Vec<u64> = world
        .sleeps
        .borrow()
        .iter()
        .map(Duration::as_secs)
        .collect();
    assert_eq!(secs, [1, 2, 4, 8, 1, 2], "{secs:?}");
}

#[test]
fn the_reader_is_ended_when_no_window_has_been_open_for_a_while_and_started_when_one_opens() {
    // A window closes; ten seconds on the reader is asked to end.
    let first = vec![
        banner(),
        Step::Windows(0),
        Step::Quiet(Duration::from_secs(4)),
        Step::Quiet(Duration::from_secs(4)),
        Step::Quiet(Duration::from_secs(4)),
        Step::Quiet(Duration::from_secs(4)),
    ];
    let world = World::new(
        1,
        vec![
            first,
            vec![banner(), Step::Quiet(Duration::from_secs(1)), Step::End],
        ],
    );
    world.stop_after_sleeps.set(3);
    world.run();

    assert_eq!(world.ended_by_us.get(), 1);
    assert!(world.told().is_empty(), "{:?}", world.told());
    // Ended by us is no failure: no second reader until a window opens, and none did.
    assert_eq!(world.started.get(), 1);
}

#[test]
fn a_stop_ends_the_reader_and_none_is_started_once_it_is_set() {
    let world = World::new(
        1,
        vec![vec![
            banner(),
            Step::Stop,
            Step::Quiet(Duration::from_secs(1)),
            Step::Quiet(Duration::from_secs(1)),
        ]],
    );
    world.run();
    assert_eq!(world.started.get(), 1);
    assert_eq!(world.ended_by_us.get(), 1);

    let world = World::new(1, vec![vec![banner(), Step::End]]);
    world.stop.store(true, Ordering::SeqCst);
    world.run();
    assert_eq!(world.started.get(), 0);
}

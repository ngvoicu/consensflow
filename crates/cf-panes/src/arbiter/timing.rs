use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use cf_proto::panes::SilentPaste;

use super::paste::characters_in;

/// Where the arbiter reads the time from, and rests by while it waits. Every moment it keeps
/// or compares comes through here: the paste-hold rules weigh how long a window printed and
/// was quiet against numbers of milliseconds, so a test that raced a thread's sleep against
/// them failed wherever the runner woke the thread late (T-1231). In production it is the
/// wall clock; the tests of this module give it a clock that moves only when they say.
pub(crate) trait Clock: Send + Sync {
    fn now(&self) -> Instant;
    /// Rests for `duration`, as a wait between two looks does.
    fn rest(&self, duration: Duration);
}

/// The time of the machine.
pub(super) struct WallClock;

impl Clock for WallClock {
    fn now(&self) -> Instant {
        Instant::now()
    }

    fn rest(&self, duration: Duration) {
        std::thread::sleep(duration);
    }
}

/// Whole milliseconds from `earlier` to `now`; none before it.
pub(super) fn millis_since(now: Instant, earlier: Instant) -> u64 {
    u64::try_from(now.saturating_duration_since(earlier).as_millis()).unwrap_or(u64::MAX)
}

/// When one generation of a pane last printed, and when what its screen shows last
/// changed. Its output thread notes every chunk here and takes no lock: output never
/// waits on the pane's input.
pub struct OutputClock {
    clock: Arc<dyn Clock>,
    started: Instant,
    /// Milliseconds after `started` of the last output, plus one; 0 before
    /// the pane has printed anything.
    printed: AtomicU64,
    /// Milliseconds after `started` of the last chunk that changed what the screen
    /// shows, plus one; 0 before any has.
    shown: AtomicU64,
    /// How many bytes the window has printed, read and drawn: output is pending while more
    /// was read than the screen shows.
    counts: Arc<OutputCount>,
}

/// The bytes of a window's output, counted twice, each by a counter that only grows: `read`,
/// advanced by the thread that reads the pty the moment the bytes are in hand, before any queue
/// or lock; and `drawn`, advanced by whoever feeds the screen when each feed ends (N17, T-2125).
/// Output is pending while more was read than drawn, and the window is quiet only once both are
/// equal again.
#[derive(Debug, Default)]
pub struct OutputCount {
    read: AtomicU64,
    drawn: AtomicU64,
    /// How many feeds changed what the screen shows.
    shown: AtomicU64,
    /// The feed panicked: the screen is partly written and the window's generation is over.
    broken: AtomicBool,
}

#[cfg(test)]
thread_local! {
    /// What a test does between the two loads of [`OutputCount::both`], once.
    pub(crate) static BETWEEN_LOADS: std::cell::Cell<Option<Box<dyn FnOnce()>>> =
        const { std::cell::Cell::new(None) };
}

#[cfg(test)]
thread_local! {
    /// What a test does once the pair of [`OutputCount::both`] is formed, once.
    pub(crate) static AFTER_LOADS: std::cell::Cell<Option<Box<dyn FnOnce()>>> =
        const { std::cell::Cell::new(None) };
}

impl OutputCount {
    /// `bytes` came off the pty.
    pub(crate) fn took(&self, bytes: usize) {
        self.read.fetch_add(bytes as u64, Ordering::AcqRel);
    }

    /// A feed changed what the screen shows.
    pub(crate) fn showed(&self) {
        self.shown.fetch_add(1, Ordering::AcqRel);
    }

    pub(crate) fn shown(&self) -> u64 {
        self.shown.load(Ordering::Acquire)
    }

    /// `bytes` are in the screen now.
    pub(crate) fn drew(&self, bytes: usize) {
        self.drawn.fetch_add(bytes as u64, Ordering::AcqRel);
    }

    /// `(read, drawn)`. The drawn count is loaded first and the read count after it, so that a
    /// read a moment later is never missed: the pair may say pending where nothing is, never
    /// the reverse.
    pub(crate) fn both(&self) -> (u64, u64) {
        let drawn = self.drawn.load(Ordering::Acquire);
        #[cfg(test)]
        if let Some(between) = BETWEEN_LOADS.with(|slot| slot.take()) {
            between();
        }
        let pair = (self.read.load(Ordering::Acquire), drawn);
        #[cfg(test)]
        if let Some(after) = AFTER_LOADS.with(|slot| slot.take()) {
            after();
        }
        pair
    }

    /// Whether the screen lacks bytes the pty has given.
    pub(crate) fn pending(&self) -> bool {
        let (read, drawn) = self.both();
        read > drawn || self.is_broken()
    }

    /// Nothing more will be drawn of what was read (the output's consumer is gone): the count
    /// is settled where it stands, so that no wait is held on bytes that cannot arrive.
    pub(crate) fn give_up(&self) {
        let read = self.read.load(Ordering::Acquire);
        self.drawn.fetch_max(read, Ordering::AcqRel);
    }

    /// The feed panicked.
    pub(crate) fn break_feed(&self) {
        self.broken.store(true, Ordering::Release);
    }

    pub(crate) fn is_broken(&self) -> bool {
        self.broken.load(Ordering::Acquire)
    }
}

impl OutputClock {
    pub(super) fn new(clock: Arc<dyn Clock>, counts: Arc<OutputCount>) -> Self {
        Self {
            started: clock.now(),
            clock,
            printed: AtomicU64::new(0),
            shown: AtomicU64::new(0),
            counts,
        }
    }

    /// The window's byte counts.
    pub(crate) fn counts(&self) -> &Arc<OutputCount> {
        &self.counts
    }

    /// The pane printed `changed` now: whether it changed what its screen shows
    /// ([`crate::screen::Screen::feed`]).
    pub(crate) fn note(&self, changed: bool) {
        let now = millis_since(self.clock.now(), self.started).saturating_add(1);
        self.printed.store(now, Ordering::Release);
        if changed {
            self.counts.showed();
            self.shown.store(now, Ordering::Release);
        }
    }

    /// [`OutputClock::note`] for a test of another crate that plays the pane host's output
    /// side: the window printed (T-1332).
    #[cfg(feature = "test-support")]
    pub fn note_output(&self, changed: bool) {
        self.note(changed);
    }

    /// How long the pane had printed nothing at `now`, once it has printed anything.
    pub(super) fn quiet_ms(&self, now: Instant) -> Option<u64> {
        if self.counts.pending() {
            return Some(0);
        }
        self.since(&self.printed, now)
    }

    /// How long what the pane's screen shows had not changed at `now`, once it has shown
    /// anything.
    pub(super) fn screen_quiet_ms(&self, now: Instant) -> Option<u64> {
        self.since(&self.shown, now)
    }

    fn since(&self, clock: &AtomicU64, now: Instant) -> Option<u64> {
        let at = clock.load(Ordering::Acquire);
        (at != 0).then(|| millis_since(now, self.started).saturating_sub(at - 1))
    }
}

/// When a paste's Enter goes: once the window has taken the paste in, that
/// is, printed something after it (its echo), `drawn_after_ms` or more after it,
/// and then nothing for `quiet_ms`; never sooner than `least_ms`, and at
/// `most_ms` whatever it printed. A window silent since the paste may still be
/// reading it: Devin draws a long paste's placeholder only once it has read all
/// of it, and an Enter before that finds nothing to send. What a window prints
/// sooner than `drawn_after_ms` after the paste is its answer to the paste's
/// first key, not its draw of the paste (Codex on Windows, [`SilentPaste`]). It
/// waits on the output, not on what the screen shows (T-1083): a window that
/// still prints is still busy with the paste as far as anything here can tell
/// (Codex on Windows asks where the cursor is while it reads one, T-1048), and an
/// Enter too soon goes into the paste; a window that only repaints at rest costs
/// the paste no more than `most_ms`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EnterTiming {
    pub least_ms: u64,
    pub drawn_after_ms: u64,
    pub quiet_ms: u64,
    pub most_ms: u64,
}

impl EnterTiming {
    /// [`EnterTiming::wait`] on the wall clock, for a test of another module of this crate.
    #[cfg(test)]
    pub(crate) fn wait_released(&self, output: &OutputClock, pasted: Instant) {
        self.wait(&WallClock, output, pasted)
    }

    /// [`EnterTiming::wait`] on a test's own clock, for a test of another module of this crate.
    #[cfg(test)]
    pub(crate) fn wait_released_on(
        &self,
        clock: &dyn Clock,
        output: &OutputClock,
        pasted: Instant,
    ) {
        self.wait(clock, output, pasted)
    }

    /// A fixed wait, whatever the window prints.
    pub const fn fixed(milliseconds: u64) -> Self {
        Self {
            least_ms: milliseconds,
            drawn_after_ms: 0,
            quiet_ms: 0,
            most_ms: milliseconds,
        }
    }

    /// When the human's keys after a paste of `characters` go, in a window that reads a paste
    /// silently: never sooner than it could have read it, once it has drawn it and gone quiet,
    /// and at the most its cap for that length.
    pub(super) fn of_silent_paste(silent: SilentPaste, characters: usize) -> Self {
        Self {
            least_ms: silent.read_ms(characters),
            drawn_after_ms: silent.drawn_after_ms,
            quiet_ms: silent.quiet_ms,
            most_ms: silent.most_ms(characters),
        }
    }

    /// The longer of two waits: each of its four numbers is the greater of the two's, so the
    /// Enter goes no sooner than either would have it.
    fn longer_of(self, other: Self) -> Self {
        Self {
            least_ms: self.least_ms.max(other.least_ms),
            drawn_after_ms: self.drawn_after_ms.max(other.drawn_after_ms),
            quiet_ms: self.quiet_ms.max(other.quiet_ms),
            most_ms: self.most_ms.max(other.most_ms),
        }
    }

    /// When the Enter of a delivery of `body` goes, this being the host's wait: that wait, and
    /// where the window's harness declared how it reads a paste, no sooner than that says for
    /// this paste's length. A window that redraws while it reads a long paste never goes quiet
    /// for the host's 120 ms, and its Enter went at the host's most (2 s) while it still read
    /// (Muse on Windows, T-1127). A window that declared nothing has the host's wait alone.
    pub(super) fn for_delivery(self, silent: Option<SilentPaste>, body: &[u8]) -> Self {
        match silent {
            Some(silent) => self.longer_of(Self::of_silent_paste(silent, characters_in(body))),
            None => self,
        }
    }

    /// Waits until the Enter of a paste written at `pasted` may go: at once, where the
    /// window has taken it in already. The Enter does not go while output the pty gave is not
    /// yet in the screen ([`OutputCount`]), up to the cap and [`PENDING_GRACE_MS`] more: a
    /// window that prints without pause for longer than that gets its Enter at the end of it,
    /// as it did before the counts (the cap is a limit, `.specs`).
    pub(super) fn wait(&self, clock: &dyn Clock, output: &OutputClock, pasted: Instant) {
        loop {
            // One look at the time for both numbers: the paste's age and the quiet are of the
            // same moment, however long the thread was kept from running between them.
            let now = clock.now();
            let since = millis_since(now, pasted);
            if output.counts().pending() && since < self.pending_cap_ms() {
                clock.rest(Duration::from_millis(5));
                continue;
            }
            // How long the window has been quiet, once it printed after the paste, and not
            // only at once (its answer to the paste's first key).
            let echoed = output
                .quiet_ms(now)
                .filter(|quiet| *quiet < since && since - quiet >= self.drawn_after_ms);
            if since >= self.most_ms
                || (since >= self.least_ms && echoed.is_some_and(|quiet| quiet >= self.quiet_ms))
            {
                return;
            }
            clock.rest(Duration::from_millis(5));
        }
    }

    /// How long after the paste the Enter waits for output not yet drawn, at the most.
    pub(super) fn pending_cap_ms(&self) -> u64 {
        self.most_ms.saturating_add(PENDING_GRACE_MS)
    }
}

/// How long past its cap a paste's Enter waits for a window that still has output not drawn:
/// less than the five seconds an adapter allows a paste beyond its harness's cap.
pub(super) const PENDING_GRACE_MS: u64 = 3_000;

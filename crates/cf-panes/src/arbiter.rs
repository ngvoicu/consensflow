use std::borrow::Cow;
use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::ops::Range;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use cf_proto::panes::SilentPaste;

use crate::pty::{write_paste_via, PaneError, PaneInputWriter, PaneKey, PaneTable};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SanitizeError {
    ControlByte(u8),
    ControlCharacter(char),
    InvalidUtf8,
}

impl fmt::Display for SanitizeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ControlByte(byte) => {
                write!(formatter, "control byte 0x{byte:02x} is not allowed")
            }
            Self::ControlCharacter(character) => write!(
                formatter,
                "control character U+{:04X} is not allowed",
                u32::from(*character)
            ),
            Self::InvalidUtf8 => write!(formatter, "paste body is not valid UTF-8"),
        }
    }
}

impl std::error::Error for SanitizeError {}

/// Why the arbiter did not take a pane's input. Every one but `Pane` is a
/// refusal before anything was written.
#[derive(Debug)]
pub enum ArbiterError {
    Stale,
    Busy,
    InputFailed,
    /// The human has typed in the window and not sent it yet.
    Unsent,
    /// A paste asked for a window the human has not typed into, and they have
    /// ([`cf_proto::panes::UNTYPED`]).
    TypedInto,
    InvalidBody(SanitizeError),
    /// The write itself failed, once begun: some of the input may have
    /// reached the pane.
    Pane(PaneError),
    LockPoisoned,
}

impl fmt::Display for ArbiterError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Stale => write!(formatter, "stale pane generation"),
            Self::Busy => write!(formatter, "a paste is already in flight"),
            Self::InputFailed => write!(formatter, "pane input is failed until restart"),
            Self::Unsent => write!(formatter, "the human has unsent text in the window"),
            Self::TypedInto => write!(
                formatter,
                "the human has typed into the window since it opened"
            ),
            Self::InvalidBody(error) => write!(formatter, "invalid paste body: {error}"),
            Self::Pane(error) => error.fmt(formatter),
            Self::LockPoisoned => write!(formatter, "input arbiter lock is poisoned"),
        }
    }
}

impl std::error::Error for ArbiterError {}

impl From<PaneError> for ArbiterError {
    fn from(error: PaneError) -> Self {
        Self::Pane(error)
    }
}

impl From<SanitizeError> for ArbiterError {
    fn from(error: SanitizeError) -> Self {
        Self::InvalidBody(error)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArbiterSnapshot {
    pub generation: u64,
    pub paste_in_flight: bool,
    pub input_failed: bool,
    /// The human has typed in the window and not sent it: a paste waits.
    pub unsent: bool,
    /// How many writes of the human's keys the pane has taken: a paste told
    /// the input box read empty at this many goes in over the count
    /// ([`InputArbiter::write_paste`]).
    pub typed: u64,
    /// How long ago the human last typed into the pane, once they have.
    pub typed_quiet_ms: Option<u64>,
    /// How long the pane has printed nothing, once it has printed anything: what a
    /// paste's Enter waits on ([`EnterTiming`]), and a harness's reader of its input box
    /// on whether the window answered the human's last key.
    pub output_quiet_ms: Option<u64>,
    /// How long what the pane's screen shows has not changed, once it has shown
    /// anything ([`crate::screen::Screen::feed`]): how a screen at rest is told from one
    /// at work. A harness can print without end and change nothing shown (T-1083).
    pub screen_quiet_ms: Option<u64>,
    /// How long since a paste was last written into the pane, once one was: a
    /// paste the host held while it was slow is written when it catches up, and
    /// the window's record has had only this long to show it.
    pub paste_quiet_ms: Option<u64>,
    /// The human's last writes into the pane, oldest first: how long ago, how many
    /// bytes, of what kinds, and never a byte of them ([`HumanWrite`]).
    pub human_writes: Vec<HumanWrite>,
}

/// A write of the human's path into a pane, as the pane keeps it for the trace of a
/// delivery held for what the human typed (T-839: what raised the count of keys of a
/// window nobody typed in is unproven, Calliope's T-844): how long ago it came, how many
/// bytes it held, and of which kinds ([`CLASSES`]). Its bytes are never kept.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HumanWrite {
    pub ago_ms: u64,
    pub length: usize,
    pub classes: Vec<&'static str>,
}

/// How many of the human's last writes a pane keeps.
const HUMAN_WRITES: usize = 8;

/// The kinds of bytes a write of the human's path holds, by the names a snapshot says
/// them in, which the daemon's trace reads them by.
const CLASSES: [&str; 9] = cf_proto::panes::HUMAN_WRITE_CLASSES;

/// Which of [`CLASSES`] a write holds, a bit each.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Classes(u16);

impl Classes {
    fn with(&mut self, class: &str) {
        if let Some(at) = CLASSES.iter().position(|known| *known == class) {
            self.0 |= 1 << at;
        }
    }

    fn names(self) -> Vec<&'static str> {
        CLASSES
            .iter()
            .enumerate()
            .filter(|(at, _)| self.0 & (1 << at) != 0)
            .map(|(_, name)| *name)
            .collect()
    }
}

/// What a pane keeps of one of the human's writes: when, how many bytes, of what kinds.
#[derive(Debug, Clone, Copy)]
struct Written {
    at: Instant,
    length: usize,
    classes: Classes,
}

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
struct WallClock;

impl Clock for WallClock {
    fn now(&self) -> Instant {
        Instant::now()
    }

    fn rest(&self, duration: Duration) {
        std::thread::sleep(duration);
    }
}

/// Whole milliseconds from `earlier` to `now`; none before it.
fn millis_since(now: Instant, earlier: Instant) -> u64 {
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
}

impl OutputClock {
    fn new(clock: Arc<dyn Clock>) -> Self {
        Self {
            started: clock.now(),
            clock,
            printed: AtomicU64::new(0),
            shown: AtomicU64::new(0),
        }
    }

    /// The pane printed `changed` now: whether it changed what its screen shows
    /// ([`crate::screen::Screen::feed`]).
    pub(crate) fn note(&self, changed: bool) {
        let now = millis_since(self.clock.now(), self.started).saturating_add(1);
        self.printed.store(now, Ordering::Release);
        if changed {
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
    fn quiet_ms(&self, now: Instant) -> Option<u64> {
        self.since(&self.printed, now)
    }

    /// How long what the pane's screen shows had not changed at `now`, once it has shown
    /// anything.
    fn screen_quiet_ms(&self, now: Instant) -> Option<u64> {
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
    fn of_silent_paste(silent: SilentPaste, characters: usize) -> Self {
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
    fn for_delivery(self, silent: Option<SilentPaste>, body: &[u8]) -> Self {
        match silent {
            Some(silent) => self.longer_of(Self::of_silent_paste(silent, characters_in(body))),
            None => self,
        }
    }

    /// Waits until the Enter of a paste written at `pasted` may go: at once, where the
    /// window has taken it in already.
    fn wait(&self, clock: &dyn Clock, output: &OutputClock, pasted: Instant) {
        loop {
            // One look at the time for both numbers: the paste's age and the quiet are of the
            // same moment, however long the thread was kept from running between them.
            let now = clock.now();
            let since = millis_since(now, pasted);
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
}

/// One pane's input. A pane's worker runs its keys, replies, pastes and
/// claims one at a time, in order, so nothing waits in here: none of the
/// human's keys can land inside a paste or before its Enter. What the human
/// typed and has not sent holds a paste back until they send it or erase it
/// (`unsent`, a count of characters as near as keys tell): a delivery is
/// never pasted into their text (the owner's choice, 2026-10-03, after
/// "send right away" on 10-01 pasted over what he was typing). A native
/// send's claim leaves the input box alone, so unsent text holds none. Keys
/// are a guess: those typed into a harness's dialog and left there with
/// Escape count as unsent though the box is empty. So a paste whose adapter
/// read the box empty on the screen, with no key of the human's since
/// (`typed`), goes in, and the count is cleared (T-288). The lock is never
/// held across a write.
struct PaneInputState {
    generation: u64,
    paste_in_flight: bool,
    input_failed: bool,
    unsent: usize,
    typed: u64,
    typed_at: Option<Instant>,
    output: Arc<OutputClock>,
    /// When a paste was last written into this generation of the pane.
    pasted: Option<Instant>,
    /// How the window's harness reads a human's paste, where it reads one silently: its
    /// adapter's word, given as the pane opened ([`InputArbiter::register_with`]).
    silent_paste: Option<SilentPaste>,
    /// When the human's last paste was written, and when their keys after it may go, while
    /// those keys have not gone ([`InputArbiter::write_typed`]).
    human_pasted: Option<(Instant, EnterTiming)>,
    /// Where the human's input stands in a paste, across the writes it comes in
    /// ([`InputArbiter::write_typed`]), in a window that reads a paste silently.
    paste: PasteWatch,
    /// The human's last writes into this generation, oldest first ([`HUMAN_WRITES`]).
    writes: VecDeque<Written>,
}

type PaneState = Arc<Mutex<PaneInputState>>;
type PaneStateMap = HashMap<String, PaneState>;

impl PaneInputState {
    fn new(generation: u64, clock: &Arc<dyn Clock>) -> Self {
        Self {
            generation,
            paste_in_flight: false,
            input_failed: false,
            unsent: 0,
            typed: 0,
            typed_at: None,
            output: Arc::new(OutputClock::new(Arc::clone(clock))),
            pasted: None,
            silent_paste: None,
            human_pasted: None,
            paste: PasteWatch::default(),
            writes: VecDeque::new(),
        }
    }

    /// What the pane holds at `now`: every age in it is of that one moment, so the human's
    /// writes, read one after another, are never aged from different ones and an older write
    /// never reads younger than a newer.
    fn snapshot(&self, now: Instant) -> ArbiterSnapshot {
        ArbiterSnapshot {
            generation: self.generation,
            paste_in_flight: self.paste_in_flight,
            input_failed: self.input_failed,
            unsent: self.unsent > 0,
            typed: self.typed,
            typed_quiet_ms: self.typed_at.map(|at| millis_since(now, at)),
            output_quiet_ms: self.output.quiet_ms(now),
            screen_quiet_ms: self.output.screen_quiet_ms(now),
            paste_quiet_ms: self.pasted.map(|pasted| millis_since(now, pasted)),
            human_writes: self
                .writes
                .iter()
                .map(|written| HumanWrite {
                    ago_ms: millis_since(now, written.at),
                    length: written.length,
                    classes: written.classes.names(),
                })
                .collect(),
        }
    }
}

pub struct InputArbiter {
    panes: Mutex<PaneStateMap>,
    enter: EnterTiming,
    clock: Arc<dyn Clock>,
}

impl InputArbiter {
    pub fn new(enter: EnterTiming) -> Self {
        Self::with_clock(enter, Arc::new(WallClock))
    }

    /// An arbiter that reads the time from `clock`: the wall clock, or a test's own.
    pub(crate) fn with_clock(enter: EnterTiming, clock: Arc<dyn Clock>) -> Self {
        Self {
            panes: Mutex::new(HashMap::new()),
            enter,
            clock,
        }
    }

    /// Takes a pane in, or a newer generation of it. The answer is the
    /// generation's output clock, for the pane's output thread to note.
    pub fn register(&self, pane: &PaneKey) -> Result<Arc<OutputClock>, ArbiterError> {
        self.register_with(pane, None)
    }

    /// Takes a pane in as [`InputArbiter::register`] does, with how its harness reads a
    /// human's paste where it reads one silently (`pane.open`'s `silentPaste`).
    pub fn register_with(
        &self,
        pane: &PaneKey,
        silent_paste: Option<SilentPaste>,
    ) -> Result<Arc<OutputClock>, ArbiterError> {
        let output = self.register_inner(pane, || {}, || {})?;
        let state = self.pane_state(pane)?;
        let mut state = lock_state(&state)?;
        if state.generation == pane.generation {
            state.silent_paste = silent_paste;
        }
        Ok(output)
    }

    /// How the pane's harness reads a paste, as it was registered.
    #[cfg(test)]
    pub(crate) fn silent_paste(&self, pane: &PaneKey) -> Option<SilentPaste> {
        let state = self.pane_state(pane).ok()?;
        let state = lock_state(&state).ok()?;
        state.silent_paste
    }

    #[cfg(test)]
    fn register_with_hook<F>(
        &self,
        pane: &PaneKey,
        before_replace: F,
    ) -> Result<Arc<OutputClock>, ArbiterError>
    where
        F: FnMut(),
    {
        self.register_inner(pane, before_replace, || {})
    }

    #[cfg(test)]
    fn register_with_locked_hook<F>(
        &self,
        pane: &PaneKey,
        before_commit: F,
    ) -> Result<Arc<OutputClock>, ArbiterError>
    where
        F: FnMut(),
    {
        self.register_inner(pane, || {}, before_commit)
    }

    fn register_inner<F, G>(
        &self,
        pane: &PaneKey,
        mut before_replace: F,
        mut before_commit: G,
    ) -> Result<Arc<OutputClock>, ArbiterError>
    where
        F: FnMut(),
        G: FnMut(),
    {
        let state = {
            let mut panes = self.lock_panes()?;
            match panes.get(&pane.id) {
                Some(state) => Arc::clone(state),
                None => {
                    let fresh = PaneInputState::new(pane.generation, &self.clock);
                    let output = Arc::clone(&fresh.output);
                    panes.insert(pane.id.clone(), Arc::new(Mutex::new(fresh)));
                    return Ok(output);
                }
            }
        };

        {
            let state = lock_state(&state)?;
            if state.generation > pane.generation {
                return Err(ArbiterError::Stale);
            }
            if state.generation == pane.generation {
                return Ok(Arc::clone(&state.output));
            }
        }

        before_replace();
        let mut state = lock_state(&state)?;
        if state.generation > pane.generation {
            return Err(ArbiterError::Stale);
        }
        if state.generation == pane.generation {
            return Ok(Arc::clone(&state.output));
        }
        if state.paste_in_flight {
            return Err(ArbiterError::Busy);
        }
        before_commit();
        *state = PaneInputState::new(pane.generation, &self.clock);
        Ok(Arc::clone(&state.output))
    }

    /// A pane gone from the table leaves the arbiter. A newer generation of
    /// the same id that took its place keeps its own state.
    pub(crate) fn retire(&self, pane: &PaneKey) {
        let Ok(mut panes) = self.lock_panes() else {
            return;
        };
        let current = panes
            .get(&pane.id)
            .and_then(|state| lock_state(state).ok().map(|state| state.generation));
        if current == Some(pane.generation) {
            panes.remove(&pane.id);
        }
    }

    pub fn snapshot(&self, pane: &PaneKey) -> Result<ArbiterSnapshot, ArbiterError> {
        let state = self.pane_state(pane)?;
        let state = lock_state(&state)?;
        validate_generation(&state, pane)?;
        let snapshot = state.snapshot(self.clock.now());
        Ok(snapshot)
    }

    /// The human's own keys, counted for what they leave unsent in the window's input box.
    /// They go at once, but in a window whose harness reads a paste silently
    /// ([`SilentPaste`]), where a write ends a paste of theirs (one that began in an earlier
    /// write, or ends in a marker split by a write's end: [`PasteWatch`]), what comes after
    /// it, in that write or a later one, waits until the window has taken the paste in: on Windows the
    /// console hands a paste on as key presses, and Codex took an Enter that came while it
    /// was still reading them for a line break in the paste (T-1025, T-1048).
    pub(crate) fn write_typed(
        &self,
        table: &PaneTable,
        pane: &PaneKey,
        bytes: &[u8],
    ) -> Result<(), ArbiterError> {
        self.write_typed_via(table, pane, bytes)
    }

    fn write_typed_via<W: PaneInputWriter + ?Sized>(
        &self,
        writer: &W,
        pane: &PaneKey,
        bytes: &[u8],
    ) -> Result<(), ArbiterError> {
        // What this write holds of keys and of paste, a paste of the human's that may have
        // begun in an earlier write, and where it ends a paste.
        let (silent, fed) = {
            let state = self.pane_state(pane)?;
            let mut state = lock_state(&state)?;
            let silent = state.silent_paste;
            let fed = if state.generation == pane.generation {
                state.paste.feed(bytes, self.clock.now())
            } else {
                Fed {
                    ends: Vec::new(),
                    segments: vec![Segment::Typed(0..bytes.len())],
                }
            };
            (silent, fed)
        };
        if let Some(silent) = silent {
            for (part, ended) in parts_ending_pastes(bytes, &fed.ends) {
                if !part.is_empty() {
                    self.settle_human_paste(pane)?;
                }
                self.write_via(writer, pane, part)?;
                if let Some(characters) = ended {
                    let timing = EnterTiming::of_silent_paste(silent, characters);
                    let state = self.pane_state(pane)?;
                    let mut state = lock_state(&state)?;
                    if state.generation == pane.generation {
                        state.human_pasted = Some((self.clock.now(), timing));
                    }
                }
            }
        } else {
            self.write_via(writer, pane, bytes)?;
        }
        let state = self.pane_state(pane)?;
        let mut state = lock_state(&state)?;
        if state.generation == pane.generation {
            let read = keys_of(state.unsent, bytes, &fed.segments);
            state.unsent = read.unsent;
            let now = self.clock.now();
            state.paste.idle(now);
            // What came, and when, and never a byte of it (`HumanWrite`).
            if state.writes.len() == HUMAN_WRITES {
                state.writes.pop_front();
            }
            state.writes.push_back(Written {
                at: now,
                length: bytes.len(),
                classes: read.classes,
            });
            // A write that holds no key (what the terminal reports of itself) is no key
            // of the human's: the adapter's wait for their last key does not start again.
            if read.keyed {
                state.typed += 1;
                state.typed_at = Some(now);
            }
        }
        Ok(())
    }

    /// Waits, where the human's last paste has keys of theirs after it not yet gone, until the
    /// window has taken the paste in, as a delivery's Enter waits: from when it was written.
    fn settle_human_paste(&self, pane: &PaneKey) -> Result<(), ArbiterError> {
        let state = self.pane_state(pane)?;
        let pasted = {
            let mut state = lock_state(&state)?;
            if state.generation != pane.generation {
                return Ok(());
            }
            state
                .human_pasted
                .take()
                .map(|(at, timing)| (at, timing, Arc::clone(&state.output)))
        };
        if let Some((at, timing, output)) = pasted {
            timing.wait(self.clock.as_ref(), &output, at);
        }
        Ok(())
    }

    /// The emulator's replies and the daemon's own keys, written at once.
    pub(crate) fn write(
        &self,
        table: &PaneTable,
        pane: &PaneKey,
        bytes: &[u8],
    ) -> Result<(), ArbiterError> {
        self.write_via(table, pane, bytes)
    }

    fn write_via<W: PaneInputWriter + ?Sized>(
        &self,
        writer: &W,
        pane: &PaneKey,
        bytes: &[u8],
    ) -> Result<(), ArbiterError> {
        let state = self.pane_state(pane)?;
        validate_admission(&*lock_state(&state)?, pane)?;
        if bytes.is_empty() {
            return Ok(());
        }
        writer
            .write(pane, bytes)
            .map_err(|error| fail_input(&state, pane, error, false))
    }

    /// A paste and its Enter, held while the human's keys left text unsent,
    /// unless `read_empty`: the window's adapter read its input box empty when
    /// the pane had taken that many writes of the human's keys, and none came
    /// since. With `untyped`, refused unwritten once any key of the human's has
    /// reached this generation of the pane ([`cf_proto::panes::UNTYPED`]).
    pub(crate) fn write_paste(
        &self,
        table: &PaneTable,
        pane: &PaneKey,
        body: &[u8],
        read_empty: Option<u64>,
        untyped: bool,
    ) -> Result<(), ArbiterError> {
        self.write_paste_into_via(table, pane, body, read_empty, untyped)
    }

    /// Admit a native-channel send (a harness's own server, inbox or socket)
    /// as a paste is admitted: the pane is current and its input works. It
    /// reserves nothing and sends nothing; its place in the pane's worker is
    /// what keeps it out of a paste going in.
    pub(crate) fn claim(&self, pane: &PaneKey) -> Result<(), ArbiterError> {
        let state = self.pane_state(pane)?;
        let state = lock_state(&state)?;
        validate_admission(&state, pane)
    }

    /// A paste and its Enter, into any window the human has typed into or not.
    #[cfg(test)]
    fn write_paste_via<W: PaneInputWriter + ?Sized>(
        &self,
        writer: &W,
        pane: &PaneKey,
        body: &[u8],
        read_empty: Option<u64>,
    ) -> Result<(), ArbiterError> {
        self.write_paste_into_via(writer, pane, body, read_empty, false)
    }

    /// A paste and its Enter. While it goes in, the pane's snapshot says so,
    /// and a newer generation of the pane cannot take its place. The human's
    /// keys reach the pane through its worker one write at a time, as this
    /// paste does, so what `untyped` reads of them under the lock is every key
    /// before the paste, and none after it.
    fn write_paste_into_via<W: PaneInputWriter + ?Sized>(
        &self,
        writer: &W,
        pane: &PaneKey,
        body: &[u8],
        read_empty: Option<u64>,
        untyped: bool,
    ) -> Result<(), ArbiterError> {
        let body = sanitize(body)?;
        let state = self.pane_state(pane)?;
        let (output, enter) = {
            let mut state = lock_state(&state)?;
            validate_admission(&state, pane)?;
            if untyped && state.typed > 0 {
                return Err(ArbiterError::TypedInto);
            }
            if state.unsent > 0 {
                if read_empty != Some(state.typed) {
                    return Err(ArbiterError::Unsent);
                }
                state.unsent = 0;
            }
            state.paste_in_flight = true;
            // The host's wait, or the longer of it and how the window's harness reads a paste of
            // this length where it declared that.
            (
                Arc::clone(&state.output),
                self.enter.for_delivery(state.silent_paste, &body),
            )
        };
        // The Enter's wait begins once the paste itself is written.
        let mut pasted = false;
        let written = write_paste_via(writer, pane, &body, || {
            pasted = true;
            enter.wait(self.clock.as_ref(), &output, self.clock.now());
        });
        // A paste is answered as it went: a lock poisoned meanwhile fails the
        // pane's next admission, never what this paste already wrote.
        if let Ok(mut state) = lock_state(&state) {
            state.paste_in_flight = false;
            if pasted {
                state.pasted = Some(self.clock.now());
            }
        }
        written.map_err(|error| fail_input(&state, pane, error, pasted))
    }

    fn pane_state(&self, pane: &PaneKey) -> Result<PaneState, ArbiterError> {
        self.lock_panes()?
            .get(&pane.id)
            .cloned()
            .ok_or(ArbiterError::Stale)
    }

    fn lock_panes(&self) -> Result<MutexGuard<'_, PaneStateMap>, ArbiterError> {
        self.panes.lock().map_err(|_| ArbiterError::LockPoisoned)
    }
}

fn lock_state(
    state: &Mutex<PaneInputState>,
) -> Result<MutexGuard<'_, PaneInputState>, ArbiterError> {
    state.lock().map_err(|_| ArbiterError::LockPoisoned)
}

fn validate_generation(state: &PaneInputState, pane: &PaneKey) -> Result<(), ArbiterError> {
    if state.generation == pane.generation {
        Ok(())
    } else {
        Err(ArbiterError::Stale)
    }
}

/// What a write of the human's path holds: how many characters its keys leave unsent
/// in the window's input box, and whether it holds any key at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Keys {
    unsent: usize,
    keyed: bool,
    classes: Classes,
}

/// How many characters the human's keys leave unsent in a window's input
/// box, as near as keys tell ([`keys_of`]).
#[cfg(test)]
fn unsent_after(unsent: usize, bytes: &[u8]) -> usize {
    let fed = PasteWatch::default().feed(bytes, Instant::now());
    keys_of(unsent, bytes, &fed.segments).unsent
}

/// How a bracketed paste begins and ends.
const PASTE_START: &[u8] = b"\x1b[200~";
const PASTE_END: &[u8] = b"\x1b[201~";

/// How many characters `text` holds: its bytes that begin one, which is every one but a
/// UTF-8 continuation byte, so that a character split between two writes is counted once.
fn characters_in(text: &[u8]) -> usize {
    text.iter().filter(|byte| **byte & 0xC0 != 0x80).count()
}

/// Where the human's input stands in a bracketed paste, across the writes it comes in. The
/// page sends input in chunks of 32 KiB (`app/ui/terminal-link.js`), so a big paste is
/// several writes, with its opening marker in the first and its closing marker in the last,
/// and a chunk can end inside a marker. It keeps the characters of the paste now open,
/// counted from after its opening marker to its closing one over all the writes it takes,
/// and the last bytes of the writes so far when they could begin the marker looked for next.
/// Those bytes are scan state alone: they went to the window with their write, and are
/// counted as characters of the paste only once it is known they are none of a marker's.
///
/// What it says of each write serves two readers: where a paste ends in it, for the hold of
/// the keys that follow it ([`Fed::ends`]), and which of its bytes are typed keys and how
/// many characters are a paste's, for the count of what is unsent ([`Fed::segments`]): text
/// inside a paste is never keys, whatever write it is in.
#[derive(Debug)]
struct PasteWatch {
    /// The characters of the open paste so far; none while no paste is open.
    open: Option<usize>,
    /// The end of the last write, when it could be the beginning of a marker.
    carried: Vec<u8>,
    /// When the pane last took a write of the human's.
    idle_since: Option<Instant>,
    /// How long it takes nothing before a paste left open is forgotten.
    abandoned_after: Duration,
}

impl Default for PasteWatch {
    fn default() -> Self {
        Self {
            open: None,
            carried: Vec::new(),
            idle_since: None,
            abandoned_after: PASTE_ABANDONED_AFTER,
        }
    }
}

/// A paste cut off, its closing marker never sent (a page that went, a chunk it could not
/// send), is forgotten once the pane has taken nothing of the human's for this long: a chunk
/// of a paste follows the one before it within the time of a call, and otherwise what the
/// human types next would be taken for the rest of the paste, and an Enter among it would
/// send nothing.
const PASTE_ABANDONED_AFTER: Duration = Duration::from_secs(10);

/// A paste that ends in a write: where in it (just after the closing marker), and how many
/// characters the paste held, all of it, whatever writes it came in.
#[derive(Debug, PartialEq, Eq)]
struct PasteEnd {
    at: usize,
    characters: usize,
}

/// A piece of a write of the human's, in order.
#[derive(Debug, PartialEq, Eq)]
enum Segment {
    /// Keys typed: these bytes of the write.
    Typed(Range<usize>),
    /// A paste, or part of one: the characters of it that came in this write, markers left
    /// out. None where only a marker came, which is the human's input all the same.
    Pasted(usize),
}

/// What a write of the human's holds, as [`PasteWatch::feed`] reads it against the paste the
/// writes before it left open.
#[derive(Debug, Default, PartialEq, Eq)]
struct Fed {
    ends: Vec<PasteEnd>,
    segments: Vec<Segment>,
}

impl Fed {
    fn typed(&mut self, from: usize, to: usize) {
        if from < to {
            self.segments.push(Segment::Typed(from..to));
        }
    }
}

impl PasteWatch {
    /// The pastes that end in `bytes`, the human's next write, in order, and what it holds
    /// of keys and of paste. A paste that began in an earlier write is counted from its
    /// opening marker; one still open at the end of `bytes` goes on in the next write; a
    /// marker split by the end of a write is found.
    fn feed(&mut self, bytes: &[u8], now: Instant) -> Fed {
        let cut_off = self
            .idle_since
            .is_some_and(|since| now.saturating_duration_since(since) > self.abandoned_after);
        if cut_off {
            self.open = None;
            self.carried.clear();
        }
        self.idle_since = Some(now);
        let carried = self.carried.len();
        let stream = if carried == 0 {
            Cow::Borrowed(bytes)
        } else {
            Cow::Owned([&self.carried[..], bytes].concat())
        };
        self.carried.clear();
        // Where a place in the stream is in `bytes`: what was carried was in the write before.
        let in_write = |index: usize| index.saturating_sub(carried);
        let mut fed = Fed::default();
        let mut at = 0;
        loop {
            let rest = &stream[at..];
            match self.open {
                None => {
                    let Some(start) = position_of(rest, PASTE_START) else {
                        self.carry(rest, &[PASTE_START]);
                        fed.typed(in_write(at), bytes.len());
                        return fed;
                    };
                    fed.typed(in_write(at), in_write(at + start));
                    fed.segments.push(Segment::Pasted(0));
                    self.open = Some(0);
                    at += start + PASTE_START.len();
                }
                Some(characters) => {
                    let closing = position_of(rest, PASTE_END);
                    let opening = position_of(rest, PASTE_START);
                    if let Some(end) =
                        closing.filter(|end| opening.is_none_or(|start| *end < start))
                    {
                        let content = characters_in(&rest[..end]);
                        at += end + PASTE_END.len();
                        fed.segments.push(Segment::Pasted(content));
                        fed.ends.push(PasteEnd {
                            at: in_write(at),
                            characters: characters + content,
                        });
                        self.open = None;
                    } else if let Some(start) = opening {
                        // An opening marker in a paste starts it again, as a terminal that was
                        // cut off and began another would.
                        fed.segments
                            .push(Segment::Pasted(characters_in(&rest[..start])));
                        self.open = Some(0);
                        at += start + PASTE_START.len();
                    } else {
                        let held = self.carry(rest, &[PASTE_END, PASTE_START]);
                        let content = characters_in(&rest[..rest.len() - held]);
                        fed.segments.push(Segment::Pasted(content));
                        self.open = Some(characters + content);
                        return fed;
                    }
                }
            }
        }
    }

    /// The pane has taken the human's last write now, whatever came of it: a paste cut off
    /// is forgotten once it has taken nothing for long.
    fn idle(&mut self, now: Instant) {
        self.idle_since = Some(now);
    }

    /// Keeps the end of `rest` that could begin one of `markers`, as many bytes as it can, and
    /// says how many it kept.
    fn carry(&mut self, rest: &[u8], markers: &[&[u8]]) -> usize {
        let held = (1..PASTE_END.len())
            .rev()
            .find(|held| {
                rest.len() >= *held
                    && markers
                        .iter()
                        .any(|marker| marker.starts_with(&rest[rest.len() - held..]))
            })
            .unwrap_or(0);
        self.carried = rest[rest.len() - held..].to_vec();
        held
    }
}

/// Where `marker` first is in `bytes`.
fn position_of(bytes: &[u8], marker: &[u8]) -> Option<usize> {
    bytes
        .windows(marker.len())
        .position(|window| window == marker)
}

/// `bytes`, a write of the human's path, in parts that each end where a paste ends (`ends`,
/// from [`PasteWatch::feed`]), each with the characters of the paste it ends; and what follows
/// the last, if anything does. One part, `bytes` itself, where it ends no paste (or is empty).
fn parts_ending_pastes<'a>(bytes: &'a [u8], ends: &[PasteEnd]) -> Vec<(&'a [u8], Option<usize>)> {
    let mut parts = Vec::new();
    let mut from = 0;
    for end in ends {
        parts.push((&bytes[from..end.at], Some(end.characters)));
        from = end.at;
    }
    if from < bytes.len() || parts.is_empty() {
        parts.push((&bytes[from..], None));
    }
    parts
}

/// What `bytes`, a write of the human's path, hold, `unsent` characters left unsent
/// before it, as near as keys tell: each character typed or pasted counts one,
/// Backspace takes one off, and Enter (sent), Ctrl+C or Ctrl+U (the line
/// cleared) leave none. Ctrl+V counts one, for what it pastes from the
/// clipboard (an image's placeholder, in Claude Code). A bracketed paste
/// counts its characters, its line breaks included. Other escape sequences
/// (arrows, function keys, Alt+Enter's new line) and control keys change
/// nothing. What the terminal reports of itself is no key (T-839): a string
/// (OSC, DCS, APC, PM, SOS), to its BEL or ST; a mouse report (`CSI <`, and the
/// legacy `CSI M` with its three bytes); a focus report (`CSI I`, `CSI O`).
///
/// `segments` say which of `bytes` are keys and which a paste ([`PasteWatch::feed`]): a paste
/// is its characters whatever write it is in, line breaks and control bytes among them, and
/// only the bytes outside it are read as keys.
fn keys_of(unsent: usize, bytes: &[u8], segments: &[Segment]) -> Keys {
    let mut keys = Keys {
        unsent,
        keyed: false,
        classes: Classes::default(),
    };
    for segment in segments {
        match segment {
            Segment::Typed(range) => typed_keys(&mut keys, &bytes[range.clone()]),
            Segment::Pasted(characters) => {
                keys.classes.with("paste");
                keys.keyed = true;
                keys.unsent += characters;
            }
        }
    }
    keys
}

/// What the keys `bytes` leave unsent, added to `keys` ([`keys_of`]): none of them is in a
/// paste.
fn typed_keys(keys: &mut Keys, bytes: &[u8]) {
    let mut unsent = keys.unsent;
    let mut keyed = keys.keyed;
    let mut classes = keys.classes;
    let mut at = 0;
    while at < bytes.len() {
        let rest = &bytes[at..];
        match rest[0] {
            0x1b if rest.get(1).is_some_and(|kind| b"]P_^X".contains(kind)) => {
                classes.with("string");
                at += 2 + string_end(&rest[2..]);
                continue;
            }
            0x1b if rest.starts_with(b"\x1b[M") => {
                classes.with("mouse");
                at += 6.min(rest.len());
                continue;
            }
            0x1b if rest.starts_with(b"\x1b[<")
                || rest.starts_with(b"\x1b[I")
                || rest.starts_with(b"\x1b[O") =>
            {
                classes.with(if rest[2] == b'<' { "mouse" } else { "focus" });
                at += csi_end(rest);
                continue;
            }
            _ => keyed = true,
        }
        match rest[0] {
            b'\r' => {
                classes.with("enter");
                unsent = 0;
            }
            // A line feed is Ctrl+J: a new line of the draft in every harness (the page writes
            // Shift+Enter as one, T-1336), where Enter, a carriage return, sends it.
            b'\n' => {
                classes.with("text");
                unsent += 1;
            }
            0x03 => {
                classes.with("control");
                unsent = 0;
            }
            0x15 => {
                classes.with("erase");
                unsent = 0;
            }
            0x7f | 0x08 => {
                classes.with("erase");
                unsent = unsent.saturating_sub(1);
            }
            0x16 => {
                classes.with("control");
                unsent += 1;
            }
            0x1b => {
                classes.with("escape");
                at += match rest.get(1) {
                    Some(b'[') => csi_end(rest),
                    Some(b'O') => 3.min(rest.len()),
                    Some(_) => 2,
                    None => 1,
                };
                continue;
            }
            byte if byte < 0x20 => classes.with("control"),
            byte if byte & 0xC0 != 0x80 => {
                classes.with("text");
                unsent += 1;
            }
            _ => {}
        }
        at += 1;
    }
    *keys = Keys {
        unsent,
        keyed,
        classes,
    };
}

/// How many bytes of `rest`, which begins with a CSI (`ESC [`), are that sequence:
/// to its final byte, or all of them where the write ends first.
fn csi_end(rest: &[u8]) -> usize {
    rest[2..]
        .iter()
        .position(|byte| (0x40..=0x7e).contains(byte))
        .map_or(rest.len(), |end| end + 3)
}

/// How many bytes of `body`, a terminal string's after its opening, are the string
/// with its ending: to its BEL or ST (`ESC \\`), or all of them where the write ends
/// first.
fn string_end(body: &[u8]) -> usize {
    body.iter()
        .enumerate()
        .find_map(|(at, byte)| match byte {
            0x07 => Some(at + 1),
            0x1b if body.get(at + 1) == Some(&b'\\') => Some(at + 2),
            _ => None,
        })
        .unwrap_or(body.len())
}

fn validate_admission(state: &PaneInputState, pane: &PaneKey) -> Result<(), ArbiterError> {
    validate_generation(state, pane)?;
    if state.input_failed {
        return Err(ArbiterError::InputFailed);
    }
    Ok(())
}

/// A write that failed may have reached the pane in part, so the pane takes
/// no more input until a new generation replaces it. A newer generation that
/// took its place meanwhile keeps working input. But a write that found the
/// pane gone from the table (killed, or ended, while the input waited its
/// turn) wrote nothing: an input no earlier write had `begun` is refused, as
/// it is once the pane's input is retired, and fails nothing.
fn fail_input(state: &PaneState, pane: &PaneKey, error: PaneError, begun: bool) -> ArbiterError {
    if !begun && matches!(error, PaneError::NotFound(_)) {
        return ArbiterError::Stale;
    }
    if let Ok(mut state) = lock_state(state) {
        if state.generation == pane.generation {
            state.input_failed = true;
        }
    }
    error.into()
}

pub(crate) fn sanitize(body: &[u8]) -> Result<Vec<u8>, SanitizeError> {
    let mut sanitized = Vec::with_capacity(body.len());
    let mut index = 0;
    while index < body.len() {
        let byte = body[index];
        if byte == b'\r' && body.get(index + 1) == Some(&b'\n') {
            sanitized.push(b'\n');
            index += 2;
            continue;
        }
        sanitized.push(byte);
        index += 1;
    }

    let text = std::str::from_utf8(&sanitized).map_err(|_| SanitizeError::InvalidUtf8)?;
    for character in text.chars() {
        if character.is_control() && !matches!(character, '\t' | '\n') {
            return if character.is_ascii() {
                Err(SanitizeError::ControlByte(character as u8))
            } else {
                Err(SanitizeError::ControlCharacter(character))
            };
        }
    }
    Ok(sanitized)
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use std::collections::HashMap;
    #[cfg(unix)]
    use std::io::{Read, Result as IoResult};
    #[cfg(unix)]
    use std::path::Path;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{mpsc, Arc, Barrier, Mutex, MutexGuard};
    use std::thread;
    use std::time::{Duration, Instant};

    #[cfg(unix)]
    use portable_pty::PtySize;

    use cf_proto::panes::SilentPaste;

    use super::{
        sanitize, ArbiterError, Clock, EnterTiming, InputArbiter, OutputClock, SanitizeError,
    };
    #[cfg(unix)]
    use crate::pty::{serial_pty_test, OpenedPane};
    use crate::pty::{PaneError, PaneInputWriter, PaneKey, PaneTable};

    /// What the pane printed that changed nothing its screen shows moves its output's
    /// clock and not its screen's (T-1083); a screen that has shown nothing says none.
    #[test]
    fn a_snapshot_says_how_long_the_screen_has_shown_the_same_apart_from_the_output() {
        let Rig { clock, arbiter, .. } = Rig::new(EnterTiming::fixed(0));
        let pane = PaneKey::new("still-pane", 1);
        let printed = arbiter.register(&pane).expect("register");
        let quiet = || {
            let snapshot = arbiter.snapshot(&pane).expect("snapshot");
            (snapshot.output_quiet_ms, snapshot.screen_quiet_ms)
        };
        printed.note(false);
        assert_eq!(quiet(), (Some(0), None), "printed, and shown nothing");
        printed.note(true);
        clock.advance(Duration::from_millis(40));
        printed.note(false);
        assert_eq!(
            quiet(),
            (Some(0), Some(40)),
            "the screen shows what it showed 40 ms ago"
        );
    }

    #[test]
    fn a_snapshot_says_how_long_the_pane_has_printed_nothing() {
        let Rig { clock, arbiter, .. } = Rig::new(EnterTiming::fixed(0));
        let pane = PaneKey::new("quiet-pane", 1);
        let printed = arbiter.register(&pane).expect("register");
        assert_eq!(
            arbiter.snapshot(&pane).expect("snapshot").output_quiet_ms,
            None
        );
        printed.note(true);
        clock.advance(Duration::from_millis(20));
        let quiet = arbiter.snapshot(&pane).expect("snapshot").output_quiet_ms;
        assert_eq!(quiet, Some(20));

        let newer = PaneKey::new("quiet-pane", 2);
        arbiter
            .register(&newer)
            .expect("register a newer generation");
        printed.note(true);
        assert_eq!(
            arbiter.snapshot(&newer).expect("snapshot").output_quiet_ms,
            None,
            "an older generation's output is not this pane's"
        );
    }

    #[cfg(unix)]
    fn terminal_size() -> PtySize {
        PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        }
    }

    #[cfg(unix)]
    fn raw_recorder(table: &PaneTable, byte_count: usize) -> (PaneKey, Box<dyn Read + Send>) {
        let script = format!("stty raw -echo; printf ready; od -An -tx1 -N {byte_count}");
        let OpenedPane { key, mut reader } = table
            .open(
                Path::new("/tmp"),
                &["/bin/sh".to_string(), "-c".to_string(), script],
                &HashMap::new(),
                terminal_size(),
            )
            .expect("open raw recorder");
        await_ready(&mut reader);
        (key, reader)
    }

    /// The recorder's first word, read with a watchdog: on a CI runner this
    /// read once waited for good, holding the PTY tests' lock. After 20 s the
    /// watchdog writes what arrived to stderr (past the test's capture) and
    /// aborts, so the run fails at once and says why.
    #[cfg(unix)]
    fn await_ready(reader: &mut Box<dyn Read + Send>) {
        use std::io::Write as _;
        use std::sync::atomic::AtomicBool;
        let seen = Arc::new(Mutex::new(Vec::new()));
        let done = Arc::new(AtomicBool::new(false));
        let (watched, finished) = (Arc::clone(&seen), Arc::clone(&done));
        thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(20);
            while Instant::now() < deadline {
                if finished.load(Ordering::SeqCst) {
                    return;
                }
                thread::sleep(Duration::from_millis(100));
            }
            let bytes = watched.lock().map(|b| b.clone()).unwrap_or_default();
            let _ = std::io::stderr().write_all(
                format!(
                    "raw recorder never said ready in 20 s; it printed {:?}\n",
                    String::from_utf8_lossy(&bytes)
                )
                .as_bytes(),
            );
            std::process::abort();
        });
        let mut byte = [0_u8; 1];
        while seen.lock().expect("ready bytes").len() < 5 {
            reader.read_exact(&mut byte).expect("raw recorder ready");
            seen.lock().expect("ready bytes").push(byte[0]);
        }
        done.store(true, Ordering::SeqCst);
        assert_eq!(&seen.lock().expect("ready bytes")[..], b"ready");
    }

    #[cfg(unix)]
    fn read_hex(mut reader: Box<dyn Read + Send>) -> String {
        let (sender, receiver) = mpsc::channel::<IoResult<Vec<u8>>>();
        thread::spawn(move || {
            let mut output = Vec::new();
            let result = reader.read_to_end(&mut output).map(|_| output);
            let _ = sender.send(result);
        });
        let output = receiver
            .recv_timeout(Duration::from_secs(3))
            .expect("raw recorder did not reach EOF")
            .expect("read raw recorder");
        String::from_utf8(output)
            .expect("UTF-8 od output")
            .split_whitespace()
            .collect()
    }

    #[derive(Clone, Debug)]
    struct RecordedWrite {
        at: Instant,
        bytes: Vec<u8>,
    }

    /// A clock that moves only when a test moves it, or a wait rests on it, and that has the
    /// events a test scheduled on it (the prints of a window) happen at their own moments on the
    /// way. A test that reads the arbiter's time on it does not depend on how soon a runner wakes
    /// a thread: a window prints at the moment it was told to, whatever the machine is doing,
    /// and a wait sees the time and the prints as of one and the same moment (T-1231: on a
    /// loaded macOS runner a 10 ms sleep of the printer's thread woke past the 50 ms the
    /// arbiter takes for a window's draw).
    struct SimClock {
        base: Instant,
        /// How long the machine really stalls before each look it is given and each event it
        /// fires: none, but in the test that shows no stall moves a moment of the clock's.
        stall: Duration,
        sim: Mutex<Sim>,
    }

    /// What happens at a moment of the clock: when, in what order of being scheduled, and what.
    type Event = (Duration, u64, Box<dyn FnOnce() + Send>);

    #[derive(Default)]
    struct Sim {
        elapsed: Duration,
        scheduled: u64,
        events: Vec<Event>,
    }

    impl SimClock {
        fn new() -> Arc<Self> {
            Self::stalling(Duration::ZERO)
        }

        fn stalling(stall: Duration) -> Arc<Self> {
            Arc::new(Self {
                base: Instant::now(),
                stall,
                sim: Mutex::new(Sim::default()),
            })
        }

        fn sim(&self) -> MutexGuard<'_, Sim> {
            self.sim.lock().expect("the simulated clock")
        }

        /// How much time has passed on it.
        fn elapsed(&self) -> Duration {
            self.sim().elapsed
        }

        /// Has `event` happen `after` now.
        fn at(&self, after: Duration, event: impl FnOnce() + Send + 'static) {
            let mut sim = self.sim();
            let due = sim.elapsed + after;
            sim.scheduled += 1;
            let order = sim.scheduled;
            sim.events.push((due, order, Box::new(event)));
        }

        /// Lets `by` pass: each event that falls in it happens at its own moment, in order, and
        /// no later event or look at the time is before it.
        fn advance(&self, by: Duration) {
            thread::sleep(self.stall);
            let target = self.elapsed() + by;
            loop {
                let next = {
                    let mut sim = self.sim();
                    let next = sim
                        .events
                        .iter()
                        .enumerate()
                        .filter(|(_, (due, _, _))| *due <= target)
                        .min_by_key(|(_, (due, order, _))| (*due, *order))
                        .map(|(index, _)| index);
                    next.map(|index| {
                        let (due, _, event) = sim.events.remove(index);
                        sim.elapsed = sim.elapsed.max(due);
                        event
                    })
                };
                match next {
                    Some(event) => {
                        thread::sleep(self.stall);
                        event();
                    }
                    None => break,
                }
            }
            self.sim().elapsed = target;
        }
    }

    impl Clock for SimClock {
        fn now(&self) -> Instant {
            self.base + self.elapsed()
        }

        fn rest(&self, duration: Duration) {
            self.advance(duration);
        }
    }

    struct RecordingWriter {
        attempts: AtomicUsize,
        fail_on_attempt: Option<usize>,
        records: Mutex<Vec<RecordedWrite>>,
        observed: mpsc::Sender<RecordedWrite>,
        /// The simulated clock its writes are stamped from, where a test has one.
        clock: Option<Arc<SimClock>>,
        /// The window behind the pane, where a test plays one: its output clock, the moments it
        /// prints at, after the first write it is given, and whether those prints change what its
        /// screen shows.
        window: Mutex<Option<PlayedWindow>>,
    }

    /// A window a test plays behind a pane ([`RecordingWriter::window`]).
    type PlayedWindow = (Arc<OutputClock>, Vec<Duration>, bool);

    struct GatedRecordingWriter {
        attempts: AtomicUsize,
        records: Mutex<Vec<Vec<u8>>>,
        first_write: Arc<Barrier>,
        release_first_write: Arc<Barrier>,
    }

    impl PaneInputWriter for GatedRecordingWriter {
        fn write(&self, _pane: &PaneKey, bytes: &[u8]) -> Result<(), PaneError> {
            self.records
                .lock()
                .expect("gated record lock")
                .push(bytes.to_vec());
            if self.attempts.fetch_add(1, Ordering::SeqCst) == 0 {
                self.first_write.wait();
                self.release_first_write.wait();
            }
            Ok(())
        }
    }

    impl RecordingWriter {
        fn new(fail_on_attempt: Option<usize>) -> (Arc<Self>, mpsc::Receiver<RecordedWrite>) {
            Self::stamped_by(None, fail_on_attempt)
        }

        /// A writer that stamps its writes from `clock`.
        fn timed(clock: Arc<SimClock>) -> (Arc<Self>, mpsc::Receiver<RecordedWrite>) {
            Self::stamped_by(Some(clock), None)
        }

        fn stamped_by(
            clock: Option<Arc<SimClock>>,
            fail_on_attempt: Option<usize>,
        ) -> (Arc<Self>, mpsc::Receiver<RecordedWrite>) {
            let (observed, receiver) = mpsc::channel();
            (
                Arc::new(Self {
                    attempts: AtomicUsize::new(0),
                    fail_on_attempt,
                    records: Mutex::new(Vec::new()),
                    observed,
                    clock,
                    window: Mutex::new(None),
                }),
                receiver,
            )
        }

        fn records(&self) -> Vec<RecordedWrite> {
            self.records.lock().expect("record lock").clone()
        }
    }

    impl PaneInputWriter for RecordingWriter {
        fn write(&self, _pane: &PaneKey, bytes: &[u8]) -> Result<(), PaneError> {
            let attempt = self.attempts.fetch_add(1, Ordering::SeqCst) + 1;
            let record = RecordedWrite {
                at: self
                    .clock
                    .as_ref()
                    .map_or_else(Instant::now, |clock| clock.now()),
                bytes: bytes.to_vec(),
            };
            // The window prints in answer to the first thing it is given.
            if let Some(clock) = &self.clock {
                if let Some((output, prints, shows)) =
                    self.window.lock().expect("the window").take()
                {
                    for after in prints {
                        let output = Arc::clone(&output);
                        clock.at(after, move || output.note(shows));
                    }
                }
            }
            self.records
                .lock()
                .expect("record lock")
                .push(record.clone());
            self.observed.send(record).expect("observe write");
            if self.fail_on_attempt == Some(attempt) {
                return Err(PaneError::Io(std::io::Error::other(
                    "injected write failure",
                )));
            }
            Ok(())
        }
    }

    /// An arbiter on a simulated clock, a writer that stamps its writes from the same clock, and
    /// the channel the writer announces them on: a test of what the arbiter does with time lives
    /// in the clock's time, not the machine's.
    struct Rig {
        clock: Arc<SimClock>,
        arbiter: InputArbiter,
        writer: Arc<RecordingWriter>,
        observed: mpsc::Receiver<RecordedWrite>,
    }

    impl Rig {
        fn new(enter: EnterTiming) -> Self {
            Self::on(SimClock::new(), enter)
        }

        /// A rig on a clock beside a machine that stalls for `stall` before every look at the
        /// time and every print.
        fn stalling(enter: EnterTiming, stall: Duration) -> Self {
            Self::on(SimClock::stalling(stall), enter)
        }

        fn on(clock: Arc<SimClock>, enter: EnterTiming) -> Self {
            let arbiter = InputArbiter::with_clock(enter, clock.clone());
            let (writer, observed) = RecordingWriter::timed(Arc::clone(&clock));
            Self {
                clock,
                arbiter,
                writer,
                observed,
            }
        }

        /// A pane of the arbiter's with the window behind it as a test plays one: its harness
        /// reads a paste as `silent` says (none: it declared nothing), and it prints at each of
        /// `prints`, from the first write it is given. A window prints in answer to what it was
        /// given, at moments of the clock's, which no thread has to reach in time.
        fn window(&self, key: &PaneKey, silent: Option<SilentPaste>, prints: Vec<Duration>) {
            self.window_showing(key, silent, prints, true);
        }

        /// [`Rig::window`], where `shows` says whether its prints change what its screen shows:
        /// a window that only repaints prints and shows the same (T-1083).
        fn window_showing(
            &self,
            key: &PaneKey,
            silent: Option<SilentPaste>,
            prints: Vec<Duration>,
            shows: bool,
        ) {
            let output = self
                .arbiter
                .register_with(key, silent)
                .expect("register pane");
            *self.writer.window.lock().expect("the window") = Some((output, prints, shows));
        }

        /// What the writer has written, as `between` and the tests read it.
        fn records(&self) -> Vec<RecordedWrite> {
            self.writer.records()
        }
    }

    /// A window printing every 10 ms, from `from` ms after the first write it is given, for
    /// `printing` ms: its echo of a paste.
    fn printing_from(from: u64, printing: u64) -> Vec<Duration> {
        (0..printing)
            .step_by(10)
            .map(|spent| Duration::from_millis(from + spent))
            .collect()
    }

    /// A window that reads a paste and draws it: it prints at each of `prints` (ms after the
    /// first write it is given) and then every 10 ms for `printing` ms from the last of them.
    fn prints(prints: &[u64], printing: u64) -> Vec<Duration> {
        let last = prints.last().copied().unwrap_or(0);
        prints
            .iter()
            .map(|at| Duration::from_millis(*at))
            .chain(printing_from(last, printing))
            .collect()
    }

    /// The Enter of a paste goes at the first look at the time that finds the rule met, and the
    /// wait looks every 5 ms: so no sooner than the rule asks, `due_ms` after the paste, and
    /// less than one look after it.
    #[track_caller]
    fn assert_enter_at(waited: Duration, due_ms: u64, rule: &str) {
        assert!(
            waited >= Duration::from_millis(due_ms),
            "{waited:?}: {rule}, due at {due_ms} ms"
        );
        assert!(
            waited < Duration::from_millis(due_ms + 5),
            "{waited:?}: later than {rule} asks, due at {due_ms} ms"
        );
    }

    /// How long the human's keys have been quiet, which the daemon's pacing waits on
    /// before it types a message in (3.0.7 chief-meeting, CMT-03): counted from the
    /// human's last key only. The daemon's own keys, its pastes and their Enter leave
    /// it as it was; a key of the human's starts it again.
    #[test]
    fn the_human_s_keys_alone_start_the_quiet_the_pacing_waits_on() {
        let key = PaneKey::new("quiet", 1);
        let Rig {
            clock,
            arbiter,
            writer,
            observed: _observed,
        } = Rig::new(EnterTiming::fixed(0));
        arbiter.register(&key).expect("register pane");
        let quiet = || arbiter.snapshot(&key).expect("snapshot").typed_quiet_ms;
        assert_eq!(quiet(), None, "never typed in");

        arbiter
            .write_typed_via(writer.as_ref(), &key, b"hi\r")
            .expect("the human's keys");
        clock.advance(Duration::from_millis(40));
        arbiter
            .write_via(writer.as_ref(), &key, b"\x03")
            .expect("the daemon's key");
        arbiter
            .write_paste_via(writer.as_ref(), &key, b"a message", None)
            .expect("the daemon's paste");
        assert_eq!(
            quiet(),
            Some(40),
            "the daemon's keys and paste are not the human's"
        );
        arbiter
            .write_typed_via(writer.as_ref(), &key, b"x")
            .expect("the human's key");
        assert_eq!(quiet(), Some(0), "a key of the human's starts it again");
    }

    /// A paste that asks for a window nobody typed into (`untyped`, T-1474) goes in past the
    /// daemon's own keys, the terminal's replies and its reports of itself on the human's path,
    /// and is refused unwritten once a key of the human's came, one that leaves nothing unsent
    /// too; a paste that does not ask goes in as before, and a new generation of the pane, a new
    /// launch, is one nobody typed into.
    #[test]
    fn a_paste_for_a_window_nobody_typed_into_is_refused_unwritten_once_a_key_of_the_humans_came() {
        let key = PaneKey::new("untyped", 1);
        let (writer, _observed) = RecordingWriter::new(None);
        let arbiter = InputArbiter::new(EnterTiming::fixed(0));
        arbiter.register(&key).expect("register pane");
        // The daemon's Escape and Enter (`pane.input`), a reply to where the cursor is
        // (`pane.reply`), and the terminal's reports on the human's path: focus, a mouse
        // report, a colour.
        for own in [b"\x1b".as_slice(), b"\r", b"\x1b[12;1R"] {
            arbiter
                .write_via(writer.as_ref(), &key, own)
                .expect("the daemon's keys");
        }
        for report in [
            b"\x1b[I".as_slice(),
            b"\x1b[O",
            b"\x1b[<0;10;5M",
            b"\x1b]11;rgb:1e1e/1e1e/1e1e\x1b\\",
        ] {
            arbiter
                .write_typed_via(writer.as_ref(), &key, report)
                .expect("a report");
        }
        arbiter
            .write_paste_into_via(writer.as_ref(), &key, b"first", None, true)
            .expect("nobody typed into it");
        // The human's Escape: a key, and nothing left unsent.
        arbiter
            .write_typed_via(writer.as_ref(), &key, b"\x1b")
            .expect("the human's Escape");
        let read = arbiter.snapshot(&key).expect("snapshot");
        assert_eq!((read.typed, read.unsent), (1, false));
        let written = writer.records().len();
        assert!(matches!(
            arbiter.write_paste_into_via(writer.as_ref(), &key, b"second", None, true),
            Err(ArbiterError::TypedInto)
        ));
        assert_eq!(writer.records().len(), written, "nothing of it was written");
        assert!(!arbiter.snapshot(&key).expect("snapshot").paste_in_flight);
        arbiter
            .write_paste_into_via(writer.as_ref(), &key, b"third", None, false)
            .expect("a paste that does not ask goes in as before");
        let next = PaneKey::new("untyped", 2);
        arbiter.register(&next).expect("the pane's next generation");
        arbiter
            .write_paste_into_via(writer.as_ref(), &next, b"fourth", None, true)
            .expect("a new launch nobody typed into");
    }

    /// What the human typed and has not sent holds a paste back until they
    /// send it or erase it, and never a native send's claim, which leaves the
    /// input box alone (the owner's choice, 2026-10-03). The daemon's own keys
    /// are not the human's: they hold nothing.
    #[test]
    fn unsent_typing_holds_a_paste_until_it_is_sent_but_never_a_claim() {
        let key = PaneKey::new("typed", 1);
        let (writer, _observed) = RecordingWriter::new(None);
        let arbiter = InputArbiter::new(EnterTiming::fixed(0));
        arbiter.register(&key).expect("register pane");
        let unsent = || arbiter.snapshot(&key).expect("snapshot").unsent;

        arbiter
            .write_typed_via(writer.as_ref(), &key, b"half a thought")
            .expect("type without sending");
        assert!(unsent());
        arbiter.claim(&key).expect("a native send is admitted");
        assert!(matches!(
            arbiter.write_paste_via(writer.as_ref(), &key, b"result", None),
            Err(ArbiterError::Unsent)
        ));
        arbiter
            .write_typed_via(writer.as_ref(), &key, &[0x7f; 14])
            .expect("erase it all");
        assert!(!unsent(), "erased to nothing, it holds nothing");
        arbiter
            .write_typed_via(writer.as_ref(), &key, b"more")
            .expect("type again");
        assert!(unsent());
        arbiter
            .write_typed_via(writer.as_ref(), &key, b"\r")
            .expect("send it");
        arbiter
            .write_paste_via(writer.as_ref(), &key, b"result", None)
            .expect("a paste goes in once the text is sent");
        arbiter
            .write_via(writer.as_ref(), &key, b"x")
            .expect("the daemon's key");
        assert!(!unsent(), "the daemon's keys are not the human's");

        assert_eq!(
            writer
                .records()
                .into_iter()
                .map(|record| record.bytes)
                .collect::<Vec<_>>(),
            vec![
                b"half a thought".to_vec(),
                vec![0x7f; 14],
                b"more".to_vec(),
                b"\r".to_vec(),
                b"\x1b[200~result\x1b[201~".to_vec(),
                b"\r".to_vec(),
                b"x".to_vec(),
            ]
        );
    }

    /// Keys typed into a harness's dialog and left there with Escape count as
    /// unsent, though the input box is empty (T-288). A paste whose adapter read
    /// the box empty at the count of the human's keys it names goes in and
    /// clears the count; at any other count, or with no read, it is held: a key
    /// of the human's since the read may be text in the box.
    #[test]
    fn a_paste_read_empty_at_the_last_key_goes_in_over_the_count_and_clears_it() {
        let key = PaneKey::new("dialog", 1);
        let (writer, _observed) = RecordingWriter::new(None);
        let arbiter = InputArbiter::new(EnterTiming::fixed(0));
        arbiter.register(&key).expect("register pane");
        let snapshot = || arbiter.snapshot(&key).expect("snapshot");
        assert_eq!((snapshot().typed, snapshot().typed_quiet_ms), (0, None));

        for keys in [&b"/model\r"[..], b"son", b"\x1b"] {
            arbiter
                .write_typed_via(writer.as_ref(), &key, keys)
                .expect("the human's keys");
        }
        arbiter
            .write_via(writer.as_ref(), &key, b"x")
            .expect("the daemon's key");
        let read = snapshot();
        assert!(
            read.unsent,
            "the picker's filter counts, and Escape clears nothing"
        );
        assert_eq!(
            read.typed, 3,
            "three writes of the human's keys, none of the daemon's"
        );
        assert!(read.typed_quiet_ms.is_some());

        for read_empty in [None, Some(2), Some(4)] {
            assert!(
                matches!(
                    arbiter.write_paste_via(writer.as_ref(), &key, b"held", read_empty),
                    Err(ArbiterError::Unsent)
                ),
                "{read_empty:?}"
            );
        }
        arbiter
            .write_paste_via(writer.as_ref(), &key, b"result", Some(3))
            .expect("read empty at the last key, it goes in");
        assert!(!snapshot().unsent, "and the count is cleared");

        arbiter
            .write_typed_via(writer.as_ref(), &key, b"new words")
            .expect("type again");
        assert!(matches!(
            arbiter.write_paste_via(writer.as_ref(), &key, b"held", Some(3)),
            Err(ArbiterError::Unsent)
        ));
        assert_eq!(
            writer.records().last().map(|record| record.bytes.clone()),
            Some(b"new words".to_vec()),
            "nothing held was written"
        );
    }

    /// A Shift+Enter is part of the draft, not a send (T-1336). The page writes it as a line feed
    /// (Ctrl+J), the new line every harness takes on a Mac and Claude Code and Pi take on Windows,
    /// whichever byte carries it: a line feed counts as a character of the draft, so a message
    /// that waits for the person's draft to be sent is still held after one, where it was let
    /// go in over the draft when the line feed counted as an Enter (the count went to nothing).
    #[test]
    fn a_new_line_in_the_draft_is_part_of_the_draft_and_a_delivery_stays_held() {
        let key = PaneKey::new("shift-enter", 1);
        let (writer, _observed) = RecordingWriter::new(None);
        let arbiter = InputArbiter::new(EnterTiming::fixed(0));
        arbiter.register(&key).expect("register pane");
        let unsent = || arbiter.snapshot(&key).expect("snapshot").unsent;
        let kinds = || {
            arbiter
                .snapshot(&key)
                .expect("snapshot")
                .human_writes
                .last()
                .map(|write| write.classes.clone())
                .expect("a write of the human's")
        };

        arbiter
            .write_typed_via(writer.as_ref(), &key, b"first line")
            .expect("type the first line");
        arbiter
            .write_typed_via(writer.as_ref(), &key, b"\n")
            .expect("Shift+Enter");
        assert_eq!(kinds(), ["text"], "a new line is typed text, no Enter");
        assert!(unsent(), "the draft is still there");
        assert!(matches!(
            arbiter.write_paste_via(writer.as_ref(), &key, b"result", None),
            Err(ArbiterError::Unsent)
        ));

        // A line feed alone is a draft too: nothing to hold a delivery before it, a line break to hold it after.
        arbiter
            .write_typed_via(writer.as_ref(), &key, &[0x7f; 11])
            .expect("erase it all, the line break too");
        assert!(!unsent(), "erased to nothing, it holds nothing");
        arbiter
            .write_typed_via(writer.as_ref(), &key, b"\n")
            .expect("Shift+Enter in an empty box");
        assert!(unsent(), "a line break is in the box");
        arbiter
            .write_typed_via(writer.as_ref(), &key, b"\r")
            .expect("send it");
        arbiter
            .write_paste_via(writer.as_ref(), &key, b"result", None)
            .expect("a paste goes in once the draft is sent");
    }

    /// What keys leave unsent, as near as keys tell.
    #[test]
    fn unsent_counts_what_the_keys_leave_in_the_input() {
        assert_eq!(super::unsent_after(0, b"abc"), 3);
        assert_eq!(super::unsent_after(3, b"\x7f\x7f"), 1);
        assert_eq!(
            super::unsent_after(1, b"\x7f\x7f"),
            0,
            "never below nothing"
        );
        assert_eq!(super::unsent_after(5, b"\r"), 0, "Enter sends it");
        assert_eq!(
            super::unsent_after(5, b"\n"),
            6,
            "a line feed (Ctrl+J, Shift+Enter's new line) is a character of the draft, no send"
        );
        assert_eq!(super::unsent_after(5, b"\x03"), 0, "Ctrl+C clears the line");
        assert_eq!(super::unsent_after(5, b"\x15"), 0, "and Ctrl+U");
        assert_eq!(
            super::unsent_after(0, "ăîș—".as_bytes()),
            4,
            "characters, not bytes"
        );
        assert_eq!(
            super::unsent_after(2, b"\x1b[D\x1b[A\x1bOP"),
            2,
            "arrows and function keys change nothing"
        );
        assert_eq!(
            super::unsent_after(2, b"\x1b\r"),
            2,
            "Alt+Enter's new line sends nothing"
        );
        assert_eq!(
            super::unsent_after(0, b"\x1b[200~two\nlines\x1b[201~"),
            9,
            "a paste counts its line breaks"
        );
        assert_eq!(super::unsent_after(0, b"\x1b[200~unfinished"), 10);
        // The page writes marks as text, in a paste too (T-548): a pasted line of them is
        // its characters, and as many Backspaces leave nothing to hold a delivery over.
        let pasted = super::unsent_after(0, "\x1b[200~ăîș — €\x1b[201~".as_bytes());
        assert_eq!(pasted, 7, "a paste's marks are characters, not bytes");
        assert_eq!(super::unsent_after(pasted, &[0x7f; 7]), 0);
        assert_eq!(
            super::unsent_after(0, b"\x16"),
            1,
            "Ctrl+V pastes from the clipboard"
        );
    }

    /// The characters the pane counts as left unsent in its window's input box.
    fn unsent_of(arbiter: &InputArbiter, key: &PaneKey) -> usize {
        let state = arbiter.pane_state(key).expect("the pane");
        let unsent = super::lock_state(&state).expect("the pane's state").unsent;
        unsent
    }

    /// A paste of the human's of 40,000 characters in 400 lines, as the page sends it: its
    /// opening marker and the first 32 KiB, then the rest and its closing marker.
    fn a_paste_in_chunks_of_lines() -> (Vec<Vec<u8>>, usize) {
        let text = ("x".repeat(99) + "\n").repeat(400);
        let paste = [&b"\x1b[200~"[..], text.as_bytes(), b"\x1b[201~"].concat();
        let chunks = paste.chunks(32 * 1024).map(<[u8]>::to_vec).collect();
        (chunks, text.chars().count())
    }

    /// The count of what is unsent knows a paste across the writes it comes in (T-1127, found
    /// by the check of the first part): the later chunks have no opening marker, and their line
    /// breaks were taken for the Enter that sends it, so a paste of 40,000 characters in lines
    /// left nothing unsent, and a delivery went in over it (the owner's rule of 2026-10-03).
    #[test]
    fn a_paste_in_chunks_with_line_breaks_leaves_every_character_of_it_unsent_and_holds_a_delivery()
    {
        let key = PaneKey::new("pasted-lines-in-chunks", 1);
        let (writer, _observed) = RecordingWriter::new(None);
        let arbiter = InputArbiter::new(EnterTiming::fixed(0));
        arbiter.register(&key).expect("register pane");
        let (chunks, characters) = a_paste_in_chunks_of_lines();
        assert_eq!(chunks.len(), 2, "the page's two chunks");
        assert_eq!(characters, 40_000);
        for chunk in &chunks {
            arbiter
                .write_typed_via(writer.as_ref(), &key, chunk)
                .expect("a chunk of the human's paste");
        }
        assert_eq!(unsent_of(&arbiter, &key), 40_000);
        assert!(arbiter.snapshot(&key).expect("snapshot").unsent);
        assert!(
            matches!(
                arbiter.write_paste_via(writer.as_ref(), &key, b"result", None),
                Err(ArbiterError::Unsent)
            ),
            "a delivery goes in over text the human has not sent"
        );

        // Their Enter sends it, and a delivery goes in after.
        arbiter
            .write_typed_via(writer.as_ref(), &key, b"\r")
            .expect("the human's Enter");
        assert_eq!(unsent_of(&arbiter, &key), 0);
        arbiter
            .write_paste_via(writer.as_ref(), &key, b"result", None)
            .expect("a delivery after the Enter");
    }

    /// A later chunk of a paste is a paste in what the pane says of the human's writes, and
    /// the human's own key: the wait the adapter makes for their last key starts from it.
    #[test]
    fn a_later_chunk_of_a_paste_is_a_paste_and_a_key_of_the_humans() {
        let key = PaneKey::new("chunk-kinds", 1);
        let (writer, _observed) = RecordingWriter::new(None);
        let arbiter = InputArbiter::new(EnterTiming::fixed(0));
        arbiter.register(&key).expect("register pane");
        let (chunks, _) = a_paste_in_chunks_of_lines();
        arbiter
            .write_typed_via(writer.as_ref(), &key, &chunks[0])
            .expect("the first chunk");
        let typed = arbiter.snapshot(&key).expect("snapshot").typed;
        arbiter
            .write_typed_via(writer.as_ref(), &key, &chunks[1])
            .expect("the last chunk");
        let snapshot = arbiter.snapshot(&key).expect("snapshot");
        assert_eq!(snapshot.human_writes[1].classes, ["paste"]);
        assert_eq!(snapshot.typed, typed + 1);
    }

    /// Sets how long the pane takes nothing of the human's before it forgets a paste left open.
    fn forgetting_a_paste_after(arbiter: &InputArbiter, key: &PaneKey, quiet: Duration) {
        let state = arbiter.pane_state(key).expect("the pane");
        super::lock_state(&state)
            .expect("the pane's state")
            .paste
            .abandoned_after = quiet;
    }

    /// A paste whose closing marker never came (the page went, a chunk it could not send) does not
    /// make what the human types after it a paste for good: its Enter sends it, and a delivery
    /// goes in after.
    #[test]
    fn a_paste_cut_off_does_not_swallow_the_keys_typed_after_it() {
        let key = PaneKey::new("cut-off", 1);
        let Rig {
            clock,
            arbiter,
            writer,
            observed: _observed,
        } = Rig::new(EnterTiming::fixed(0));
        arbiter.register(&key).expect("register pane");
        forgetting_a_paste_after(&arbiter, &key, Duration::from_millis(100));
        arbiter
            .write_typed_via(writer.as_ref(), &key, b"\x1b[200~half")
            .expect("half of a paste");
        assert_eq!(unsent_of(&arbiter, &key), 4);
        clock.advance(Duration::from_millis(150));
        arbiter
            .write_typed_via(writer.as_ref(), &key, b"typed\r")
            .expect("what the human typed after");
        assert_eq!(
            unsent_of(&arbiter, &key),
            0,
            "the Enter sent what was typed, and what was cut off with it"
        );
        arbiter
            .write_paste_via(writer.as_ref(), &key, b"result", None)
            .expect("a delivery after the Enter");
    }

    /// The quiet a cut-off paste is forgotten after counts from the end of the pane's last write,
    /// not from its beginning: a chunk that waited behind the hold on the keys before it (the
    /// human's earlier paste) is followed by the next within the time of a call.
    #[test]
    fn the_quiet_that_forgets_a_paste_starts_when_the_panes_last_write_ends() {
        let key = PaneKey::new("slow-chunk", 1);
        let rig = Rig::new(EnterTiming::fixed(0));
        let Rig {
            arbiter, writer, ..
        } = &rig;
        // 300 characters at 1 ms each: the keys after a paste of them wait 300 ms.
        let silent = SilentPaste {
            pace_us_per_character: 1_000,
            quiet_ms: 0,
            ..DRAWS
        };
        rig.window(&key, Some(silent), prints(&[20], 0));
        forgetting_a_paste_after(arbiter, &key, Duration::from_millis(100));
        let first = [&b"\x1b[200~"[..], &[b'x'; 300], b"\x1b[201~"].concat();
        for chunk in [&first[..], b"\x1b[200~y", b"z\n\x1b[201~"] {
            arbiter
                .write_typed_via(writer.as_ref(), &key, chunk)
                .expect("a chunk of the human's input");
        }
        assert_eq!(
            unsent_of(arbiter, &key),
            300 + 3,
            "the second paste went on across its chunks, though the first waited 300 ms"
        );
    }

    /// A marker the end of a write cuts is no characters, nor is it typed: the bytes of a
    /// closing marker that the next write completes are not the paste's, and the opening
    /// marker one write begins and the next ends is the paste's beginning.
    #[test]
    fn a_marker_cut_by_the_end_of_a_write_leaves_no_characters_unsent() {
        let key = PaneKey::new("markers-cut", 1);
        let (writer, _observed) = RecordingWriter::new(None);
        let arbiter = InputArbiter::new(EnterTiming::fixed(0));
        arbiter.register(&key).expect("register pane");
        for write in [&b"typed\x1b[2"[..], b"00~abc\x1b[20", b"1~"] {
            arbiter
                .write_typed_via(writer.as_ref(), &key, write)
                .expect("the human's input");
        }
        // Five typed, three pasted.
        assert_eq!(unsent_of(&arbiter, &key), 8);
    }

    /// What a terminal reports of itself on the human's path is no key either: the count
    /// of the human's writes stays, and so does the time since their last key, which an
    /// adapter waits a second past. A program that asks its terminal every few seconds
    /// restarted that wait at each answer, and a paste read empty raced it (T-839,
    /// Calliope's T-844).
    #[test]
    fn a_terminals_report_moves_neither_the_count_of_keys_nor_the_time_since_the_last() {
        let key = PaneKey::new("reports", 1);
        let Rig {
            clock,
            arbiter,
            writer,
            observed: _observed,
        } = Rig::new(EnterTiming::fixed(0));
        arbiter.register(&key).expect("register pane");
        arbiter
            .write_typed_via(writer.as_ref(), &key, b"x")
            .expect("a key");
        clock.advance(Duration::from_millis(30));
        let reports: [&[u8]; 6] = [
            b"\x1b]11;rgb:1e1e/1e1e/1e1e\x1b\\",
            b"\x1bP>|xterm.js(6.0.0)\x1b\\",
            b"\x1b[<35;80;24M",
            b"\x1b[M #!",
            b"\x1b[I",
            b"\x1b[O",
        ];
        for report in reports {
            arbiter
                .write_typed_via(writer.as_ref(), &key, report)
                .expect("a report");
        }
        let read = arbiter.snapshot(&key).expect("snapshot");
        assert_eq!(read.typed, 1, "the reports are no keys");
        assert_eq!(
            read.typed_quiet_ms,
            Some(30),
            "the time since the last key goes on"
        );
        assert!(read.unsent, "the key typed is still there");
        arbiter
            .write_typed_via(writer.as_ref(), &key, b"\x1b[A")
            .expect("an arrow");
        assert_eq!(
            arbiter.snapshot(&key).expect("snapshot").typed,
            2,
            "an arrow is a key"
        );
    }

    /// What a terminal says of itself on the page's human path types nothing: a string
    /// of OSC (`ESC ]`, a colour asked for), DCS (`ESC P`, the terminal's name), APC,
    /// PM or SOS, ended by BEL or ST (`ESC \`), and a mouse report (`CSI <`, which
    /// Claude Code's fullscreen asks for at every move, `?1003h`). The count took a
    /// string's body for typed characters, so one such reply held every delivery to
    /// the window (T-839).
    #[test]
    fn what_the_terminal_reports_types_nothing() {
        let reports: [&[u8]; 7] = [
            b"\x1b]11;rgb:1e1e/1e1e/1e1e\x1b\\",
            b"\x1b]10;rgb:cccc/cccc/cccc\x07",
            b"\x1bP>|xterm.js(6.0.0)\x1b\\",
            b"\x1bP1$r0;1m\x1b\\",
            b"\x1b_Gi=1;OK\x1b\\",
            b"\x1b^private\x1b\\",
            b"\x1b[<35;80;24M\x1b[<0;12;5m",
        ];
        for report in reports {
            assert_eq!(
                super::unsent_after(2, report),
                2,
                "{:?}",
                String::from_utf8_lossy(report)
            );
        }
        assert_eq!(
            super::unsent_after(0, b"\x1b]11;rgb:1e1e/1e1e/1e1e\x1b\\ab"),
            2,
            "what follows a string is typed"
        );
        assert_eq!(
            super::unsent_after(0, b"\x1b]11;rgb:1e1e"),
            0,
            "nor is a string the write cuts short"
        );
    }

    /// The pane keeps the human's last writes as how long ago, how many bytes and of what
    /// kinds, for the trace of a delivery held for what they typed, and never a byte of
    /// them (Calliope's T-844, T-883): the letters typed are nowhere in the snapshot.
    #[test]
    fn a_pane_keeps_the_humans_last_writes_as_age_length_and_kinds_and_no_byte_of_them() {
        let key = PaneKey::new("ring", 1);
        let Rig {
            clock,
            arbiter,
            writer,
            observed: _observed,
        } = Rig::new(EnterTiming::fixed(0));
        arbiter.register(&key).expect("register pane");
        let writes: [&[u8]; 6] = [
            b"QZXJ",
            b"\r",
            b"\x1b]11;rgb:1e1e/1e1e/1e1e\x1b\\",
            b"\x1b[200~QZ\nXJ\x1b[201~",
            b"\x7f\x1b[A\x03",
            b"\x1b[<35;80;24M\x1b[I",
        ];
        for write in writes {
            arbiter
                .write_typed_via(writer.as_ref(), &key, write)
                .expect("a write");
            clock.advance(Duration::from_millis(1));
        }
        let read = arbiter.snapshot(&key).expect("snapshot");
        let kept: Vec<_> = read
            .human_writes
            .iter()
            .map(|write| (write.length, write.classes.clone()))
            .collect();
        assert_eq!(
            kept,
            [
                (4, vec!["text"]),
                (1, vec!["enter"]),
                (25, vec!["string"]),
                (17, vec!["paste"]),
                (5, vec!["erase", "control", "escape"]),
                (15, vec!["mouse", "focus"]),
            ]
        );
        // One moment ages them all: a millisecond apart, oldest first, and no write reads
        // younger than one after it (T-1000, T-1012: ages read one by one tied or crossed).
        let ages: Vec<u64> = read.human_writes.iter().map(|write| write.ago_ms).collect();
        assert_eq!(ages, [6, 5, 4, 3, 2, 1], "oldest first: {read:?}");
        let shown = format!("{read:?}");
        assert!(
            !shown.contains(['Q', 'Z', 'X', 'J']),
            "no byte of what was typed: {shown}"
        );
        for _ in 0..20 {
            arbiter
                .write_typed_via(writer.as_ref(), &key, b"a")
                .expect("a key");
        }
        let read = arbiter.snapshot(&key).expect("snapshot");
        assert_eq!(read.human_writes.len(), 8, "the last eight");
        // The daemon's own keys and the emulator's replies are no writes of the human's.
        arbiter
            .write_via(writer.as_ref(), &key, b"b")
            .expect("ours");
        assert_eq!(
            arbiter.snapshot(&key).expect("snapshot").human_writes.len(),
            8
        );
    }

    /// A pane that leaves the table takes its state with it, but an older
    /// generation retiring late leaves the newer one's state alone.
    #[test]
    fn a_retired_pane_leaves_and_a_newer_generation_stays() {
        let arbiter = InputArbiter::new(EnterTiming::fixed(0));
        let old = PaneKey::new("worker", 1);
        let new = PaneKey::new("worker", 2);
        arbiter.register(&old).expect("register the old generation");
        arbiter.register(&new).expect("register the new generation");

        arbiter.retire(&old);
        assert_eq!(arbiter.snapshot(&new).expect("still there").generation, 2);
        arbiter.retire(&new);
        assert!(matches!(arbiter.snapshot(&new), Err(ArbiterError::Stale)));
    }

    /// A write waiting on a busy harness holds no lock: the pane's output is
    /// noted and its state read meanwhile. The lock used to be held across
    /// the write, and the output thread took it for every chunk, so a long
    /// paste into a briefly busy harness held its output back from the page.
    #[test]
    fn a_blocked_write_holds_up_no_one_else() {
        let key = PaneKey::new("busy", 1);
        let first_write = Arc::new(Barrier::new(2));
        let release_first_write = Arc::new(Barrier::new(2));
        let writer = Arc::new(GatedRecordingWriter {
            attempts: AtomicUsize::new(0),
            records: Mutex::new(Vec::new()),
            first_write: Arc::clone(&first_write),
            release_first_write: Arc::clone(&release_first_write),
        });
        let arbiter = Arc::new(InputArbiter::new(EnterTiming::fixed(0)));
        let printed = arbiter.register(&key).expect("register pane");

        let writing_arbiter = Arc::clone(&arbiter);
        let writing_key = key.clone();
        let writing = thread::spawn(move || {
            writing_arbiter.write_via(writer.as_ref(), &writing_key, b"typed")
        });
        first_write.wait();
        printed.note(true);
        let (read, snapshot) = mpsc::channel();
        let reading_arbiter = Arc::clone(&arbiter);
        let reading_key = key.clone();
        thread::spawn(move || {
            let quiet = reading_arbiter
                .snapshot(&reading_key)
                .map(|snapshot| snapshot.output_quiet_ms);
            let _ = read.send(quiet.is_ok_and(|quiet| quiet.is_some()));
        });
        // A lock held across the write would hold the read until the writer is released below,
        // which is after this wait: a long deadline costs the green case nothing.
        let read_meanwhile = snapshot.recv_timeout(Duration::from_secs(30));
        release_first_write.wait();
        writing.join().expect("writing thread").expect("the write");
        assert_eq!(read_meanwhile, Ok(true), "the write held the pane's lock");
    }

    #[test]
    fn a_snapshot_says_how_long_since_a_paste_was_written_into_the_pane() {
        let key = PaneKey::new("pasted", 1);
        let Rig {
            clock,
            arbiter,
            writer,
            observed: _observed,
        } = Rig::new(EnterTiming::fixed(0));
        arbiter.register(&key).expect("register pane");
        let pasted = || arbiter.snapshot(&key).expect("snapshot").paste_quiet_ms;
        assert_eq!(pasted(), None, "nothing pasted yet");
        arbiter
            .write_paste_via(writer.as_ref(), &key, b"body", None)
            .expect("write paste");
        clock.advance(Duration::from_millis(20));
        assert_eq!(pasted(), Some(20));
        // The daemon's own keys and the human's are no paste.
        clock.advance(Duration::from_millis(30));
        arbiter
            .write_via(writer.as_ref(), &key, b"\r")
            .expect("a key");
        assert_eq!(pasted(), Some(50));
    }

    #[test]
    fn paste_is_bracketed_then_enter_is_a_delayed_separate_write() {
        let key = PaneKey::new("recorded", 1);
        let Rig {
            clock: _,
            arbiter,
            writer,
            observed: _observed,
        } = Rig::new(EnterTiming::fixed(25));
        arbiter.register(&key).expect("register pane");

        arbiter
            .write_paste_via(writer.as_ref(), &key, b"body", None)
            .expect("write paste");

        let records = writer.records();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].bytes, b"\x1b[200~body\x1b[201~");
        assert_eq!(records[1].bytes, b"\r");
        assert_enter_at(between(&records), 25, "the host's fixed wait");
    }

    /// The same on the machine's own clock, which every pane but a test's has: the Enter waits
    /// at least the host's wait, and a sleep never wakes early, so no load can fail this.
    #[test]
    fn on_the_wall_clock_a_pastes_enter_waits_at_least_the_hosts_wait() {
        let key = PaneKey::new("on-the-wall", 1);
        let (writer, _observed) = RecordingWriter::new(None);
        let arbiter = InputArbiter::new(EnterTiming::fixed(25));
        arbiter.register(&key).expect("register pane");
        arbiter
            .write_paste_via(writer.as_ref(), &key, b"body", None)
            .expect("write paste");
        let records = writer.records();
        assert_eq!(records.len(), 2);
        assert!(records[1].at.duration_since(records[0].at) >= Duration::from_millis(25));
    }

    /// And a window nothing of which prints has its Enter at the most, on the wall clock too.
    #[test]
    fn on_the_wall_clock_a_paste_nothing_echoes_gets_its_enter_at_the_most_and_not_before() {
        let key = PaneKey::new("on-the-wall-silent", 1);
        let (writer, _observed) = RecordingWriter::new(None);
        let arbiter = InputArbiter::new(EnterTiming {
            least_ms: 0,
            drawn_after_ms: 0,
            quiet_ms: 60,
            most_ms: 120,
        });
        arbiter.register(&key).expect("register pane");
        arbiter
            .write_paste_via(writer.as_ref(), &key, b"body", None)
            .expect("write paste");
        let records = writer.records();
        assert_eq!(records.len(), 2);
        assert!(records[1].at.duration_since(records[0].at) >= Duration::from_millis(120));
    }

    /// Pastes `body` through an arbiter with `enter`, the pane printing nothing for `silent_ms`
    /// and then every 10 ms for `printing_ms`, and says how long its Enter waited. What it prints
    /// changes nothing its screen shows: the Enter waits on the window's output all the same, for
    /// a window that still prints may still be taking the paste in (T-1083).
    fn enter_waited(enter: EnterTiming, silent_ms: u64, printing_ms: u64) -> Duration {
        let key = PaneKey::new("echoing", 1);
        let rig = Rig::new(enter);
        rig.window_showing(&key, None, printing_from(silent_ms, printing_ms), false);
        rig.arbiter
            .write_paste_via(rig.writer.as_ref(), &key, b"body", None)
            .expect("write paste");
        between(&rig.records())
    }

    #[test]
    fn a_pastes_enter_waits_for_the_window_to_finish_echoing_it() {
        // The window prints every 10 ms until 240 ms: its echo ends there, and 150 ms of quiet
        // after it is when the Enter goes.
        let enter = EnterTiming {
            least_ms: 0,
            drawn_after_ms: 0,
            quiet_ms: 150,
            most_ms: 5_000,
        };
        let waited = enter_waited(enter, 0, 250);
        assert_enter_at(waited, 240 + 150, "the quiet after the echo");
    }

    #[test]
    fn a_pastes_enter_waits_for_an_echo_that_comes_late() {
        // Devin reading a long paste: silent, then its placeholder drawn at 300 to 340 ms.
        let enter = EnterTiming {
            least_ms: 0,
            drawn_after_ms: 0,
            quiet_ms: 60,
            most_ms: 5_000,
        };
        let waited = enter_waited(enter, 300, 50);
        assert_enter_at(waited, 340 + 60, "the quiet after the late echo");
    }

    #[test]
    fn a_paste_nothing_echoes_gets_its_enter_at_the_most() {
        let enter = EnterTiming {
            least_ms: 0,
            drawn_after_ms: 0,
            quiet_ms: 60,
            most_ms: 300,
        };
        let waited = enter_waited(enter, 0, 0);
        assert_enter_at(waited, 300, "the most");
    }

    #[test]
    fn a_window_that_never_stops_printing_gets_its_enter_at_the_most() {
        // A quiet spell longer than the most: only the most lets Enter go.
        let enter = EnterTiming {
            least_ms: 0,
            drawn_after_ms: 0,
            quiet_ms: 10_000,
            most_ms: 300,
        };
        let waited = enter_waited(enter, 0, 3_000);
        assert_enter_at(waited, 300, "the most");
    }

    /// A window that reads a paste silently and then draws it, with no pace or floor of its
    /// own: the human's keys after a paste wait for its draw and 100 ms of quiet.
    const DRAWS: SilentPaste = SilentPaste {
        pace_us_per_character: 0,
        drawn_after_ms: 0,
        quiet_ms: 100,
        most_ms: 5_000,
        most_us_per_character: 0,
    };

    /// A paste of the human's, as the page's terminal writes one.
    const HUMAN_PASTE: &[u8] = b"\x1b[200~a few\rlines\x1b[201~";

    /// How long after the first of `records` its second was written.
    fn between(records: &[RecordedWrite]) -> Duration {
        records[1].at.duration_since(records[0].at)
    }

    /// In a window that reads a paste's keys silently, a harness takes an Enter that comes
    /// while they are still coming for a line break in the paste (Codex on Windows, T-1025):
    /// the human's paste and its Enter in one write go as two, the Enter once the window has
    /// drawn the paste and gone quiet. The pane keeps the write as the one it was: one write
    /// of the human's, one key more, nothing left unsent.
    #[test]
    fn a_humans_paste_and_its_enter_in_one_write_go_as_two_the_enter_once_the_window_took_it() {
        let key = PaneKey::new("pasted-at-once", 1);
        let rig = Rig::new(EnterTiming::fixed(0));
        let Rig {
            arbiter, writer, ..
        } = &rig;
        // The window draws from 200 to 240 ms.
        rig.window(&key, Some(DRAWS), prints(&[200], 50));
        let at_once = [HUMAN_PASTE, b"\r"].concat();
        arbiter
            .write_typed_via(writer.as_ref(), &key, &at_once)
            .expect("the human's paste and Enter");

        let records = rig.records();
        let written: Vec<&[u8]> = records.iter().map(|record| &record.bytes[..]).collect();
        assert_eq!(written, [HUMAN_PASTE, b"\r"]);
        assert_enter_at(
            between(&records),
            240 + 100,
            "the window's draw of the paste and its quiet",
        );
        let snapshot = arbiter.snapshot(&key).expect("snapshot");
        assert_eq!(snapshot.human_writes.len(), 1, "{snapshot:?}");
        assert_eq!(snapshot.human_writes[0].length, at_once.len());
        assert_eq!(snapshot.typed, 1);
        assert!(!snapshot.unsent, "{snapshot:?}");
    }

    /// A person's Enter, pressed while the window is still reading their paste, waits until
    /// it has drawn it (T-1025: a second was not enough for Codex on Windows).
    #[test]
    fn a_humans_enter_while_the_window_still_reads_their_paste_waits_until_it_drew_it() {
        let key = PaneKey::new("pasted-then-enter", 1);
        let rig = Rig::new(EnterTiming::fixed(0));
        let Rig {
            clock,
            arbiter,
            writer,
            ..
        } = &rig;
        // The window draws from 400 to 440 ms.
        rig.window(&key, Some(DRAWS), prints(&[400], 50));
        arbiter
            .write_typed_via(writer.as_ref(), &key, HUMAN_PASTE)
            .expect("the human's paste");
        clock.advance(Duration::from_millis(100));
        arbiter
            .write_typed_via(writer.as_ref(), &key, b"\r")
            .expect("the human's Enter");

        let records = rig.records();
        assert_eq!(records.len(), 2);
        assert_enter_at(
            between(&records),
            440 + 100,
            "the window's draw of the paste and its quiet",
        );
        assert_eq!(arbiter.snapshot(&key).expect("snapshot").typed, 2);
    }

    /// The wait is from when the paste was written, not from when the Enter came: an Enter
    /// pressed once the window has drawn the paste and gone quiet goes at once, and so does
    /// what the human types after it.
    #[test]
    fn a_humans_enter_after_the_window_took_their_paste_goes_at_once() {
        let key = PaneKey::new("pasted-and-drawn", 1);
        let rig = Rig::new(EnterTiming::fixed(0));
        let Rig {
            clock,
            arbiter,
            writer,
            observed,
        } = &rig;
        // The window draws the paste for 50 ms after it, and then it is quiet.
        rig.window(&key, Some(DRAWS), printing_from(0, 50));
        arbiter
            .write_typed_via(writer.as_ref(), &key, HUMAN_PASTE)
            .expect("the human's paste");
        clock.advance(Duration::from_millis(400));
        observed.try_iter().for_each(drop);

        for keys in [&b"\r"[..], b"next\r"] {
            let pressed = clock.now();
            arbiter
                .write_typed_via(writer.as_ref(), &key, keys)
                .expect("the human's keys");
            let written = observed.try_recv().expect("the keys went").at;
            assert_eq!(
                written.duration_since(pressed),
                Duration::ZERO,
                "{keys:?} waited for a paste the window had taken"
            );
        }
    }

    /// What a window prints at once after a paste is its answer to the paste's first key, not
    /// its draw of the paste: Codex on Windows printed at about 10 ms and drew a long paste only
    /// at about 4.5 s (T-1048). The Enter waits for a print `drawn_after_ms` or more after the
    /// paste.
    #[test]
    fn a_print_at_once_after_the_paste_is_not_its_draw_and_the_enter_waits_for_the_draw() {
        let key = PaneKey::new("first-key-answered", 1);
        let rig = Rig::new(EnterTiming::fixed(0));
        let Rig {
            arbiter, writer, ..
        } = &rig;
        let silent = SilentPaste {
            drawn_after_ms: 50,
            ..DRAWS
        };
        // The answer to the first key comes 10 ms after the paste, the draw 600 ms on: at the
        // moments the clock was told, whatever the machine does meanwhile (CI run 41817130287:
        // on a loaded runner a printer's 10 ms woke past the 50 ms and the Enter went early).
        rig.window(&key, Some(silent), prints(&[10, 600], 0));
        let at_once = [HUMAN_PASTE, b"\r"].concat();
        arbiter
            .write_typed_via(writer.as_ref(), &key, &at_once)
            .expect("the human's paste and Enter");
        assert_enter_at(
            between(&rig.records()),
            600 + 100,
            "the window's draw of the paste and its quiet, not its answer to the first key",
        );
    }

    /// The clock is the only time the rules read: a machine that really stalls for 20 ms before
    /// every look at the time and every print (four looks' worth of the wait's own 5 ms) moves no
    /// moment of the wait. Were the arbiter to read the machine's clock, or a window to print at
    /// the machine's moment, the stalls would show here: the answer to the first key at 10 ms
    /// would be taken for the draw, or the Enter would go at another moment.
    #[test]
    fn a_machine_that_stalls_before_every_look_and_every_print_moves_no_moment_of_the_wait() {
        let key = PaneKey::new("stalled", 1);
        let rig = Rig::stalling(EnterTiming::fixed(0), Duration::from_millis(20));
        let Rig {
            arbiter, writer, ..
        } = &rig;
        let silent = SilentPaste {
            drawn_after_ms: 50,
            ..DRAWS
        };
        rig.window(&key, Some(silent), prints(&[10, 120], 0));
        let at_once = [HUMAN_PASTE, b"\r"].concat();
        arbiter
            .write_typed_via(writer.as_ref(), &key, &at_once)
            .expect("the human's paste and Enter");
        assert_enter_at(
            between(&rig.records()),
            120 + 100,
            "the window's draw of the paste and its quiet, whatever the machine does between",
        );
    }

    /// Never sooner than the window could have read the paste at its pace: a print and a quiet
    /// spell while it reads (a timer of its own) do not let the Enter go.
    #[test]
    fn a_humans_enter_waits_at_least_as_long_as_the_window_takes_to_read_the_paste() {
        let key = PaneKey::new("read-at-its-pace", 1);
        let rig = Rig::new(EnterTiming::fixed(0));
        let Rig {
            arbiter, writer, ..
        } = &rig;
        let silent = SilentPaste {
            pace_us_per_character: 2_000,
            ..DRAWS
        };
        // 300 characters at 2 ms each: 600 ms.
        let paste = [&b"\x1b[200~"[..], &[b'x'; 300], b"\x1b[201~"].concat();
        rig.window(&key, Some(silent), prints(&[100], 0));
        let at_once = [&paste[..], b"\r"].concat();
        arbiter
            .write_typed_via(writer.as_ref(), &key, &at_once)
            .expect("the human's paste and Enter");
        assert_enter_at(
            between(&rig.records()),
            600,
            "the time the window takes to read the paste",
        );
    }

    /// A window that never draws the paste has the human's Enter at its cap for the paste's
    /// length: a second and a millisecond a character for Codex on Windows.
    #[test]
    fn a_window_that_never_draws_the_paste_has_the_enter_at_its_cap_for_the_pastes_length() {
        let key = PaneKey::new("never-drawn", 1);
        let rig = Rig::new(EnterTiming::fixed(0));
        let Rig {
            arbiter, writer, ..
        } = &rig;
        let silent = SilentPaste {
            most_ms: 100,
            most_us_per_character: 1_000,
            ..DRAWS
        };
        // A cap of 100 ms and 300 characters at 1 ms each: 400 ms.
        let paste = [&b"\x1b[200~"[..], &[b'x'; 300], b"\x1b[201~"].concat();
        rig.window(&key, Some(silent), Vec::new());
        let at_once = [&paste[..], b"\r"].concat();
        arbiter
            .write_typed_via(writer.as_ref(), &key, &at_once)
            .expect("the human's paste and Enter");
        assert_enter_at(
            between(&rig.records()),
            400,
            "the cap for the paste's length",
        );
    }

    /// What the pastes that end in each of `writes` (in order, to one watch) held.
    fn ended_in(writes: &[&[u8]]) -> Vec<Vec<super::PasteEnd>> {
        let mut watch = super::PasteWatch::default();
        let now = Instant::now();
        writes
            .iter()
            .map(|write| watch.feed(write, now).ends)
            .collect()
    }

    fn ending(at: usize, characters: usize) -> super::PasteEnd {
        super::PasteEnd { at, characters }
    }

    /// The characters of a paste, not its bytes or its markers, are what its wait is for: all
    /// of them, from its opening marker to its closing one, in however many writes it came.
    #[test]
    fn a_pastes_characters_are_counted_between_its_markers() {
        assert_eq!(ended_in(&[b"\x1b[200~abc\x1b[201~"]), [vec![ending(15, 3)]]);
        let accents = "typed\x1b[200~ăî—\x1b[201~".as_bytes();
        assert_eq!(ended_in(&[accents]), [vec![ending(accents.len(), 3)]]);
        // What follows the closing marker is no part of the paste.
        assert_eq!(
            ended_in(&[b"\x1b[200~abc\x1b[201~\r"]),
            [vec![ending(15, 3)]]
        );
        // The paste of several writes, of which the old count saw the last alone: ten.
        assert_eq!(
            ended_in(&[
                b"\x1b[200~the head of it, ",
                b"and the middle, ",
                b"tail of it\x1b[201~"
            ]),
            [vec![], vec![], vec![ending(16, 16 + 16 + 10)]],
        );
        // Typing outside a paste is not counted, and a paste after a paste is counted afresh.
        assert_eq!(
            ended_in(&[b"typed\x1b[200~ab\x1b[201~more\x1b[200~cde\x1b[201~"]),
            [vec![ending(19, 2), ending(38, 3)]]
        );
    }

    /// A chunk can end inside a marker: its bytes are no characters of the paste, the marker
    /// is found whole when the next write completes it, and bytes that only looked like the
    /// start of one are counted when the next write shows they were none.
    #[test]
    fn a_marker_split_between_writes_is_found_and_none_of_it_is_counted() {
        // The closing marker, split at every place.
        for split in 1..6 {
            let marker = b"\x1b[201~";
            let (first, second) = marker.split_at(split);
            assert_eq!(
                ended_in(&[
                    &[&b"\x1b[200~abc"[..], first].concat(),
                    &[second, b"\r"].concat()
                ]),
                [vec![], vec![ending(6 - split, 3)]],
                "split after {split} bytes"
            );
        }
        // The opening marker, split at every place.
        for split in 1..6 {
            let marker = b"\x1b[200~";
            let (first, second) = marker.split_at(split);
            assert_eq!(
                ended_in(&[
                    &[&b"typed"[..], first].concat(),
                    &[second, b"abc\x1b[201~"].concat()
                ]),
                [vec![], vec![ending(6 - split + 3 + 6, 3)]],
                "split after {split} bytes"
            );
        }
        // Bytes that began a marker and were none: the paste's own, once known.
        assert_eq!(
            ended_in(&[b"\x1b[200~ab\x1b[20", b"0 and more\x1b[201~"]),
            [vec![], vec![ending(16, 2 + 4 + 10)]]
        );
        // A character of several bytes cut between two writes is one.
        let accents = "\x1b[200~ă\x1b[201~".as_bytes();
        let cut = 6 + 1;
        assert_eq!(
            ended_in(&[&accents[..cut], &accents[cut..]]),
            [vec![], vec![ending(accents.len() - cut, 1)]]
        );
    }

    /// What a write holds of keys and of paste, by the places of its bytes: the text of a paste
    /// is its characters, in the write that has its opening marker and in every other, and only
    /// the bytes outside a paste are keys.
    #[test]
    fn a_write_is_keys_and_a_pastes_characters_wherever_the_paste_began() {
        use super::Segment::{Pasted, Typed};
        let mut watch = super::PasteWatch::default();
        let now = Instant::now();
        let mut read = |write: &[u8]| watch.feed(write, now).segments;
        assert_eq!(
            read(b"ab\x1b[200~cd\x1b[201~ef"),
            [Typed(0..2), Pasted(0), Pasted(2), Typed(16..18)]
        );
        // A paste of three writes: the markers in the first and the last, lines in all.
        assert_eq!(read(b"\x1b[200~one\ntwo\n"), [Pasted(0), Pasted(8)]);
        assert_eq!(read(b"three\n\r\x03"), [Pasted(8)]);
        assert_eq!(read(b"four\n\x1b[201~\rx"), [Pasted(5), Typed(11..13)]);
        // Keys typed with the beginning of a marker at the end of the write are keys, and what
        // completes the marker is no characters.
        assert_eq!(read(b"ab\x1b[20"), [Typed(0..6)]);
        assert_eq!(read(b"0~xy\x1b[20"), [Pasted(0), Pasted(2)]);
        assert_eq!(read(b"1~"), [Pasted(0)]);
        // Bytes that began a marker and were none: the paste's own once known.
        assert_eq!(read(b"\x1b[200~ab\x1b[20"), [Pasted(0), Pasted(2)]);
        assert_eq!(read(b"0 and\x1b[201~"), [Pasted(4 + 5)]);
    }

    /// A paste whose closing marker never came (the page went, a chunk it could not send) does
    /// not make the keys typed after it a paste for good: the pane forgets it once it has taken
    /// nothing of the human's for ten seconds, and an Enter then sends what is typed.
    #[test]
    fn a_paste_cut_off_is_forgotten_once_the_pane_has_taken_nothing_for_long() {
        use super::Segment::{Pasted, Typed};
        let cut_off = Instant::now();
        let mut watch = super::PasteWatch::default();
        let _ = watch.feed(b"\x1b[200~half of it", cut_off);
        // A chunk within the time of a call goes on with the paste.
        let soon = cut_off + Duration::from_secs(5);
        assert_eq!(watch.feed(b"more\r", soon).segments, [Pasted(5)]);
        // After a long quiet, the keys are keys.
        let later = soon + Duration::from_secs(11);
        assert_eq!(watch.feed(b"typed\r", later).segments, [Typed(0..6)]);
        // And the pane's own end of a write, when it is slow to finish (a hold on the keys
        // before it), starts the quiet from there.
        let _ = watch.feed(b"\x1b[200~again", later);
        watch.idle(later + Duration::from_secs(40));
        let after = later + Duration::from_secs(45);
        assert_eq!(watch.feed(b"rest\x1b[201~", after).ends.len(), 1);
    }

    /// A paste that is cut off and a new one begun counts from the new one, as one paste of a
    /// single write always did.
    #[test]
    fn a_paste_begun_again_is_counted_from_its_last_opening_marker() {
        assert_eq!(
            ended_in(&[b"\x1b[200~lost\x1b[200~ab", b"c\x1b[201~"]),
            [vec![], vec![ending(7, 3)]]
        );
    }

    /// The page sends the human's input in chunks of 32 KiB (`app/ui/terminal-link.js`), so a
    /// big paste is several writes: the markers are in the first and the last, and the pane host
    /// holds what follows the paste for the whole of it, not for the characters of its last chunk
    /// (T-1127, found by T-1121).
    #[test]
    fn a_humans_paste_sent_in_chunks_is_held_for_all_of_its_characters() {
        let key = PaneKey::new("pasted-in-chunks", 1);
        let rig = Rig::new(EnterTiming::fixed(0));
        let Rig {
            arbiter, writer, ..
        } = &rig;
        // 30,010 characters at 20 µs each: 600 ms, and the window drew them at once.
        let silent = SilentPaste {
            pace_us_per_character: 20,
            ..DRAWS
        };
        rig.window(&key, Some(silent), prints(&[20], 0));
        let first = [&b"\x1b[200~"[..], &[b'x'; 30_000]].concat();
        let last = b"tail of it\x1b[201~".to_vec();
        for chunk in [&first[..], &last[..], b"\r"] {
            arbiter
                .write_typed_via(writer.as_ref(), &key, chunk)
                .expect("a chunk of the human's input");
        }

        let records = rig.records();
        let written: Vec<&[u8]> = records.iter().map(|record| &record.bytes[..]).collect();
        assert_eq!(written, [&first[..], &last[..], b"\r"]);
        assert_enter_at(
            records[2].at.duration_since(records[1].at),
            600,
            "the pace for the paste's thirty thousand characters, not the last chunk's ten",
        );
    }

    /// A chunk can end inside the closing marker: the paste is over all the same, and what
    /// follows it waits, in the write that completes the marker or the next.
    #[test]
    fn a_closing_marker_split_between_two_writes_still_holds_what_follows_the_paste() {
        for (second, then) in [(&b"1~"[..], Some(&b"\r"[..])), (b"1~\r", None)] {
            let key = PaneKey::new("marker-split", 1);
            let rig = Rig::new(EnterTiming::fixed(0));
            let Rig {
                clock,
                arbiter,
                writer,
                ..
            } = &rig;
            rig.window(&key, Some(DRAWS), prints(&[300], 0));
            let first = b"\x1b[200~three\x1b[20";
            arbiter
                .write_typed_via(writer.as_ref(), &key, first)
                .expect("the paste, up to part of its closing marker");
            let completed = clock.now();
            for write in [Some(second), then].into_iter().flatten() {
                arbiter
                    .write_typed_via(writer.as_ref(), &key, write)
                    .expect("the rest of the human's input");
            }

            let written: Vec<Vec<u8>> = rig
                .records()
                .into_iter()
                .map(|record| record.bytes)
                .collect();
            assert_eq!(
                written,
                [first.to_vec(), b"1~".to_vec(), b"\r".to_vec()],
                "the write that completes the marker goes as two, the paste and what follows"
            );
            assert_enter_at(
                rig.records()[2].at.duration_since(completed),
                300 + 100,
                "the window's draw of the paste and its quiet",
            );
        }
    }

    /// The Enter of a delivery (a brief, a note) waits for the longer of the host's wait and
    /// what the window's harness declared of how it reads a paste, for this paste's length: a
    /// window that redraws while it reads never goes quiet for the host's 120 ms, and its Enter
    /// went at the host's most, while it still read (Muse on Windows, T-1127 / T-1121).
    #[test]
    fn a_deliverys_enter_waits_for_the_window_to_have_read_the_paste_where_it_declared_how() {
        let host = EnterTiming {
            least_ms: 10,
            drawn_after_ms: 0,
            quiet_ms: 120,
            most_ms: 300,
        };
        let key = PaneKey::new("delivered-slowly", 1);
        let rig = Rig::new(host);
        let Rig {
            arbiter, writer, ..
        } = &rig;
        // 300 characters at 4 ms each: 1.2 s of reading, drawn all the while (the last print
        // at 1,290 ms), then quiet for the 120 ms the host's wait says.
        let silent = SilentPaste {
            pace_us_per_character: 4_000,
            most_ms: 5_000,
            ..DRAWS
        };
        rig.window(&key, Some(silent), printing_from(0, 1_300));
        let body = vec![b'x'; 300];
        arbiter
            .write_paste_via(writer.as_ref(), &key, &body, None)
            .expect("the delivery");

        let records = rig.records();
        assert_eq!(records.len(), 2, "{records:?}");
        assert_eq!(records[1].bytes, b"\r");
        assert_enter_at(
            between(&records),
            1_290 + 120,
            "the window's read of the paste and its quiet, not the host's most",
        );
    }

    /// The wait of a delivery is the host's and the declared one's longer, each of its four
    /// numbers: a short brief to a window like Devin changes nothing of it, a long one lets it
    /// read, and what is counted is the paste's characters, not its bytes.
    #[test]
    fn a_deliverys_wait_is_the_longer_of_the_hosts_and_the_declared_one_in_each_of_its_numbers() {
        use crate::headless::ENTER;

        let devin = SilentPaste {
            pace_us_per_character: 20,
            drawn_after_ms: 0,
            quiet_ms: 80,
            most_ms: 1_000,
            most_us_per_character: 100,
        };
        // The 252 characters of the longest brief the live suites send: the host's wait as it is.
        assert_eq!(ENTER.for_delivery(Some(devin), &[b'x'; 252]), ENTER);
        assert_eq!(ENTER.for_delivery(None, &[b'x'; 100_000]), ENTER);
        // 20,000 characters: Devin's cap (1 s and 0.1 ms each: 3 s) is past the host's 2 s.
        assert_eq!(
            ENTER.for_delivery(Some(devin), &[b'x'; 20_000]),
            EnterTiming {
                least_ms: 400,
                drawn_after_ms: 0,
                quiet_ms: 120,
                most_ms: 3_000,
            }
        );
        // A window that reads at 0.5 ms a character, is quiet 200 ms and may take a second and
        // a millisecond a character, and 4,000 characters of two bytes each.
        let slow = SilentPaste {
            pace_us_per_character: 500,
            drawn_after_ms: 50,
            quiet_ms: 200,
            most_ms: 1_000,
            most_us_per_character: 1_000,
        };
        assert_eq!(
            ENTER.for_delivery(Some(slow), "ă".repeat(4_000).as_bytes()),
            EnterTiming {
                least_ms: 2_000,
                drawn_after_ms: 50,
                quiet_ms: 200,
                most_ms: 5_000,
            }
        );
    }

    /// A window whose harness declares nothing has its deliveries' Enter after the host's
    /// wait alone, as it always had it, however long it goes on printing.
    #[test]
    fn a_deliverys_enter_in_a_window_that_declared_nothing_waits_only_the_hosts_wait() {
        let host = EnterTiming {
            least_ms: 10,
            drawn_after_ms: 0,
            quiet_ms: 120,
            most_ms: 300,
        };
        let key = PaneKey::new("delivered-undeclared", 1);
        let rig = Rig::new(host);
        let Rig {
            arbiter, writer, ..
        } = &rig;
        rig.window(&key, None, printing_from(0, 1_300));
        arbiter
            .write_paste_via(writer.as_ref(), &key, &vec![b'x'; 300], None)
            .expect("the delivery");

        assert_enter_at(
            between(&rig.records()),
            300,
            "the host's most: a window that declared nothing is not waited on past it",
        );
    }

    /// The human's typing, with no paste, goes as it was written, one write.
    #[test]
    fn the_humans_typing_with_no_paste_goes_in_one_write_at_once() {
        let key = PaneKey::new("typed-alone", 1);
        let (writer, _observed) = RecordingWriter::new(None);
        let arbiter = InputArbiter::new(EnterTiming::fixed(0));
        arbiter
            .register_with(&key, Some(DRAWS))
            .expect("register pane");
        arbiter
            .write_typed_via(writer.as_ref(), &key, b"typed\r")
            .expect("the human's keys");
        let written: Vec<Vec<u8>> = writer
            .records()
            .into_iter()
            .map(|record| record.bytes)
            .collect();
        assert_eq!(written, [b"typed\r".to_vec()]);
    }

    /// A window whose harness declares nothing has the human's paste and its Enter written as
    /// they came, on every platform: only a harness shown to read a paste silently is held.
    #[test]
    fn a_window_whose_harness_declares_no_silent_paste_has_the_humans_keys_as_they_came() {
        let key = PaneKey::new("pasted-here", 1);
        let (writer, _observed) = RecordingWriter::new(None);
        let arbiter = InputArbiter::new(EnterTiming::fixed(10));
        arbiter.register(&key).expect("register pane");
        let at_once = [HUMAN_PASTE, b"\r"].concat();
        arbiter
            .write_typed_via(writer.as_ref(), &key, &at_once)
            .expect("the human's paste and Enter");
        let written: Vec<Vec<u8>> = writer
            .records()
            .into_iter()
            .map(|record| record.bytes)
            .collect();
        assert_eq!(written, [at_once]);
    }

    /// A newer generation of a pane is registered with its own word on how it reads a paste.
    #[test]
    fn a_panes_word_on_how_it_reads_a_paste_is_its_generations() {
        let arbiter = InputArbiter::new(EnterTiming::fixed(0));
        let first = PaneKey::new("declared", 1);
        arbiter
            .register_with(&first, Some(DRAWS))
            .expect("register pane");
        assert_eq!(arbiter.silent_paste(&first), Some(DRAWS));
        let newer = PaneKey::new("declared", 2);
        arbiter
            .register(&newer)
            .expect("register a newer generation");
        assert_eq!(arbiter.silent_paste(&newer), None);
    }

    #[cfg(unix)]
    #[test]
    fn a_blocked_write_does_not_stall_an_unrelated_pane() {
        let _pty_guard = serial_pty_test();
        let table = Arc::new(PaneTable::new());
        let OpenedPane {
            key: blocked_key,
            reader: mut blocked_reader,
        } = table
            .open(
                Path::new("/tmp"),
                &[
                    "/bin/sh".to_string(),
                    "-c".to_string(),
                    "stty raw -echo; printf ready; sleep 1000".to_string(),
                ],
                &HashMap::new(),
                terminal_size(),
            )
            .expect("open non-consuming pane");
        let mut ready = [0_u8; 5];
        blocked_reader
            .read_exact(&mut ready)
            .expect("blocked pane ready");
        assert_eq!(&ready, b"ready");
        let (responsive_key, reader) = raw_recorder(&table, 1);
        let arbiter = Arc::new(InputArbiter::new(EnterTiming::fixed(5)));
        arbiter
            .register(&blocked_key)
            .expect("register blocked pane");
        arbiter
            .register(&responsive_key)
            .expect("register responsive pane");

        let (started_sender, started_receiver) = mpsc::channel();
        let (blocked_sender, blocked_receiver) = mpsc::channel();
        let blocked_arbiter = Arc::clone(&arbiter);
        let blocked_table = Arc::clone(&table);
        let writer_key = blocked_key.clone();
        let blocked_writer = thread::spawn(move || {
            started_sender
                .send(())
                .expect("announce blocked human write");
            let result =
                blocked_arbiter.write(&blocked_table, &writer_key, &vec![b'x'; 8 * 1024 * 1024]);
            let _ = blocked_sender.send(result);
        });
        started_receiver
            .recv_timeout(Duration::from_secs(30))
            .expect("blocked human writer started");
        thread::sleep(Duration::from_millis(50));
        assert!(matches!(
            blocked_receiver.try_recv(),
            Err(mpsc::TryRecvError::Empty)
        ));

        let (responsive_sender, responsive_receiver) = mpsc::channel();
        let responsive_arbiter = Arc::clone(&arbiter);
        let responsive_table = Arc::clone(&table);
        let writer_key = responsive_key.clone();
        let responsive_writer = thread::spawn(move || {
            let _ = responsive_sender.send(responsive_arbiter.write(
                &responsive_table,
                &writer_key,
                b"R",
            ));
        });
        // A pane that waited on the blocked one would wait until the kill below, which is after
        // this wait: the deadline can be as long as a loaded runner needs to run the thread.
        let responsive_on_time = responsive_receiver.recv_timeout(Duration::from_secs(30));

        if responsive_on_time.is_err() {
            let cleanup_table = Arc::clone(&table);
            let cleanup_key = blocked_key.clone();
            let (cleanup_sender, cleanup_receiver) = mpsc::channel();
            thread::spawn(move || {
                let _ = cleanup_sender.send(cleanup_table.kill(&cleanup_key));
            });
            let _ = cleanup_receiver.recv_timeout(Duration::from_millis(500));
            drop(blocked_reader);
            panic!("an unrelated pane waited on the blocked pane's arbiter lock");
        }

        table.kill(&blocked_key).expect("kill blocked pane");
        let responsive_result = responsive_on_time
            .or_else(|_| responsive_receiver.recv_timeout(Duration::from_secs(30)))
            .expect("responsive human write eventually finishes");
        let _ = blocked_receiver
            .recv_timeout(Duration::from_secs(30))
            .expect("blocked human write unblocks after kill");
        blocked_writer.join().expect("blocked human writer thread");
        responsive_writer
            .join()
            .expect("responsive human writer thread");

        responsive_result.expect("responsive write");
        assert_eq!(read_hex(reader), "52");
        let _ = table.kill(&responsive_key);
    }

    /// What still refuses a delivery: a pane replaced since, or failed input
    /// (see below). Another paste cannot be going in: the pane's worker runs
    /// one at a time.
    #[test]
    fn claims_and_pastes_refuse_a_stale_pane() {
        let key = PaneKey::new("guarded", 1);
        let (writer, _observed) = RecordingWriter::new(None);
        let arbiter = InputArbiter::new(EnterTiming::fixed(0));
        arbiter.register(&key).expect("register pane");
        let stale = PaneKey::new(&key.id, key.generation + 1);
        assert!(matches!(arbiter.claim(&stale), Err(ArbiterError::Stale)));
        assert!(matches!(
            arbiter.write_paste_via(writer.as_ref(), &stale, b"late", None),
            Err(ArbiterError::Stale)
        ));
        assert!(writer.records().is_empty());
    }

    /// A newer generation cannot take a pane's place while its paste goes in:
    /// the paste finishes against the state it was admitted to.
    #[test]
    fn a_new_generation_cannot_cut_a_paste_short() {
        let old_key = PaneKey::new("worker", 1);
        let new_key = PaneKey::new("worker", 2);
        let arbiter = Arc::new(InputArbiter::new(EnterTiming::fixed(0)));
        arbiter.register(&old_key).expect("register old generation");

        let registration_checked = Arc::new(Barrier::new(2));
        let resume_registration = Arc::new(Barrier::new(2));
        let replacing_arbiter = Arc::clone(&arbiter);
        let replacing_key = new_key.clone();
        let register_checked = Arc::clone(&registration_checked);
        let register_resume = Arc::clone(&resume_registration);
        let replacement = thread::spawn(move || {
            replacing_arbiter.register_with_hook(&replacing_key, || {
                register_checked.wait();
                register_resume.wait();
            })
        });
        registration_checked.wait();

        let first_write = Arc::new(Barrier::new(2));
        let release_first_write = Arc::new(Barrier::new(2));
        let writer = Arc::new(GatedRecordingWriter {
            attempts: AtomicUsize::new(0),
            records: Mutex::new(Vec::new()),
            first_write: Arc::clone(&first_write),
            release_first_write: Arc::clone(&release_first_write),
        });
        let paste_arbiter = Arc::clone(&arbiter);
        let paste_writer = Arc::clone(&writer);
        let paste_key = old_key.clone();
        let paste = thread::spawn(move || {
            paste_arbiter.write_paste_via(paste_writer.as_ref(), &paste_key, b"body", None)
        });
        first_write.wait();
        assert!(
            arbiter
                .snapshot(&old_key)
                .expect("a paste is going in")
                .paste_in_flight
        );

        resume_registration.wait();
        let replacement_result = replacement.join().expect("replacement thread");
        release_first_write.wait();
        let paste_result = paste.join().expect("paste thread");

        assert!(matches!(replacement_result, Err(ArbiterError::Busy)));
        paste_result.expect("admitted paste finishes against its stable state");
        assert_eq!(
            *writer.records.lock().expect("gated records"),
            vec![b"\x1b[200~body\x1b[201~".to_vec(), b"\r".to_vec()]
        );
        let snapshot = arbiter
            .snapshot(&old_key)
            .expect("old generation remains active");
        assert!(!snapshot.paste_in_flight);
    }

    /// A paste that went in is answered as it went, whatever became of the
    /// pane's lock meanwhile: a lock error after the write read as a refusal,
    /// and a refused paste is one the daemon sends again.
    #[test]
    fn a_paste_that_went_in_is_answered_as_it_went() {
        let key = PaneKey::new("poisoned", 1);
        let first_write = Arc::new(Barrier::new(2));
        let release_first_write = Arc::new(Barrier::new(2));
        let writer = Arc::new(GatedRecordingWriter {
            attempts: AtomicUsize::new(0),
            records: Mutex::new(Vec::new()),
            first_write: Arc::clone(&first_write),
            release_first_write: Arc::clone(&release_first_write),
        });
        let arbiter = Arc::new(InputArbiter::new(EnterTiming::fixed(0)));
        arbiter.register(&key).expect("register pane");
        let cell = arbiter
            .lock_panes()
            .expect("pane map lock")
            .get(&key.id)
            .cloned()
            .expect("the pane's state");

        let paste_arbiter = Arc::clone(&arbiter);
        let paste_writer = Arc::clone(&writer);
        let paste_key = key.clone();
        let paste = thread::spawn(move || {
            paste_arbiter.write_paste_via(paste_writer.as_ref(), &paste_key, b"body", None)
        });
        first_write.wait();
        let poisoned = thread::spawn(move || {
            let _held = cell.lock();
            panic!("the pane's lock is poisoned while the paste goes in");
        })
        .join();
        assert!(poisoned.is_err());
        release_first_write.wait();

        let answered = paste.join().expect("paste thread");
        assert!(answered.is_ok(), "{answered:?}");
        assert_eq!(
            *writer.records.lock().expect("gated records"),
            vec![b"\x1b[200~body\x1b[201~".to_vec(), b"\r".to_vec()]
        );
    }

    #[test]
    fn generation_replacement_that_holds_the_cell_rejects_old_input_before_write() {
        let old_key = PaneKey::new("worker", 1);
        let new_key = PaneKey::new("worker", 2);
        let arbiter = Arc::new(InputArbiter::new(EnterTiming::fixed(0)));
        arbiter.register(&old_key).expect("register old generation");
        let cell = arbiter
            .lock_panes()
            .expect("pane map lock")
            .get(&old_key.id)
            .cloned()
            .expect("stable state cell");

        let replacement_locked = Arc::new(Barrier::new(2));
        let release_replacement = Arc::new(Barrier::new(2));
        let replacing_arbiter = Arc::clone(&arbiter);
        let replacing_key = new_key.clone();
        let locked = Arc::clone(&replacement_locked);
        let release = Arc::clone(&release_replacement);
        let replacement = thread::spawn(move || {
            replacing_arbiter.register_with_locked_hook(&replacing_key, || {
                locked.wait();
                release.wait();
            })
        });
        replacement_locked.wait();

        let (writer, _observed) = RecordingWriter::new(None);
        let paste_arbiter = Arc::clone(&arbiter);
        let paste_writer = Arc::clone(&writer);
        let paste_key = old_key.clone();
        let paste = thread::spawn(move || {
            paste_arbiter.write_paste_via(paste_writer.as_ref(), &paste_key, b"old", None)
        });
        let clone_deadline = Instant::now() + Duration::from_secs(30);
        while Arc::strong_count(&cell) < 4 && Instant::now() < clone_deadline {
            thread::yield_now();
        }
        assert!(
            Arc::strong_count(&cell) >= 4,
            "old paste did not reach the locked state cell"
        );

        release_replacement.wait();
        replacement
            .join()
            .expect("replacement thread")
            .expect("replacement wins cell lock");
        assert!(matches!(
            paste.join().expect("old paste thread"),
            Err(ArbiterError::Stale)
        ));
        assert!(writer.records().is_empty());
        assert_eq!(
            arbiter.snapshot(&new_key).expect("new generation state"),
            super::ArbiterSnapshot {
                generation: 2,
                paste_in_flight: false,
                input_failed: false,
                unsent: false,
                typed: 0,
                typed_quiet_ms: None,
                output_quiet_ms: None,
                screen_quiet_ms: None,
                paste_quiet_ms: None,
                human_writes: Vec::new(),
            }
        );
    }

    /// A write that failed may have reached the pane in part: the pane takes
    /// no more input until a new generation replaces it.
    #[test]
    fn a_failed_write_stops_the_pane_input_until_a_new_generation() {
        let key = PaneKey::new("recorded", 1);
        let (first_write_fails, _observed) = RecordingWriter::new(Some(1));
        let arbiter = InputArbiter::new(EnterTiming::fixed(0));
        arbiter.register(&key).expect("register pane");

        assert!(matches!(
            arbiter.write_paste_via(first_write_fails.as_ref(), &key, b"body", None),
            Err(ArbiterError::Pane(PaneError::Io(_)))
        ));
        assert!(
            !arbiter
                .snapshot(&key)
                .expect("state after paste write failure")
                .paste_in_flight
        );
        assert!(
            arbiter
                .snapshot(&key)
                .expect("state after paste write failure")
                .input_failed
        );
        let writes_after_failure = first_write_fails.records().len();
        assert!(matches!(
            arbiter.write_via(first_write_fails.as_ref(), &key, b"later"),
            Err(ArbiterError::InputFailed)
        ));
        assert!(matches!(
            arbiter.claim(&key),
            Err(ArbiterError::InputFailed)
        ));
        assert_eq!(first_write_fails.records().len(), writes_after_failure);
        let restarted_key = PaneKey::new(&key.id, key.generation + 1);
        arbiter
            .register(&restarted_key)
            .expect("restart input with a new generation");
        arbiter
            .write_via(first_write_fails.as_ref(), &restarted_key, b"R")
            .expect("new generation accepts input");
        assert!(
            !arbiter
                .snapshot(&restarted_key)
                .expect("restarted state")
                .input_failed
        );
    }

    /// A pane that leaves the table once `writes` writes have gone in.
    struct GoneAfter {
        writes: usize,
        attempts: AtomicUsize,
    }

    impl PaneInputWriter for GoneAfter {
        fn write(&self, pane: &PaneKey, _bytes: &[u8]) -> Result<(), PaneError> {
            if self.attempts.fetch_add(1, Ordering::SeqCst) < self.writes {
                Ok(())
            } else {
                Err(PaneError::NotFound(pane.clone()))
            }
        }
    }

    /// A write that finds its pane gone from the table (killed while the
    /// input waited its turn, its input not yet retired) wrote nothing: it is
    /// refused, as input sent after the retirement is, and the pane's input
    /// is not failed for it. It used to be uncertain, and fail the pane's
    /// input. An Enter that finds the pane gone after its paste went in is
    /// uncertain still.
    #[test]
    fn a_write_that_finds_its_pane_gone_is_refused_and_fails_nothing() {
        let key = PaneKey::new("p1-zeus", 1);
        let arbiter = InputArbiter::new(EnterTiming::fixed(0));
        arbiter.register(&key).expect("register pane");
        let without_the_pane = PaneTable::new();

        assert!(matches!(
            arbiter.write(&without_the_pane, &key, b"typed"),
            Err(ArbiterError::Stale)
        ));
        assert!(matches!(
            arbiter.write_paste(&without_the_pane, &key, b"body", None, false),
            Err(ArbiterError::Stale)
        ));
        let snapshot = arbiter.snapshot(&key).expect("the pane's input");
        assert!(
            !snapshot.input_failed && !snapshot.paste_in_flight,
            "{snapshot:?}"
        );

        let gone_before_the_enter = GoneAfter {
            writes: 1,
            attempts: AtomicUsize::new(0),
        };
        assert!(matches!(
            arbiter.write_paste_via(&gone_before_the_enter, &key, b"body", None),
            Err(ArbiterError::Pane(PaneError::NotFound(_)))
        ));
        assert!(
            arbiter
                .snapshot(&key)
                .expect("the pane's input")
                .input_failed
        );
    }

    #[test]
    fn sanitize_normalizes_crlf_and_rejects_other_controls() {
        assert_eq!(
            sanitize(b"first\r\nsecond\tthird\n").expect("sanitize text"),
            b"first\nsecond\tthird\n"
        );
        assert_eq!(
            sanitize("caf\u{e9}".as_bytes()).expect("allow UTF-8"),
            "caf\u{e9}".as_bytes()
        );

        for byte in (0_u8..=0x1f).filter(|byte| !matches!(byte, b'\t' | b'\n')) {
            assert_eq!(sanitize(&[byte]), Err(SanitizeError::ControlByte(byte)));
        }
        assert_eq!(sanitize(&[0x7f]), Err(SanitizeError::ControlByte(0x7f)));
        assert!(sanitize("\u{009b}".as_bytes()).is_err());
        assert!(sanitize(&[0x9b]).is_err());
        assert!(sanitize(&[0xff]).is_err());
    }

    #[cfg(windows)]
    mod windows {
        use std::collections::HashMap;
        use std::sync::{Arc, Mutex};
        use std::thread;
        use std::time::{Duration, Instant};

        use cf_proto::panes::SilentPaste;

        use super::super::{EnterTiming, InputArbiter};
        use crate::headless::ENTER;
        use crate::pty::conpty_test::{line_echo, open, powershell, read_until, take};
        use crate::pty::{serial_pty_test, PaneTable};

        /// How long a console case waits before it calls its child hung. A busy runner's
        /// PowerShell has taken more than 20 s to print its READY (`read_until` waits a minute
        /// for it), and one that reads a long paste key by key reads it more slowly still when the
        /// machine is loaded: zeewin2 under 32 busy processes, 1 run in 4, did not get through the
        /// slow reader's 4,000 keys in the 43 s the case allowed (T-1332). So nothing here is quick
        /// or slow by it: only a child that has not sent in this long, printing or not, runs out
        /// of it.
        const HANG: Duration = Duration::from_secs(180);

        /// How long the child may print nothing, having sent nothing, before the human presses
        /// Enter again, as the engine does for a paste its window did not send (10 s with the
        /// window unshown, 3 s of quiet: `arrival.rs`). It counts from the child's last print and
        /// not from the paste: a window that still reads prints as it goes, and a slow machine
        /// makes it slower and not quiet.
        const QUIET_BEFORE_AGAIN: Duration = Duration::from_secs(13);

        /// The quiet a console case's window must show before the human's Enter goes: two
        /// seconds, well above the gap in which a busy machine's PowerShell prints nothing while
        /// it still reads (T-1332: zeewin2 under 60 busy processes, 500 ms was not).
        const QUIET_MS: u64 = 2_000;

        /// The most a console case's window may be given to take a paste in: two minutes, past
        /// anything a loaded machine's PowerShell takes to read what these cases give it, so
        /// that the Enter goes because the window has read the paste and gone quiet, and not
        /// because the cap came while it read (zeewin2 under 60 busy processes: the slow reader's
        /// 4,000 keys took more than the 13 s the cap was). The rule that the host's own cap
        /// is too short for a window that reads a long paste is the plant's, in the case of the
        /// slow reader.
        const MOST_MS: u64 = 120_000;

        /// What a pane's child has printed, and when it last did.
        #[derive(Clone)]
        struct Printed {
            text: Arc<Mutex<String>>,
            last: Arc<Mutex<Instant>>,
        }

        impl Printed {
            fn new() -> Self {
                Self {
                    text: Arc::new(Mutex::new(String::new())),
                    last: Arc::new(Mutex::new(Instant::now())),
                }
            }

            fn push(&self, more: &str) {
                self.text.lock().expect("printed").push_str(more);
                *self.last.lock().expect("printed") = Instant::now();
            }

            fn text(&self) -> String {
                self.text.lock().expect("printed").clone()
            }

            /// Waits until the child has sent (what it printed says so, `]#`), and says how long
            /// after `from` that was. None where it printed nothing for `quiet` first, counted
            /// from the later of its last print and `from`: it read what it was given and sent
            /// nothing. A child that has not sent in [`HANG`] is hung, whatever it printed.
            fn sent_or_quiet(&self, from: Instant, quiet: Duration) -> Option<Duration> {
                loop {
                    if self.text().contains("]#") {
                        return Some(from.elapsed());
                    }
                    let silent = self
                        .last
                        .lock()
                        .expect("printed")
                        .elapsed()
                        .min(from.elapsed());
                    if silent >= quiet {
                        return None;
                    }
                    assert!(
                        from.elapsed() < HANG,
                        "the reader neither sent nor went quiet in {HANG:?}: {:?}",
                        self.text()
                    );
                    thread::sleep(Duration::from_millis(10));
                }
            }
        }

        /// A child that says READY and reads the console's key presses as Codex reads a paste
        /// on Windows (its `paste_burst`): an Enter within 120 ms of the key before it is a
        /// line break in the text, a later one sends it, and it says how many line breaks the
        /// text it sent held, as `SENT=[1]#`. It prints a dot for each key it reads, as a
        /// window draws what it is given.
        fn burst_reader() -> Vec<String> {
            powershell(
                "Write-Output 'READY'; $clock = [Diagnostics.Stopwatch]::StartNew(); \
                 $last = -1000; $breaks = 0; while ($true) { $k = [Console]::ReadKey($true); \
                 $now = $clock.ElapsedMilliseconds; if ($k.Key -eq 'Enter') { \
                 if ($now - $last -lt 120) { $breaks += 1; $last = $now } \
                 else { Write-Output ('SENT=[' + $breaks + ']#'); break } } \
                 else { $last = $now }; [Console]::Write('.') }",
            )
        }

        /// How many line breaks the burst reader sent with the human's `writes` (each after its
        /// pause), typed into a pane declared to read a paste silently, and whether that took an
        /// Enter more than theirs: where it sent nothing within ten seconds, the human presses
        /// Enter again.
        fn sent(writes: &[(Duration, Vec<u8>)]) -> (String, bool) {
            let _pty_guard = serial_pty_test();
            let table = Arc::new(PaneTable::new());
            let (key, output) = open(&table, &burst_reader(), &HashMap::new(), 1 << 20);
            // A reader that prints as it reads, declared to read a paste silently: a quiet
            // spell well above a busy machine's scheduling gap means it has read every key.
            let arbiter = InputArbiter::new(EnterTiming::fixed(0));
            let silent = SilentPaste {
                pace_us_per_character: 0,
                drawn_after_ms: 0,
                quiet_ms: QUIET_MS,
                most_ms: MOST_MS,
                most_us_per_character: 0,
            };
            let clock = arbiter
                .register_with(&key, Some(silent))
                .expect("register pane");
            read_until(&table, &key, &output, "READY");
            let seen = Printed::new();
            let reader = {
                let (table, key, seen) = (Arc::clone(&table), key.clone(), seen.clone());
                thread::spawn(move || {
                    while let Ok(chunk) = output.recv_timeout(Duration::from_secs(60)) {
                        clock.note(true);
                        let text = take(&table, &key, &chunk);
                        seen.push(&text);
                    }
                })
            };
            for (pause, write) in writes {
                thread::sleep(*pause);
                arbiter
                    .write_typed(&table, &key, write)
                    .expect("the human's keys");
            }
            // Where the child read what it was given and sent nothing, the human presses
            // Enter again.
            let again = seen
                .sent_or_quiet(Instant::now(), QUIET_BEFORE_AGAIN)
                .is_none();
            if again {
                arbiter
                    .write_typed(&table, &key, b"\r")
                    .expect("the human's Enter again");
                seen.sent_or_quiet(Instant::now(), QUIET_BEFORE_AGAIN);
            }
            table.kill(&key).expect("kill the pane");
            reader.join().expect("the pane's reader");
            let seen = seen.text();
            let sent = seen
                .split_once("SENT=")
                .and_then(|(_, rest)| rest.split_once('#'))
                .map(|(sent, _)| sent.to_owned())
                .unwrap_or_else(|| panic!("the reader sent nothing: {seen:?}"));
            (sent, again)
        }

        /// A child that says READY and reads the console's key presses slowly, as a window does
        /// that reads a long paste at a pace of its own (Muse on Windows, T-1121): each key takes
        /// `pace_us` microseconds to read, and it draws a dot for each, as a window redraws while
        /// it reads. An Enter it reads within 120 ms of the key before it is a line break in the
        /// text, as Codex reads it ([`burst_reader`]); one it reads later sends the text, and it
        /// says how many line breaks the text it sent held, as `SENT=[1]#`.
        fn slow_reader(pace_us: u64) -> Vec<String> {
            powershell(&format!(
                "Write-Output 'READY'; $clock = [Diagnostics.Stopwatch]::StartNew(); \
                 $pace = [long]([Diagnostics.Stopwatch]::Frequency * {pace_us} / 1000000); \
                 $last = -1000; $breaks = 0; while ($true) {{ $k = [Console]::ReadKey($true); \
                 $until = $clock.ElapsedTicks + $pace; while ($clock.ElapsedTicks -lt $until) {{}}; \
                 $now = $clock.ElapsedMilliseconds; if ($k.Key -eq 'Enter') {{ \
                 if ($now - $last -lt 120) {{ $breaks += 1; $last = $now }} \
                 else {{ Write-Output ('SENT=[' + $breaks + ']#'); break }} }} \
                 else {{ $last = $now }}; [Console]::Write('.') }}"
            ))
        }

        /// What came of a brief delivered to the slow reader.
        #[derive(Debug)]
        struct Delivered {
            /// How long after the brief was written its Enter was written.
            entered: Duration,
            /// How long after it the reader sent the text, if it did without another Enter.
            sent: Option<Duration>,
            /// What it sent held this many line breaks (1 where its Enter was one).
            breaks: String,
        }

        /// Delivers `brief` into a pane running the slow reader, as the daemon does
        /// (`write_paste`) with the host's own wait, the pane declared to read a paste as
        /// `silent` says. A paste its window did not send is pressed Enter on again after 13 s,
        /// as the engine does (10 s unshown, 3 s quiet: `arrival.rs`).
        fn delivered(brief: &[u8], silent: Option<SilentPaste>, pace_us: u64) -> Delivered {
            let _pty_guard = serial_pty_test();
            let table = Arc::new(PaneTable::new());
            let (key, output) = open(&table, &slow_reader(pace_us), &HashMap::new(), 1 << 20);
            let arbiter = InputArbiter::new(ENTER);
            let clock = arbiter.register_with(&key, silent).expect("register pane");
            read_until(&table, &key, &output, "READY");
            let seen = Printed::new();
            let reader = {
                let (table, key, seen) = (Arc::clone(&table), key.clone(), seen.clone());
                thread::spawn(move || {
                    while let Ok(chunk) = output.recv_timeout(Duration::from_secs(90)) {
                        clock.note(true);
                        let text = take(&table, &key, &chunk);
                        seen.push(&text);
                    }
                })
            };
            let pasted = Instant::now();
            arbiter
                .write_paste(&table, &key, brief, None, false)
                .expect("the delivery");
            let entered = pasted.elapsed();
            // A paste its window did not send, having read it, is pressed Enter on again, as the
            // engine does.
            let mut sent = seen.sent_or_quiet(pasted, QUIET_BEFORE_AGAIN);
            if sent.is_none() {
                arbiter.write(&table, &key, b"\r").expect("the Enter again");
                seen.sent_or_quiet(Instant::now(), QUIET_BEFORE_AGAIN);
            }
            table.kill(&key).expect("kill the pane");
            reader.join().expect("the pane's reader");
            let seen = seen.text();
            let breaks = seen
                .split_once("SENT=")
                .and_then(|(_, rest)| rest.split_once('#'))
                .map(|(breaks, _)| breaks.to_owned())
                .unwrap_or_else(|| panic!("the reader sent nothing: {seen:?}"));
            Delivered {
                entered,
                sent: sent.take(),
                breaks,
            }
        }

        /// How a window declares that it reads a paste at 0.5 ms a character and draws as it
        /// reads (Muse's measure on Windows, T-954), with the quiet spell and the cap of the burst
        /// reader's test: well above a busy machine's scheduling gap, and past the time its
        /// PowerShell takes to read 4,000 keys.
        const SLOW: SilentPaste = SilentPaste {
            pace_us_per_character: 500,
            drawn_after_ms: 0,
            quiet_ms: QUIET_MS,
            most_ms: MOST_MS,
            most_us_per_character: 0,
        };

        /// A brief of 4,000 characters delivered into a window that reads each of them in 0.7 ms
        /// and draws as it does: the host's wait alone sends its Enter at its most, 2 s, while the
        /// window still reads, and it is a line break in the text. The Enter of the declaration's
        /// wait comes once the window has read it and gone quiet, and sends it (T-1127).
        #[test]
        fn a_long_brief_delivered_to_a_window_that_reads_it_slowly_is_sent_by_its_enter() {
            let brief = vec![b'x'; 4_000];
            let delivered = delivered(&brief, Some(SLOW), 700);
            #[allow(clippy::print_stderr)] // Said in the output of the case, with --nocapture.
            {
                eprintln!("delivered: {delivered:?}");
            }
            assert_eq!(
                delivered.breaks, "[0]",
                "{delivered:?}: the Enter was a line break"
            );
            assert!(
                delivered.sent.is_some(),
                "{delivered:?}: the brief stayed in the window's box until the Enter was pressed again"
            );
            // Never sooner than the window could have read it at its declared pace: 4,000
            // characters at 0.5 ms each.
            assert!(
                delivered.entered >= Duration::from_secs(2),
                "{delivered:?}: the Enter went before the window could have read the brief"
            );
        }

        /// The human's paste and its Enter in one write reach a program that reads the
        /// console's key presses as a paste and then its Enter, which sends it (T-1025).
        #[test]
        fn a_humans_paste_and_its_enter_in_one_write_send_it_through_the_console() {
            let at_once = b"\x1b[200~pasted words\x1b[201~\r".to_vec();
            assert_eq!(
                sent(&[(Duration::ZERO, at_once)]),
                ("[0]".to_owned(), false)
            );
        }

        /// A long paste of the human's, its Enter pressed while the program still reads it,
        /// is sent by that Enter (T-1025).
        #[test]
        fn a_humans_enter_pressed_while_their_long_paste_is_read_sends_it_through_the_console() {
            let paste = [&b"\x1b[200~"[..], &[b'x'; 2_000], b"\x1b[201~"].concat();
            assert_eq!(
                sent(&[
                    (Duration::ZERO, paste),
                    (Duration::from_millis(100), b"\r".to_vec())
                ]),
                ("[0]".to_owned(), false)
            );
        }

        /// The page's terminal brackets a paste only where the program asked for it
        /// (`CSI ? 2004 h`, which Codex's terminal library sends), and the pane host knows a
        /// human's paste by its brackets: the console passes a program's ask on to the
        /// terminal. The program turns the console's handling of escape sequences on first,
        /// as such a library does.
        #[test]
        fn a_programs_ask_for_bracketed_pastes_reaches_the_pages_terminal_through_the_console() {
            let _pty_guard = serial_pty_test();
            let table = Arc::new(PaneTable::new());
            let asks = powershell(
                "$import = '[DllImport(' + [char]34 + 'kernel32.dll' + [char]34 + ')] public static extern'; \
                 $k = Add-Type -Name Mode -Namespace Cf -PassThru -MemberDefinition ( \
                 $import + ' IntPtr GetStdHandle(int n); ' + \
                 $import + ' bool GetConsoleMode(IntPtr h, out int m); ' + \
                 $import + ' bool SetConsoleMode(IntPtr h, int m);'); \
                 $h = $k::GetStdHandle(-11); $m = 0; [void]$k::GetConsoleMode($h, [ref]$m); \
                 [void]$k::SetConsoleMode($h, $m -bor 4); \
                 [Console]::Write([string][char]27 + '[?2004h'); Write-Output 'ASKED#'; \
                 Start-Sleep -Seconds 30",
            );
            let (key, output) = open(&table, &asks, &HashMap::new(), 1 << 20);
            let text = read_until(&table, &key, &output, "ASKED#");
            table.kill(&key).expect("kill the pane");
            assert!(text.contains("\x1b[?2004h"), "{text:?}");
        }

        /// A delivery is a bracketed paste and a delayed Enter; the harness
        /// must read the text alone, as one submitted line.
        #[test]
        fn a_paste_and_its_enter_reach_the_child_as_one_line() {
            let _pty_guard = serial_pty_test();
            let table = Arc::new(PaneTable::new());
            let (key, output) = open(&table, &line_echo(), &HashMap::new(), 1 << 20);
            let arbiter = InputArbiter::new(EnterTiming::fixed(5));
            arbiter.register(&key).expect("register pane");
            read_until(&table, &key, &output, "READY");

            arbiter
                .write_paste(&table, &key, b"pasted words", None, false)
                .expect("paste into the pane");
            let text = read_until(&table, &key, &output, "#");
            assert!(text.contains("GOT=[pasted words]"), "{text:?}");
            table.kill(&key).expect("kill the pane");
        }

        /// A human's typing reaches the child, Enter and all.
        #[test]
        fn human_typing_reaches_the_child() {
            let _pty_guard = serial_pty_test();
            let table = Arc::new(PaneTable::new());
            let (key, output) = open(&table, &line_echo(), &HashMap::new(), 1 << 20);
            let arbiter = InputArbiter::new(EnterTiming::fixed(5));
            arbiter.register(&key).expect("register pane");
            read_until(&table, &key, &output, "READY");

            arbiter
                .write(&table, &key, b"typed\r")
                .expect("type into the pane");
            let text = read_until(&table, &key, &output, "#");
            assert!(text.contains("GOT=[typed]"), "{text:?}");
            table.kill(&key).expect("kill the pane");
        }
    }
}

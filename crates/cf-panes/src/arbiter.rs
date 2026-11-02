use std::borrow::Cow;
use std::collections::{HashMap, VecDeque};
use std::fmt;
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
    /// How long the pane has printed nothing, once it has printed anything:
    /// a harness still drawing its screen is not ready for input.
    pub output_quiet_ms: Option<u64>,
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

/// When one generation of a pane last printed. Its output thread notes every
/// chunk here and takes no lock: output never waits on the pane's input.
pub struct OutputClock {
    started: Instant,
    /// Milliseconds after `started` of the last output, plus one; 0 before
    /// the pane has printed anything.
    printed: AtomicU64,
}

impl OutputClock {
    fn new() -> Self {
        Self {
            started: Instant::now(),
            printed: AtomicU64::new(0),
        }
    }

    /// The pane printed something now.
    pub(crate) fn note(&self) {
        let now = self.elapsed_ms();
        self.printed.store(now.saturating_add(1), Ordering::Release);
    }

    fn quiet_ms(&self) -> Option<u64> {
        let printed = self.printed.load(Ordering::Acquire);
        (printed != 0).then(|| self.elapsed_ms().saturating_sub(printed - 1))
    }

    fn elapsed_ms(&self) -> u64 {
        u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX)
    }
}

/// When a paste's Enter goes: once the window has taken the paste in, that
/// is, printed something after it (its echo), `drawn_after_ms` or more after it,
/// and then nothing for `quiet_ms`; never sooner than `least_ms`, and at
/// `most_ms` whatever it printed. A window silent since the paste may still be
/// reading it: Devin draws a long paste's placeholder only once it has read all
/// of it, and an Enter before that finds nothing to send. What a window prints
/// sooner than `drawn_after_ms` after the paste is its answer to the paste's
/// first key, not its draw of the paste (Codex on Windows, [`SilentPaste`]).
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
    fn wait(&self, output: &OutputClock, pasted: Instant) {
        loop {
            let since = u64::try_from(pasted.elapsed().as_millis()).unwrap_or(u64::MAX);
            // How long the window has been quiet, once it printed after the paste, and not
            // only at once (its answer to the paste's first key).
            let echoed = output
                .quiet_ms()
                .filter(|quiet| *quiet < since && since - quiet >= self.drawn_after_ms);
            if since >= self.most_ms
                || (since >= self.least_ms && echoed.is_some_and(|quiet| quiet >= self.quiet_ms))
            {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
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
    fn new(generation: u64) -> Self {
        Self {
            generation,
            paste_in_flight: false,
            input_failed: false,
            unsent: 0,
            typed: 0,
            typed_at: None,
            output: Arc::new(OutputClock::new()),
            pasted: None,
            silent_paste: None,
            human_pasted: None,
            paste: PasteWatch::default(),
            writes: VecDeque::new(),
        }
    }

    fn snapshot(&self) -> ArbiterSnapshot {
        ArbiterSnapshot {
            generation: self.generation,
            paste_in_flight: self.paste_in_flight,
            input_failed: self.input_failed,
            unsent: self.unsent > 0,
            typed: self.typed,
            typed_quiet_ms: self
                .typed_at
                .map(|at| u64::try_from(at.elapsed().as_millis()).unwrap_or(u64::MAX)),
            output_quiet_ms: self.output.quiet_ms(),
            paste_quiet_ms: self
                .pasted
                .map(|pasted| u64::try_from(pasted.elapsed().as_millis()).unwrap_or(u64::MAX)),
            human_writes: self
                .writes
                .iter()
                .map(|written| HumanWrite {
                    ago_ms: u64::try_from(written.at.elapsed().as_millis()).unwrap_or(u64::MAX),
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
}

impl InputArbiter {
    pub fn new(enter: EnterTiming) -> Self {
        Self {
            panes: Mutex::new(HashMap::new()),
            enter,
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
                    let fresh = PaneInputState::new(pane.generation);
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
        *state = PaneInputState::new(pane.generation);
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
        let snapshot = state.snapshot();
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
        // Where this write ends a paste of the human's, which may have begun in an earlier one.
        let (silent, ends) = {
            let state = self.pane_state(pane)?;
            let mut state = lock_state(&state)?;
            let silent = state.silent_paste;
            let current = state.generation == pane.generation;
            let ends = if silent.is_some() && current {
                state.paste.feed(bytes)
            } else {
                Vec::new()
            };
            (silent, ends)
        };
        if let Some(silent) = silent {
            for (part, ended) in parts_ending_pastes(bytes, &ends) {
                if !part.is_empty() {
                    self.settle_human_paste(pane)?;
                }
                self.write_via(writer, pane, part)?;
                if let Some(characters) = ended {
                    let timing = EnterTiming::of_silent_paste(silent, characters);
                    let state = self.pane_state(pane)?;
                    let mut state = lock_state(&state)?;
                    if state.generation == pane.generation {
                        state.human_pasted = Some((Instant::now(), timing));
                    }
                }
            }
        } else {
            self.write_via(writer, pane, bytes)?;
        }
        let state = self.pane_state(pane)?;
        let mut state = lock_state(&state)?;
        if state.generation == pane.generation {
            let read = keys_of(state.unsent, bytes);
            state.unsent = read.unsent;
            // What came, and when, and never a byte of it (`HumanWrite`).
            if state.writes.len() == HUMAN_WRITES {
                state.writes.pop_front();
            }
            state.writes.push_back(Written {
                at: Instant::now(),
                length: bytes.len(),
                classes: read.classes,
            });
            // A write that holds no key (what the terminal reports of itself) is no key
            // of the human's: the adapter's wait for their last key does not start again.
            if read.keyed {
                state.typed += 1;
                state.typed_at = Some(Instant::now());
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
            timing.wait(&output, at);
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
    /// since.
    pub(crate) fn write_paste(
        &self,
        table: &PaneTable,
        pane: &PaneKey,
        body: &[u8],
        read_empty: Option<u64>,
    ) -> Result<(), ArbiterError> {
        self.write_paste_via(table, pane, body, read_empty)
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

    /// A paste and its Enter. While it goes in, the pane's snapshot says so,
    /// and a newer generation of the pane cannot take its place.
    fn write_paste_via<W: PaneInputWriter + ?Sized>(
        &self,
        writer: &W,
        pane: &PaneKey,
        body: &[u8],
        read_empty: Option<u64>,
    ) -> Result<(), ArbiterError> {
        let body = sanitize(body)?;
        let state = self.pane_state(pane)?;
        let (output, enter) = {
            let mut state = lock_state(&state)?;
            validate_admission(&state, pane)?;
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
            enter.wait(&output, Instant::now());
        });
        // A paste is answered as it went: a lock poisoned meanwhile fails the
        // pane's next admission, never what this paste already wrote.
        if let Ok(mut state) = lock_state(&state) {
            state.paste_in_flight = false;
            if pasted {
                state.pasted = Some(Instant::now());
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
    keys_of(unsent, bytes).unsent
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
#[derive(Debug, Default)]
struct PasteWatch {
    /// The characters of the open paste so far; none while no paste is open.
    open: Option<usize>,
    /// The end of the last write, when it could be the beginning of a marker.
    carried: Vec<u8>,
}

/// A paste that ends in a write: where in it (just after the closing marker), and how many
/// characters the paste held, all of it, whatever writes it came in.
#[derive(Debug, PartialEq, Eq)]
struct PasteEnd {
    at: usize,
    characters: usize,
}

impl PasteWatch {
    /// The pastes that end in `bytes`, the human's next write, in order. A paste that began
    /// in an earlier write is counted from its opening marker; one still open at the end of
    /// `bytes` goes on in the next write; a marker split by the end of a write is found.
    fn feed(&mut self, bytes: &[u8]) -> Vec<PasteEnd> {
        let carried = self.carried.len();
        let stream = if carried == 0 {
            Cow::Borrowed(bytes)
        } else {
            Cow::Owned([&self.carried[..], bytes].concat())
        };
        self.carried.clear();
        let mut ends = Vec::new();
        let mut at = 0;
        loop {
            let rest = &stream[at..];
            match self.open {
                None => {
                    let Some(start) = position_of(rest, PASTE_START) else {
                        self.carry(rest, &[PASTE_START]);
                        return ends;
                    };
                    self.open = Some(0);
                    at += start + PASTE_START.len();
                }
                Some(characters) => {
                    let closing = position_of(rest, PASTE_END);
                    let opening = position_of(rest, PASTE_START);
                    if let Some(end) =
                        closing.filter(|end| opening.is_none_or(|start| *end < start))
                    {
                        at += end + PASTE_END.len();
                        ends.push(PasteEnd {
                            at: at.saturating_sub(carried),
                            characters: characters + characters_in(&rest[..end]),
                        });
                        self.open = None;
                    } else if let Some(start) = opening {
                        // An opening marker in a paste starts it again, as a terminal that was
                        // cut off and began another would.
                        self.open = Some(0);
                        at += start + PASTE_START.len();
                    } else {
                        let held = self.carry(rest, &[PASTE_END, PASTE_START]);
                        self.open = Some(characters + characters_in(&rest[..rest.len() - held]));
                        return ends;
                    }
                }
            }
        }
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
fn keys_of(mut unsent: usize, bytes: &[u8]) -> Keys {
    let characters = |text: &[u8]| text.iter().filter(|byte| *byte & 0xC0 != 0x80).count();
    let mut keyed = false;
    let mut classes = Classes::default();
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
            b'\r' | b'\n' => {
                classes.with("enter");
                unsent = 0;
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
            0x1b if rest.starts_with(PASTE_START) => {
                classes.with("paste");
                let text = &rest[PASTE_START.len()..];
                let end = text
                    .windows(PASTE_END.len())
                    .position(|window| window == PASTE_END)
                    .unwrap_or(text.len());
                unsent += characters(&text[..end]);
                at += PASTE_START.len() + (end + PASTE_END.len()).min(text.len());
                continue;
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
    Keys {
        unsent,
        keyed,
        classes,
    }
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
    use std::sync::{mpsc, Arc, Barrier, Mutex};
    use std::thread;
    use std::time::{Duration, Instant};

    #[cfg(unix)]
    use portable_pty::PtySize;

    use cf_proto::panes::SilentPaste;

    use super::{sanitize, ArbiterError, EnterTiming, InputArbiter, SanitizeError};
    #[cfg(unix)]
    use crate::pty::{serial_pty_test, OpenedPane};
    use crate::pty::{PaneError, PaneInputWriter, PaneKey, PaneTable};

    #[test]
    fn a_snapshot_says_how_long_the_pane_has_printed_nothing() {
        let arbiter = InputArbiter::new(EnterTiming::fixed(0));
        let pane = PaneKey::new("quiet-pane", 1);
        let printed = arbiter.register(&pane).expect("register");
        assert_eq!(
            arbiter.snapshot(&pane).expect("snapshot").output_quiet_ms,
            None
        );
        printed.note();
        thread::sleep(Duration::from_millis(20));
        let quiet = arbiter.snapshot(&pane).expect("snapshot").output_quiet_ms;
        assert!(
            quiet.is_some_and(|ms| (20..5_000).contains(&ms)),
            "{quiet:?}"
        );

        let newer = PaneKey::new("quiet-pane", 2);
        arbiter
            .register(&newer)
            .expect("register a newer generation");
        printed.note();
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

    struct RecordingWriter {
        attempts: AtomicUsize,
        fail_on_attempt: Option<usize>,
        records: Mutex<Vec<RecordedWrite>>,
        observed: mpsc::Sender<RecordedWrite>,
    }

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
            let (observed, receiver) = mpsc::channel();
            (
                Arc::new(Self {
                    attempts: AtomicUsize::new(0),
                    fail_on_attempt,
                    records: Mutex::new(Vec::new()),
                    observed,
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
                at: Instant::now(),
                bytes: bytes.to_vec(),
            };
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

    /// How long the human's keys have been quiet, which the daemon's pacing waits on
    /// before it types a message in (3.0.7 chief-meeting, CMT-03): counted from the
    /// human's last key only. The daemon's own keys, its pastes and their Enter leave
    /// it as it was; a key of the human's starts it again.
    #[test]
    fn the_human_s_keys_alone_start_the_quiet_the_pacing_waits_on() {
        let key = PaneKey::new("quiet", 1);
        let (writer, _observed) = RecordingWriter::new(None);
        let arbiter = InputArbiter::new(EnterTiming::fixed(0));
        arbiter.register(&key).expect("register pane");
        let quiet = || arbiter.snapshot(&key).expect("snapshot").typed_quiet_ms;
        assert_eq!(quiet(), None, "never typed in");

        arbiter
            .write_typed_via(writer.as_ref(), &key, b"hi\r")
            .expect("the human's keys");
        std::thread::sleep(std::time::Duration::from_millis(40));
        arbiter
            .write_via(writer.as_ref(), &key, b"\x03")
            .expect("the daemon's key");
        arbiter
            .write_paste_via(writer.as_ref(), &key, b"a message", None)
            .expect("the daemon's paste");
        assert!(
            quiet().is_some_and(|quiet| quiet >= 40),
            "the daemon's keys and paste are not the human's: {:?}",
            quiet()
        );
        arbiter
            .write_typed_via(writer.as_ref(), &key, b"x")
            .expect("the human's key");
        assert!(
            quiet().is_some_and(|quiet| quiet < 40),
            "a key of the human's starts it again: {:?}",
            quiet()
        );
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
    /// by T-1127's sibling check): the later chunks have no opening marker, and their line
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
        let (writer, _observed) = RecordingWriter::new(None);
        let arbiter = InputArbiter::new(EnterTiming::fixed(0));
        arbiter.register(&key).expect("register pane");
        arbiter
            .write_typed_via(writer.as_ref(), &key, b"x")
            .expect("a key");
        std::thread::sleep(Duration::from_millis(30));
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
        assert!(
            read.typed_quiet_ms.is_some_and(|quiet| quiet >= 30),
            "the time since the last key goes on: {:?}",
            read.typed_quiet_ms
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
        let (writer, _observed) = RecordingWriter::new(None);
        let arbiter = InputArbiter::new(EnterTiming::fixed(0));
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
        assert!(
            read.human_writes
                .windows(2)
                .all(|pair| pair[0].ago_ms >= pair[1].ago_ms),
            "oldest first: {:?}",
            read.human_writes
        );
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
        printed.note();
        let (read, snapshot) = mpsc::channel();
        let reading_arbiter = Arc::clone(&arbiter);
        let reading_key = key.clone();
        thread::spawn(move || {
            let quiet = reading_arbiter
                .snapshot(&reading_key)
                .map(|snapshot| snapshot.output_quiet_ms);
            let _ = read.send(quiet.is_ok_and(|quiet| quiet.is_some()));
        });
        let read_meanwhile = snapshot.recv_timeout(Duration::from_secs(1));
        release_first_write.wait();
        writing.join().expect("writing thread").expect("the write");
        assert_eq!(read_meanwhile, Ok(true), "the write held the pane's lock");
    }

    #[test]
    fn a_snapshot_says_how_long_since_a_paste_was_written_into_the_pane() {
        let key = PaneKey::new("pasted", 1);
        let (writer, _observed) = RecordingWriter::new(None);
        let arbiter = InputArbiter::new(EnterTiming::fixed(0));
        arbiter.register(&key).expect("register pane");
        let pasted = || arbiter.snapshot(&key).expect("snapshot").paste_quiet_ms;
        assert_eq!(pasted(), None, "nothing pasted yet");
        arbiter
            .write_paste_via(writer.as_ref(), &key, b"body", None)
            .expect("write paste");
        thread::sleep(Duration::from_millis(20));
        assert!(
            pasted().is_some_and(|ms| (20..5_000).contains(&ms)),
            "{:?}",
            pasted()
        );
        // The daemon's own keys and the human's are no paste.
        thread::sleep(Duration::from_millis(30));
        arbiter
            .write_via(writer.as_ref(), &key, b"\r")
            .expect("a key");
        assert!(pasted().is_some_and(|ms| ms >= 50), "{:?}", pasted());
    }

    #[test]
    fn paste_is_bracketed_then_enter_is_a_delayed_separate_write() {
        let key = PaneKey::new("recorded", 1);
        let (writer, _observed) = RecordingWriter::new(None);
        let arbiter = InputArbiter::new(EnterTiming::fixed(25));
        arbiter.register(&key).expect("register pane");

        arbiter
            .write_paste_via(writer.as_ref(), &key, b"body", None)
            .expect("write paste");

        let records = writer.records();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].bytes, b"\x1b[200~body\x1b[201~");
        assert_eq!(records[1].bytes, b"\r");
        assert!(records[1].at.duration_since(records[0].at) >= Duration::from_millis(25));
    }

    /// Pastes `body` through an arbiter with `enter`, the pane printing
    /// nothing for `silent` and then every 10 ms for `printing`, and says how
    /// long its Enter waited.
    fn enter_waited(enter: EnterTiming, silent: Duration, printing: Duration) -> Duration {
        let key = PaneKey::new("echoing", 1);
        let (writer, _observed) = RecordingWriter::new(None);
        let arbiter = InputArbiter::new(enter);
        let clock = arbiter.register(&key).expect("register pane");
        let printer = thread::spawn(move || {
            thread::sleep(silent);
            let started = Instant::now();
            while started.elapsed() < printing {
                clock.note();
                thread::sleep(Duration::from_millis(10));
            }
        });
        arbiter
            .write_paste_via(writer.as_ref(), &key, b"body", None)
            .expect("write paste");
        printer.join().expect("printer");
        let records = writer.records();
        records[1].at.duration_since(records[0].at)
    }

    #[test]
    fn a_pastes_enter_waits_for_the_window_to_finish_echoing_it() {
        // A quiet spell well above the printer's 10 ms: a busy machine's
        // scheduling gap is not the echo's end.
        let enter = EnterTiming {
            least_ms: 0,
            drawn_after_ms: 0,
            quiet_ms: 150,
            most_ms: 5_000,
        };
        let waited = enter_waited(enter, Duration::ZERO, Duration::from_millis(250));
        assert!(
            waited >= Duration::from_millis(250),
            "{waited:?}: before the echo ended"
        );
        assert!(
            waited < Duration::from_millis(2_000),
            "{waited:?}: long after it"
        );
    }

    #[test]
    fn a_pastes_enter_waits_for_an_echo_that_comes_late() {
        // Devin reading a long paste: silent, then its placeholder drawn.
        let enter = EnterTiming {
            least_ms: 0,
            drawn_after_ms: 0,
            quiet_ms: 60,
            most_ms: 5_000,
        };
        let waited = enter_waited(enter, Duration::from_millis(300), Duration::from_millis(50));
        assert!(
            waited >= Duration::from_millis(350),
            "{waited:?}: before the late echo"
        );
        assert!(
            waited < Duration::from_millis(2_500),
            "{waited:?}: long after it"
        );
    }

    #[test]
    fn a_paste_nothing_echoes_gets_its_enter_at_the_most() {
        let enter = EnterTiming {
            least_ms: 0,
            drawn_after_ms: 0,
            quiet_ms: 60,
            most_ms: 300,
        };
        let waited = enter_waited(enter, Duration::ZERO, Duration::ZERO);
        assert!(waited >= Duration::from_millis(300), "{waited:?}");
        assert!(waited < Duration::from_millis(2_500), "{waited:?}");
    }

    #[test]
    fn a_window_that_never_stops_printing_gets_its_enter_at_the_most() {
        // A quiet spell longer than the most: only the most lets Enter go,
        // however the printer is scheduled.
        let enter = EnterTiming {
            least_ms: 0,
            drawn_after_ms: 0,
            quiet_ms: 10_000,
            most_ms: 300,
        };
        let waited = enter_waited(enter, Duration::ZERO, Duration::from_millis(3_000));
        assert!(waited >= Duration::from_millis(300), "{waited:?}");
        assert!(waited < Duration::from_millis(2_500), "{waited:?}");
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

    /// A pane of `arbiter`'s, its harness reading a paste as `silent` says, that prints at each
    /// of `prints` (from now) and then every 10 ms for `printing`: the window reading a paste
    /// and then drawing it.
    fn drawing(
        arbiter: &InputArbiter,
        key: &PaneKey,
        silent: SilentPaste,
        prints: &[Duration],
        printing: Duration,
    ) -> thread::JoinHandle<()> {
        let clock = arbiter
            .register_with(key, Some(silent))
            .expect("register pane");
        let prints = prints.to_vec();
        thread::spawn(move || {
            let started = Instant::now();
            for at in prints {
                thread::sleep(at.saturating_sub(started.elapsed()));
                clock.note();
            }
            let drawn = Instant::now();
            while drawn.elapsed() < printing {
                clock.note();
                thread::sleep(Duration::from_millis(10));
            }
        })
    }

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
        let (writer, _observed) = RecordingWriter::new(None);
        let arbiter = InputArbiter::new(EnterTiming::fixed(0));
        let printer = drawing(
            &arbiter,
            &key,
            DRAWS,
            &[Duration::from_millis(200)],
            Duration::from_millis(50),
        );
        let at_once = [HUMAN_PASTE, b"\r"].concat();
        arbiter
            .write_typed_via(writer.as_ref(), &key, &at_once)
            .expect("the human's paste and Enter");
        printer.join().expect("printer");

        let records = writer.records();
        let written: Vec<&[u8]> = records.iter().map(|record| &record.bytes[..]).collect();
        assert_eq!(written, [HUMAN_PASTE, b"\r"]);
        let waited = between(&records);
        assert!(
            waited >= Duration::from_millis(300),
            "{waited:?}: before the window drew the paste and went quiet"
        );
        assert!(waited < Duration::from_millis(4_000), "{waited:?}");
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
        let (writer, _observed) = RecordingWriter::new(None);
        let arbiter = InputArbiter::new(EnterTiming::fixed(0));
        let printer = drawing(
            &arbiter,
            &key,
            DRAWS,
            &[Duration::from_millis(400)],
            Duration::from_millis(50),
        );
        arbiter
            .write_typed_via(writer.as_ref(), &key, HUMAN_PASTE)
            .expect("the human's paste");
        thread::sleep(Duration::from_millis(100));
        arbiter
            .write_typed_via(writer.as_ref(), &key, b"\r")
            .expect("the human's Enter");
        printer.join().expect("printer");

        let records = writer.records();
        assert_eq!(records.len(), 2);
        let waited = between(&records);
        assert!(
            waited >= Duration::from_millis(500),
            "{waited:?}: before the window drew the paste and went quiet"
        );
        assert!(waited < Duration::from_millis(4_000), "{waited:?}");
        assert_eq!(arbiter.snapshot(&key).expect("snapshot").typed, 2);
    }

    /// The wait is from when the paste was written, not from when the Enter came: an Enter
    /// pressed once the window has drawn the paste and gone quiet goes at once, and so does
    /// what the human types after it.
    #[test]
    fn a_humans_enter_after_the_window_took_their_paste_goes_at_once() {
        let key = PaneKey::new("pasted-and-drawn", 1);
        let (writer, observed) = RecordingWriter::new(None);
        let arbiter = InputArbiter::new(EnterTiming::fixed(0));
        let printer = drawing(&arbiter, &key, DRAWS, &[], Duration::from_millis(50));
        arbiter
            .write_typed_via(writer.as_ref(), &key, HUMAN_PASTE)
            .expect("the human's paste");
        printer.join().expect("printer");
        thread::sleep(Duration::from_millis(400));
        observed.try_iter().for_each(drop);

        for keys in [&b"\r"[..], b"next\r"] {
            let pressed = Instant::now();
            arbiter
                .write_typed_via(writer.as_ref(), &key, keys)
                .expect("the human's keys");
            let written = observed.try_recv().expect("the keys went").at;
            assert!(
                written.duration_since(pressed) < Duration::from_millis(1_000),
                "{:?}: {keys:?} waited for a paste the window had taken",
                written.duration_since(pressed)
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
        let (writer, _observed) = RecordingWriter::new(None);
        let arbiter = InputArbiter::new(EnterTiming::fixed(0));
        let silent = SilentPaste {
            drawn_after_ms: 50,
            ..DRAWS
        };
        let printer = drawing(
            &arbiter,
            &key,
            silent,
            &[Duration::from_millis(10), Duration::from_millis(600)],
            Duration::ZERO,
        );
        let at_once = [HUMAN_PASTE, b"\r"].concat();
        arbiter
            .write_typed_via(writer.as_ref(), &key, &at_once)
            .expect("the human's paste and Enter");
        printer.join().expect("printer");
        let waited = between(&writer.records());
        assert!(
            waited >= Duration::from_millis(650),
            "{waited:?}: the Enter went on the window's answer to the first key"
        );
        assert!(waited < Duration::from_millis(4_000), "{waited:?}");
    }

    /// Never sooner than the window could have read the paste at its pace: a print and a quiet
    /// spell while it reads (a timer of its own) do not let the Enter go.
    #[test]
    fn a_humans_enter_waits_at_least_as_long_as_the_window_takes_to_read_the_paste() {
        let key = PaneKey::new("read-at-its-pace", 1);
        let (writer, _observed) = RecordingWriter::new(None);
        let arbiter = InputArbiter::new(EnterTiming::fixed(0));
        let silent = SilentPaste {
            pace_us_per_character: 2_000,
            ..DRAWS
        };
        // 300 characters at 2 ms each: 600 ms.
        let paste = [&b"\x1b[200~"[..], &[b'x'; 300], b"\x1b[201~"].concat();
        let printer = drawing(
            &arbiter,
            &key,
            silent,
            &[Duration::from_millis(100)],
            Duration::ZERO,
        );
        let at_once = [&paste[..], b"\r"].concat();
        arbiter
            .write_typed_via(writer.as_ref(), &key, &at_once)
            .expect("the human's paste and Enter");
        printer.join().expect("printer");
        let waited = between(&writer.records());
        assert!(
            waited >= Duration::from_millis(600),
            "{waited:?}: before the window could have read the paste"
        );
        assert!(waited < Duration::from_millis(4_000), "{waited:?}");
    }

    /// A window that never draws the paste has the human's Enter at its cap for the paste's
    /// length: a second and a millisecond a character for Codex on Windows.
    #[test]
    fn a_window_that_never_draws_the_paste_has_the_enter_at_its_cap_for_the_pastes_length() {
        let key = PaneKey::new("never-drawn", 1);
        let (writer, _observed) = RecordingWriter::new(None);
        let arbiter = InputArbiter::new(EnterTiming::fixed(0));
        let silent = SilentPaste {
            most_ms: 100,
            most_us_per_character: 1_000,
            ..DRAWS
        };
        // A cap of 100 ms and 300 characters at 1 ms each: 400 ms.
        let paste = [&b"\x1b[200~"[..], &[b'x'; 300], b"\x1b[201~"].concat();
        arbiter
            .register_with(&key, Some(silent))
            .expect("register pane");
        let at_once = [&paste[..], b"\r"].concat();
        arbiter
            .write_typed_via(writer.as_ref(), &key, &at_once)
            .expect("the human's paste and Enter");
        let waited = between(&writer.records());
        assert!(waited >= Duration::from_millis(400), "{waited:?}");
        assert!(waited < Duration::from_millis(3_000), "{waited:?}");
    }

    /// What the pastes that end in each of `writes` (in order, to one watch) held.
    fn ended_in(writes: &[&[u8]]) -> Vec<Vec<super::PasteEnd>> {
        let mut watch = super::PasteWatch::default();
        writes.iter().map(|write| watch.feed(write)).collect()
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
        let (writer, _observed) = RecordingWriter::new(None);
        let arbiter = InputArbiter::new(EnterTiming::fixed(0));
        // 30,000 characters at 20 µs each: 600 ms, and the window drew them at once.
        let silent = SilentPaste {
            pace_us_per_character: 20,
            ..DRAWS
        };
        let printer = drawing(
            &arbiter,
            &key,
            silent,
            &[Duration::from_millis(20)],
            Duration::ZERO,
        );
        let first = [&b"\x1b[200~"[..], &[b'x'; 30_000]].concat();
        let last = b"tail of it\x1b[201~".to_vec();
        for chunk in [&first[..], &last[..], b"\r"] {
            arbiter
                .write_typed_via(writer.as_ref(), &key, chunk)
                .expect("a chunk of the human's input");
        }
        printer.join().expect("printer");

        let records = writer.records();
        let written: Vec<&[u8]> = records.iter().map(|record| &record.bytes[..]).collect();
        assert_eq!(written, [&first[..], &last[..], b"\r"]);
        let waited = records[2].at.duration_since(records[1].at);
        assert!(
            waited >= Duration::from_millis(550),
            "{waited:?}: held for the last chunk's ten characters, not the paste's thirty thousand"
        );
        assert!(waited < Duration::from_millis(4_000), "{waited:?}");
    }

    /// A chunk can end inside the closing marker: the paste is over all the same, and what
    /// follows it waits, in the write that completes the marker or the next.
    #[test]
    fn a_closing_marker_split_between_two_writes_still_holds_what_follows_the_paste() {
        for (second, then) in [(&b"1~"[..], Some(&b"\r"[..])), (b"1~\r", None)] {
            let key = PaneKey::new("marker-split", 1);
            let (writer, _observed) = RecordingWriter::new(None);
            let arbiter = InputArbiter::new(EnterTiming::fixed(0));
            let printer = drawing(
                &arbiter,
                &key,
                DRAWS,
                &[Duration::from_millis(300)],
                Duration::ZERO,
            );
            let first = b"\x1b[200~three\x1b[20";
            arbiter
                .write_typed_via(writer.as_ref(), &key, first)
                .expect("the paste, up to part of its closing marker");
            let completed = Instant::now();
            for write in [Some(second), then].into_iter().flatten() {
                arbiter
                    .write_typed_via(writer.as_ref(), &key, write)
                    .expect("the rest of the human's input");
            }
            printer.join().expect("printer");

            let written: Vec<Vec<u8>> = writer
                .records()
                .into_iter()
                .map(|record| record.bytes)
                .collect();
            assert_eq!(
                written,
                [first.to_vec(), b"1~".to_vec(), b"\r".to_vec()],
                "the write that completes the marker goes as two, the paste and what follows"
            );
            let enter = writer.records()[2].at.duration_since(completed);
            assert!(
                enter >= Duration::from_millis(350),
                "{enter:?}: the Enter went before the window drew the paste and went quiet"
            );
            assert!(enter < Duration::from_millis(4_000), "{enter:?}");
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
        let (writer, _observed) = RecordingWriter::new(None);
        let arbiter = InputArbiter::new(host);
        // 300 characters at 4 ms each: 1.2 s of reading, drawn all the while, then quiet.
        let silent = SilentPaste {
            pace_us_per_character: 4_000,
            most_ms: 5_000,
            ..DRAWS
        };
        let printer = drawing(&arbiter, &key, silent, &[], Duration::from_millis(1_300));
        let body = vec![b'x'; 300];
        arbiter
            .write_paste_via(writer.as_ref(), &key, &body, None)
            .expect("the delivery");
        printer.join().expect("printer");

        let records = writer.records();
        assert_eq!(records.len(), 2, "{records:?}");
        assert_eq!(records[1].bytes, b"\r");
        let waited = between(&records);
        assert!(
            waited >= Duration::from_millis(1_300),
            "{waited:?}: the Enter went while the window still read the paste"
        );
        assert!(waited < Duration::from_millis(4_000), "{waited:?}");
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
        let (writer, _observed) = RecordingWriter::new(None);
        let arbiter = InputArbiter::new(host);
        let clock = arbiter.register(&key).expect("register pane");
        let printer = thread::spawn(move || {
            let started = Instant::now();
            while started.elapsed() < Duration::from_millis(1_300) {
                clock.note();
                thread::sleep(Duration::from_millis(10));
            }
        });
        arbiter
            .write_paste_via(writer.as_ref(), &key, &vec![b'x'; 300], None)
            .expect("the delivery");
        printer.join().expect("printer");

        let waited = between(&writer.records());
        assert!(waited >= Duration::from_millis(300), "{waited:?}");
        assert!(
            waited < Duration::from_millis(1_200),
            "{waited:?}: waited on a declaration nobody made"
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
            .recv_timeout(Duration::from_secs(1))
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
        let responsive_on_time = responsive_receiver.recv_timeout(Duration::from_millis(500));

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
            .or_else(|_| responsive_receiver.recv_timeout(Duration::from_secs(2)))
            .expect("responsive human write eventually finishes");
        let _ = blocked_receiver
            .recv_timeout(Duration::from_secs(2))
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
        let clone_deadline = Instant::now() + Duration::from_secs(1);
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
            arbiter.write_paste(&without_the_pane, &key, b"body", None),
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
                quiet_ms: 500,
                most_ms: 20_000,
                most_us_per_character: 0,
            };
            let clock = arbiter
                .register_with(&key, Some(silent))
                .expect("register pane");
            read_until(&table, &key, &output, "READY");
            let seen = Arc::new(Mutex::new(String::new()));
            let reader = {
                let (table, key, seen) = (Arc::clone(&table), key.clone(), Arc::clone(&seen));
                thread::spawn(move || {
                    while let Ok(chunk) = output.recv_timeout(Duration::from_secs(60)) {
                        clock.note();
                        let text = take(&table, &key, &chunk);
                        seen.lock().expect("seen").push_str(&text);
                    }
                })
            };
            let has_sent = |within: Duration| {
                let deadline = Instant::now() + within;
                while Instant::now() < deadline {
                    if seen.lock().expect("seen").contains("]#") {
                        return true;
                    }
                    thread::sleep(Duration::from_millis(20));
                }
                false
            };
            for (pause, write) in writes {
                thread::sleep(*pause);
                arbiter
                    .write_typed(&table, &key, write)
                    .expect("the human's keys");
            }
            let again = !has_sent(Duration::from_secs(10));
            if again {
                arbiter
                    .write_typed(&table, &key, b"\r")
                    .expect("the human's Enter again");
                has_sent(Duration::from_secs(10));
            }
            table.kill(&key).expect("kill the pane");
            reader.join().expect("the pane's reader");
            let seen = seen.lock().expect("seen").clone();
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
            let seen = Arc::new(Mutex::new(String::new()));
            let reader = {
                let (table, key, seen) = (Arc::clone(&table), key.clone(), Arc::clone(&seen));
                thread::spawn(move || {
                    while let Ok(chunk) = output.recv_timeout(Duration::from_secs(90)) {
                        clock.note();
                        let text = take(&table, &key, &chunk);
                        seen.lock().expect("seen").push_str(&text);
                    }
                })
            };
            let sent_within = |from: Instant, within: Duration| {
                while from.elapsed() < within {
                    if seen.lock().expect("seen").contains("]#") {
                        return Some(from.elapsed());
                    }
                    thread::sleep(Duration::from_millis(10));
                }
                None
            };
            let pasted = Instant::now();
            arbiter
                .write_paste(&table, &key, brief, None)
                .expect("the delivery");
            let entered = pasted.elapsed();
            let mut sent = sent_within(pasted, Duration::from_secs(13));
            if sent.is_none() {
                arbiter.write(&table, &key, b"\r").expect("the Enter again");
                sent_within(Instant::now(), Duration::from_secs(30));
            }
            table.kill(&key).expect("kill the pane");
            reader.join().expect("the pane's reader");
            let seen = seen.lock().expect("seen").clone();
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
        /// reads (Muse's measure on Windows, T-954), with a quiet spell well above a busy
        /// machine's scheduling gap, as the burst reader's test has it, and a cap that gives a
        /// slow machine's PowerShell room.
        const SLOW: SilentPaste = SilentPaste {
            pace_us_per_character: 500,
            drawn_after_ms: 0,
            quiet_ms: 500,
            most_ms: 1_000,
            most_us_per_character: 3_000,
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
                delivered.sent.is_some_and(|sent| sent < Duration::from_secs(12)),
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
                .write_paste(&table, &key, b"pasted words", None)
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

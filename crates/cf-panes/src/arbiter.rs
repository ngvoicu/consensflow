use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

use cf_proto::panes::SilentPaste;

use crate::pty::{bracketed, Gate, Gated, PaneError, PaneInputWriter, PaneKey, PaneTable};
use crate::screen::Seen;

mod keys;
mod paste;
mod state;
#[cfg(test)]
mod tests;
mod timing;

use keys::keys_of;
#[cfg(test)]
use keys::unsent_after;
use paste::{parts_ending_pastes, Fed, Segment};
#[cfg(test)]
use paste::{PasteEnd, PasteWatch};
use state::{
    fail_input, lock_state, validate_admission, validate_generation, PaneInputState, PaneState,
    PaneStateMap, Written, HUMAN_WRITES,
};
pub(crate) use timing::Clock;
use timing::{millis_since, WallClock, PENDING_GRACE_MS};
pub use timing::{EnterTiming, OutputClock, OutputCount};
#[cfg(test)]
pub(crate) use timing::{AFTER_LOADS, BETWEEN_LOADS};

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

/// The look a paste, or an Enter, was licensed by: what the window's screen showed of its input
/// box (`pane.snapshot`'s `seen`) and the count of the human's keys then (`typed`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct At {
    pub seen: Seen,
    pub typed: u64,
}

/// A delivery's paste and what licensed it.
#[derive(Debug)]
pub struct Paste<'a> {
    pub body: &'a [u8],
    /// None: a daemon older than `seen` (refused for the human's unsent text alone).
    pub at: Option<At>,
    /// The count of the human's keys at which the look's reader read the box empty over their
    /// unsent text.
    pub read_empty: Option<u64>,
    /// The key that empties the box, written with the body.
    pub clear_first: Option<&'a [u8]>,
}

/// Why a look is no longer true.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Why {
    BoxChanged,
    OutputPending,
}

impl Why {
    pub fn cause(self) -> &'static str {
        match self {
            Self::BoxChanged => "box-changed",
            Self::OutputPending => "output-pending",
        }
    }
}

/// Why the arbiter did not take a pane's input. Every one but `Pane` is a
/// refusal before anything was written.
#[derive(Debug)]
pub enum ArbiterError {
    Stale,
    Busy,
    InputFailed,
    /// The human has typed in the window and not sent it yet.
    Unsent,
    /// The look that licensed this is no longer true of the window: nothing was written, and
    /// the caller looks again.
    LookAgain(Why),
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
            Self::LookAgain(why) => write!(formatter, "{}", why.cause()),
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
    /// Bytes the pty has given, and bytes in the screen: the output is pending while the first
    /// is the greater (`OutputCount`).
    pub output_read: u64,
    pub output_drawn: u64,
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

    /// [`InputArbiter::register_with`] for a pane whose output is counted by `counts`, which
    /// the thread that reads its pty advances (`stream_output`).
    pub fn register_counted(
        &self,
        pane: &PaneKey,
        silent_paste: Option<SilentPaste>,
        counts: &Arc<OutputCount>,
    ) -> Result<Arc<OutputClock>, ArbiterError> {
        self.register_with_counts(pane, silent_paste, Arc::clone(counts))
    }

    /// Takes a pane in as [`InputArbiter::register`] does, with how its harness reads a
    /// human's paste where it reads one silently (`pane.open`'s `silentPaste`).
    pub fn register_with(
        &self,
        pane: &PaneKey,
        silent_paste: Option<SilentPaste>,
    ) -> Result<Arc<OutputClock>, ArbiterError> {
        self.register_with_counts(pane, silent_paste, Arc::new(OutputCount::default()))
    }

    fn register_with_counts(
        &self,
        pane: &PaneKey,
        silent_paste: Option<SilentPaste>,
        counts: Arc<OutputCount>,
    ) -> Result<Arc<OutputClock>, ArbiterError> {
        let output = self.register_inner(pane, counts, || {}, || {})?;
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
        self.register_inner(
            pane,
            Arc::new(OutputCount::default()),
            before_replace,
            || {},
        )
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
        self.register_inner(pane, Arc::new(OutputCount::default()), || {}, before_commit)
    }

    fn register_inner<F, G>(
        &self,
        pane: &PaneKey,
        counts: Arc<OutputCount>,
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
                    let fresh = PaneInputState::new(pane.generation, &self.clock, counts);
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
        *state = PaneInputState::new(pane.generation, &self.clock, counts);
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

    pub(crate) fn write_typed_via<W: PaneInputWriter + ?Sized>(
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

    /// A paste and its Enter ([`Paste`]): admitted by the gate of [`InputArbiter::paste_gate`]
    /// with the pane's writer, the screen's lock and the pane's input state held, and its
    /// Enter by the first Enter's gate.
    pub(crate) fn write_paste(
        &self,
        table: &PaneTable,
        pane: &PaneKey,
        paste: Paste<'_>,
    ) -> Result<(), ArbiterError> {
        self.write_paste_via(table, pane, paste)
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

    /// A paste and its Enter. While it goes in, the pane's snapshot says so, and a newer
    /// generation of the pane cannot take its place.
    pub(crate) fn write_paste_via<W: PaneInputWriter + ?Sized>(
        &self,
        writer: &W,
        pane: &PaneKey,
        paste: Paste<'_>,
    ) -> Result<(), ArbiterError> {
        let body = sanitize(paste.body)?;
        let state = self.pane_state(pane)?;
        let mut bytes = paste.clear_first.map(<[u8]>::to_vec).unwrap_or_default();
        bytes.extend_from_slice(&bracketed(&body));
        // What the gate takes from the pane's state when it admits.
        let mut admitted = None;
        let first = writer.write_gated(pane, &bytes, &mut || {
            let mut gate = Gate::Wait;
            writer.with_seen(pane, &mut |seen| {
                gate = self.paste_gate(&state, pane, &paste, seen, &body, &mut admitted);
            });
            gate
        });
        let Some((output, enter)) = admitted else {
            return Err(match first {
                Err(Gated::Refused(refused)) => refused,
                Err(Gated::Pane(error)) => fail_input(&state, pane, error, false),
                Ok(()) => ArbiterError::Stale,
            });
        };
        if let Err(error) = first {
            self.paste_ended(&state, false);
            return Err(match error {
                Gated::Refused(refused) => refused,
                Gated::Pane(error) => fail_input(&state, pane, error, false),
            });
        }
        // The Enter's wait begins once the paste itself is written. It goes once the window's
        // output is drawn, and is looked at again with the pane's writer held, where it is
        // written: a read that came while it waited for the writer holds it back too, up to the
        // same cap.
        let at = self.clock.now();
        enter.wait(self.clock.as_ref(), &output, at);
        let written = writer.write_gated(pane, b"\r", &mut || {
            let overdue = millis_since(self.clock.now(), at) >= enter.pending_cap_ms();
            if overdue || !output.counts().pending() {
                Gate::Go
            } else {
                Gate::Wait
            }
        });
        // A paste is answered as it went: a lock poisoned meanwhile fails the
        // pane's next admission, never what this paste already wrote.
        self.paste_ended(&state, true);
        written.map_err(|error| match error {
            Gated::Refused(refused) => refused,
            Gated::Pane(error) => fail_input(&state, pane, error, true),
        })
    }

    /// A paste with no look, only what the human's keys had left unsent: how a daemon older than
    /// `seen` sends one (a test of the host's older rule and of what is not the look's).
    #[cfg(test)]
    pub(crate) fn write_paste_plain<W: PaneInputWriter + ?Sized>(
        &self,
        writer: &W,
        pane: &PaneKey,
        body: &[u8],
        read_empty: Option<u64>,
    ) -> Result<(), ArbiterError> {
        self.write_paste_via(
            writer,
            pane,
            Paste {
                body,
                at: None,
                read_empty,
                clear_first: None,
            },
        )
    }

    fn paste_ended(&self, state: &PaneState, pasted: bool) {
        if let Ok(mut state) = lock_state(state) {
            state.paste_in_flight = false;
            if pasted {
                state.pasted = Some(self.clock.now());
            }
        }
    }

    /// Whether a paste goes in, asked with the pane's writer, the screen's lock (`seen` is of
    /// the screen under it) and, taken here, the pane's input state held: all of this holds at
    /// that one moment or nothing is written. With the look it was licensed by (`at`): the
    /// pane current and its input working; nothing pending (output read and not yet drawn);
    /// what the screen shows of the box the look's; no key of the human's since; their unsent
    /// text none, or the look's reader read the box empty over it. Without a look (a daemon that
    /// makes none): refused for the human's unsent text alone, as it was.
    fn paste_gate(
        &self,
        state: &PaneState,
        pane: &PaneKey,
        paste: &Paste<'_>,
        seen: Option<Seen>,
        body: &[u8],
        admitted: &mut Option<(Arc<OutputClock>, EnterTiming)>,
    ) -> Gate {
        let mut state = match lock_state(state) {
            Ok(state) => state,
            Err(error) => return Gate::Refuse(error),
        };
        if let Err(error) = validate_admission(&state, pane) {
            return Gate::Refuse(error);
        }
        match &paste.at {
            Some(at) => {
                if state.output.counts().pending() {
                    return Gate::Refuse(ArbiterError::LookAgain(Why::OutputPending));
                }
                if seen != Some(at.seen) {
                    return Gate::Refuse(ArbiterError::LookAgain(Why::BoxChanged));
                }
                if state.typed != at.typed {
                    return Gate::Refuse(ArbiterError::Unsent);
                }
                if state.unsent > 0 && paste.read_empty != Some(at.typed) {
                    return Gate::Refuse(ArbiterError::Unsent);
                }
            }
            None => {
                if state.unsent > 0 && paste.read_empty != Some(state.typed) {
                    return Gate::Refuse(ArbiterError::Unsent);
                }
            }
        }
        state.unsent = 0;
        state.paste_in_flight = true;
        // The host's wait, or the longer of it and how the window's harness reads a paste of
        // this length where it declared that.
        *admitted = Some((
            Arc::clone(&state.output),
            self.enter.for_delivery(state.silent_paste, body),
        ));
        Gate::Go
    }

    /// An Enter of a delivery that is not a paste's first: the one pressed again for a paste the
    /// window has not sent, the one for a turn that never began, Devin's for its `/bypass`. It
    /// goes only on the look that licensed it (`at`), checked again where it is written, with the
    /// pane's writer, the screen's lock and the pane's input state held: the pane current; no
    /// unsent text of the human's and no key of theirs since; the box the look's; nothing pending.
    /// Output pending makes it wait, for [`PENDING_GRACE_MS`] at the most and then it is
    /// refused: it is never written while output is pending.
    pub(crate) fn enter<W: PaneInputWriter + ?Sized>(
        &self,
        writer: &W,
        pane: &PaneKey,
        at: At,
    ) -> Result<(), ArbiterError> {
        let state = self.pane_state(pane)?;
        let started = self.clock.now();
        writer
            .write_gated(pane, b"\r", &mut || {
                let mut gate = Gate::Wait;
                writer.with_seen(pane, &mut |seen| {
                    gate = self.enter_gate(&state, pane, &at, seen, started);
                });
                gate
            })
            .map_err(|error| match error {
                Gated::Refused(refused) => refused,
                Gated::Pane(error) => fail_input(&state, pane, error, false),
            })
    }

    fn enter_gate(
        &self,
        state: &PaneState,
        pane: &PaneKey,
        at: &At,
        seen: Option<Seen>,
        started: Instant,
    ) -> Gate {
        let state = match lock_state(state) {
            Ok(state) => state,
            Err(error) => return Gate::Refuse(error),
        };
        if let Err(error) = validate_admission(&state, pane) {
            return Gate::Refuse(error);
        }
        if state.unsent > 0 || state.typed != at.typed {
            return Gate::Refuse(ArbiterError::Unsent);
        }
        if seen != Some(at.seen) {
            return Gate::Refuse(ArbiterError::LookAgain(Why::BoxChanged));
        }
        if !state.output.counts().pending() {
            return Gate::Go;
        }
        if millis_since(self.clock.now(), started) >= PENDING_GRACE_MS {
            Gate::Refuse(ArbiterError::LookAgain(Why::OutputPending))
        } else {
            Gate::Wait
        }
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

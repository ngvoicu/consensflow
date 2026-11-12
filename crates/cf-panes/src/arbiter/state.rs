use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

use cf_proto::panes::SilentPaste;

use crate::pty::{PaneError, PaneKey};

use super::keys::Classes;
use super::paste::PasteWatch;
use super::timing::{millis_since, Clock, EnterTiming, OutputClock};
use super::{ArbiterError, ArbiterSnapshot, HumanWrite};

/// How many of the human's last writes a pane keeps.
pub(super) const HUMAN_WRITES: usize = 8;

/// What a pane keeps of one of the human's writes: when, how many bytes, of what kinds.
#[derive(Debug, Clone, Copy)]
pub(super) struct Written {
    pub(super) at: Instant,
    pub(super) length: usize,
    pub(super) classes: Classes,
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
pub(super) struct PaneInputState {
    pub(super) generation: u64,
    pub(super) paste_in_flight: bool,
    pub(super) input_failed: bool,
    pub(super) unsent: usize,
    pub(super) typed: u64,
    pub(super) typed_at: Option<Instant>,
    pub(super) output: Arc<OutputClock>,
    /// When a paste was last written into this generation of the pane.
    pub(super) pasted: Option<Instant>,
    /// How the window's harness reads a human's paste, where it reads one silently: its
    /// adapter's word, given as the pane opened ([`InputArbiter::register_with`]).
    pub(super) silent_paste: Option<SilentPaste>,
    /// When the human's last paste was written, and when their keys after it may go, while
    /// those keys have not gone ([`InputArbiter::write_typed`]).
    pub(super) human_pasted: Option<(Instant, EnterTiming)>,
    /// Where the human's input stands in a paste, across the writes it comes in
    /// ([`InputArbiter::write_typed`]), in a window that reads a paste silently.
    pub(super) paste: PasteWatch,
    /// The human's last writes into this generation, oldest first ([`HUMAN_WRITES`]).
    pub(super) writes: VecDeque<Written>,
}

pub(super) type PaneState = Arc<Mutex<PaneInputState>>;
pub(super) type PaneStateMap = HashMap<String, PaneState>;

impl PaneInputState {
    pub(super) fn new(generation: u64, clock: &Arc<dyn Clock>) -> Self {
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
    pub(super) fn snapshot(&self, now: Instant) -> ArbiterSnapshot {
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

pub(super) fn lock_state(
    state: &Mutex<PaneInputState>,
) -> Result<MutexGuard<'_, PaneInputState>, ArbiterError> {
    state.lock().map_err(|_| ArbiterError::LockPoisoned)
}

pub(super) fn validate_generation(
    state: &PaneInputState,
    pane: &PaneKey,
) -> Result<(), ArbiterError> {
    if state.generation == pane.generation {
        Ok(())
    } else {
        Err(ArbiterError::Stale)
    }
}

pub(super) fn validate_admission(
    state: &PaneInputState,
    pane: &PaneKey,
) -> Result<(), ArbiterError> {
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
pub(super) fn fail_input(
    state: &PaneState,
    pane: &PaneKey,
    error: PaneError,
    begun: bool,
) -> ArbiterError {
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

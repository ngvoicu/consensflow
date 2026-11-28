use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

use cf_proto::panes::{Evidence, ReadEmpty, SilentPaste};

use crate::pty::{PaneError, PaneKey};

use super::keys::Classes;
use super::paste::PasteWatch;
use super::timing::{millis_since, Clock, EnterTiming, OutputClock, OutputCount};
use super::{ArbiterError, ArbiterSnapshot, HumanWrite, Kept, UnsentAt, TYPED_AT_MOST};

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
/// read the box empty goes in over the count where the host holds the evidence
/// of where those keys went ([`UnsentAt`], N17), and the count is cleared
/// (T-288). The lock is never held across a write.
pub(super) struct PaneInputState {
    pub(super) generation: u64,
    pub(super) paste_in_flight: bool,
    pub(super) input_failed: bool,
    pub(super) unsent: usize,
    pub(super) typed: u64,
    pub(super) typed_at: Option<Instant>,
    /// The screens kept at the person's keys while the count is above zero.
    pub(super) unsent_at: UnsentAt,
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
    pub(super) fn new(generation: u64, clock: &Arc<dyn Clock>, counts: Arc<OutputCount>) -> Self {
        Self {
            generation,
            paste_in_flight: false,
            input_failed: false,
            unsent: 0,
            typed: 0,
            typed_at: None,
            unsent_at: UnsentAt::default(),
            output: Arc::new(OutputClock::new(Arc::clone(clock), counts)),
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
        // One question to the terminal with one sampling of the pair, under the gate
        // ([`OutputCount::sample`]): two samplings, one for each number, could tell 10/10 of a
        // stream going 10/9, 11/9, 11/10.
        let (output_read, output_drawn, output_caught_up) = self.output.counts().sample();
        ArbiterSnapshot {
            generation: self.generation,
            paste_in_flight: self.paste_in_flight,
            input_failed: self.input_failed,
            unsent: self.unsent > 0,
            typed: self.typed,
            typed_quiet_ms: self.typed_at.map(|at| millis_since(now, at)),
            output_quiet_ms: self.output.quiet_ms(now),
            screen_quiet_ms: self.output.screen_quiet_ms(now),
            output_read,
            output_drawn,
            output_caught_up,
            unsent_at: (self.unsent > 0).then(|| self.unsent_at.clone()),
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

impl PaneInputState {
    /// A write of the person's that holds a key: `after` what it leaves unsent, `added`
    /// whether it added a character, `kept` the screen as it stood before it: the evidence of
    /// where the keys the count holds went (the design's table).
    pub(super) fn keep_screens(&mut self, after: usize, added: bool, kept: Option<Arc<Kept>>) {
        let at = &mut self.unsent_at;
        if after == 0 {
            *at = UnsentAt::default();
        } else if added {
            at.pressed_over = None;
            let room = at.typed_at.len() < TYPED_AT_MOST;
            match (kept, at.typed_at.last_mut()) {
                // The same screen twice is kept once, with what the newer one says of itself.
                (Some(kept), Some(last)) if last.drawn == kept.drawn => *last = kept,
                (Some(kept), _) if room => at.typed_at.push(kept),
                _ => at.typed_more = true,
            }
        } else if at.pressed_over.is_none() {
            at.pressed_over = kept;
        }
    }

    /// Whether the paste that says `read_empty` may go over the count: the host holds the
    /// screens it names. For [`Evidence::Erased`] the screen over the key was current; for
    /// [`Evidence::Elsewhere`] every character has its screen, and the last of them or the
    /// screen over the next key was current. The host cannot check the reader's reading of
    /// them.
    pub(super) fn admits_over_count(&self, read_empty: Option<ReadEmpty>) -> bool {
        let Some(ReadEmpty { typed, by }) = read_empty else {
            return false;
        };
        let at = &self.unsent_at;
        let over = at.pressed_over.as_ref().is_some_and(|over| over.current);
        typed == self.typed
            && match by {
                Evidence::Erased => over,
                Evidence::Elsewhere => {
                    !at.typed_more && at.typed_at.last().is_some_and(|last| last.current || over)
                }
            }
    }
}

pub(super) fn lock_state(
    state: &Mutex<PaneInputState>,
) -> Result<MutexGuard<'_, PaneInputState>, ArbiterError> {
    crate::sync::lock(state).map_err(|_| ArbiterError::LockPoisoned)
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
    // A feed that panicked leaves a screen only partly written: nothing is admitted against it.
    if state.input_failed || state.output.counts().is_broken() {
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

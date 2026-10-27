//! Paced delivery (3.0.7 chief-meeting, CMT-03 to CMT-06): a message is typed into a
//! window only once its turn has ended, read from the harness's own word, and has
//! stayed ended: the owner's word of 2026-10-26 was to wait for the chief to do the
//! work before the next message goes. Each look at a window keeps its rest
//! ([`Dispatcher::note_rest`]): begun at the first look that reads it at rest from a
//! record that was read, and begun again by any look that reads otherwise, a record
//! that could not be read, a delivery on its way, or a new last item of the record. A
//! message goes ([`Dispatcher::paced`]) once that rest has held for the settle
//! ([`Limits::settle_ms`](crate::seams::Limits::settle_ms)) and the pane host says no
//! key of the human's came in that time; a fresh look just before the paste
//! ([`Dispatcher::still_at_rest`]) must read the window at rest on the same last item.
//! Otherwise nothing is typed, no delivery begins and no attempt is spent.
//!
//! - **Unknown is not rest.** A record that could not be read says nothing of the
//!   window, so nothing goes in on it, with a settle or without: an adapter's own
//!   reading may call such a look settled (`record_state`, Claude's `empty`), for its
//!   lane and its turn's end, and the pacing reads the record's kind itself.
//! - **The fallback.** A harness with no word of its own that its window is at rest
//!   (`Window::idle_word`) is read at rest only by its record's turn ended, its screen
//!   quiet and no key of the human's, each for the settle; its lane says so.
//! - **What goes as before**, at the first look at rest: a launch's prelude (the role it
//!   says first, and what that displaced: a handoff, a brief, the note that a turn was
//!   cut) and an urgent row (the chief's `cf tell`). A launch's first message is handed
//!   over by `hand_first`, which this does not gate.
//! - **No settle** (0, the kit's) is no pacing: the gate asks the pane host nothing and
//!   looks no second time, so every case keeps the calls it was written with; only the
//!   rule that unknown is not rest holds.
//!
//! Limits. The settle shrinks the time in which a turn the window begins by itself (a
//! woken agent, a human's Enter the record shows late) can meet a paste; it cannot end
//! it: a paste can still land as such a turn begins, and the arrival's rules settle it
//! (`arrival`). The human's keys are the pane host's count, written by the app's own
//! terminal: keys typed into the window some other way are not seen.

use std::rc::Rc;
use std::sync::Arc;

use cf_base::js;
use cf_harness::contract::{InputBox, Observed};
use cf_harness::records::Reading;
use cf_proto::ledger::MessageView;
use serde_json::{json, Value};

use crate::dispatcher::Dispatcher;
use crate::record::Record;
use crate::runtime::returning;
use crate::seams::EngineError;

/// What the lane of a window read at rest by the fallback says.
pub(crate) const FALLBACK: &str =
    "at rest by its record and a quiet screen: its harness says nothing of its own";

/// What a look read of a window at rest, which every look of its rest reads the same.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Frontier {
    /// The last item of its record: its id, and whether it was whole.
    last: Option<(Arc<str>, bool)>,
    /// How many items the record held.
    items: usize,
    /// The harness's own word that the window waits at its prompt.
    at_prompt: bool,
}

impl Frontier {
    fn of(observed: &Observed) -> Self {
        let items = observed.items();
        Self {
            last: items.last().map(|item| (Arc::clone(&item.id), item.complete)),
            items: items.len(),
            at_prompt: observed.at_prompt,
        }
    }
}

/// The rest a window's looks have read since `since`, on one frontier, the last of
/// them the step's look `look`.
#[derive(Debug, Clone)]
pub(crate) struct Rest {
    since: i64,
    frontier: Frontier,
    look: u64,
}

/// What the look that let a message go read, for the fresh look before its paste to
/// read the same and for its `delivery.begun` to say (CMT-08).
#[derive(Debug, Clone)]
pub(crate) struct Paced {
    message: i64,
    frontier: Frontier,
    rest_ms: i64,
    typed_quiet_ms: Option<i64>,
    output_quiet_ms: Option<i64>,
    fallback: bool,
}

impl Paced {
    /// The evidence, as `delivery.begun` says it.
    pub(crate) fn look(&self) -> Value {
        json!({
            "status": if self.fallback { "idle by the fallback" } else { "idle" },
            "atPrompt": self.frontier.at_prompt,
            "lastItem": self.frontier.last.as_ref().map(|(id, _)| &**id),
            "items": self.frontier.items,
            "restMs": self.rest_ms,
            "typedQuietMs": self.typed_quiet_ms,
            "outputQuietMs": self.output_quiet_ms,
        })
    }
}

/// Whether a look read the window's record: one not there was read, and holds
/// nothing; one that could not be read says nothing.
fn read(observed: &Observed) -> bool {
    observed
        .reading
        .as_deref()
        .is_some_and(|reading| !Reading::read_failed(reading))
}

/// A number of milliseconds the pane host said, none where it said none.
fn ms(snapshot: &Value, field: &str) -> Option<i64> {
    snapshot
        .get(field)
        .and_then(Value::as_f64)
        .map(|ms| ms as i64)
}

impl Dispatcher {
    /// The step's look at a window is the next: its number, for [`Dispatcher::note_rest`].
    pub(crate) fn looked(&self, record: &Record) -> u64 {
        let mut part = record.delivery.borrow_mut();
        part.looks += 1;
        part.looks
    }

    /// Whether the window's harness has a word of its own that it is at rest
    /// (`Window::idle_word`); a closed window has no harness to ask, and is taken as
    /// one that has.
    pub(crate) fn idle_word(&self, record: &Record) -> bool {
        record
            .window
            .borrow()
            .window
            .as_ref()
            .is_none_or(|window| window.idle_word())
    }

    /// The step's look `looked` at an open window: its rest goes on, begins, or ends.
    /// `idle` is the step's: nothing on its way, the harness at rest and not waiting,
    /// its screen drawn. A rest goes on only over looks that follow each other, each
    /// reading the same frontier.
    pub(crate) fn note_rest(&self, record: &Record, observed: &Observed, idle: bool, looked: u64) {
        // The harness's live word that its window is at work outranks a record that has
        // not caught up (Codex's broker).
        let at_work = record
            .window
            .borrow()
            .window
            .as_ref()
            .is_some_and(|window| window.at_work());
        let mut part = record.delivery.borrow_mut();
        if !(idle && read(observed)) || at_work {
            part.rest = None;
            return;
        }
        let frontier = Frontier::of(observed);
        if let Some(rest) = part
            .rest
            .as_mut()
            .filter(|rest| rest.frontier == frontier && rest.look + 1 == looked)
        {
            rest.look = looked;
            return;
        }
        part.rest = Some(Rest {
            since: self.now(),
            frontier,
            look: looked,
        });
    }

    /// The rest of a window is over: a look at it failed, or it refused a message for
    /// being at work.
    pub(crate) fn rest_ends(&self, record: &Record) {
        let mut part = record.delivery.borrow_mut();
        part.rest = None;
        part.paced = None;
    }

    /// Whether the window has rested long enough for `message` to be typed into it
    /// now. What a launch put ahead of its queue and an urgent row go at its first
    /// rest; anything else waits for the settle, and for the human's keys to have
    /// been quiet as long. Says nothing to the trace: the lane says the window is idle,
    /// and the next look asks again.
    pub(crate) async fn paced(
        self: &Rc<Self>,
        record: &Rc<Record>,
        message: &MessageView,
    ) -> Result<bool, EngineError> {
        record.delivery.borrow_mut().paced = None;
        if self.goes_at_rest(record, message) {
            return Ok(true);
        }
        let Some(rest) = record.delivery.borrow().rest.clone() else {
            return Ok(false);
        };
        let settle = self.seams.limits.settle_ms;
        if settle == 0 {
            return Ok(true);
        }
        let rest_ms = self.now() - rest.since;
        if rest_ms < settle {
            return Ok(false);
        }
        let (window, pane) = {
            let part = record.window.borrow();
            (part.window.clone(), part.pane.clone())
        };
        let (Some(window), Some(pane)) = (window, pane) else {
            return Ok(false);
        };
        // A host that does not answer in time says nothing of the human's keys.
        let Some(snapshot) = self.snapshot_now(&pane).await else {
            return Ok(false);
        };
        if self.forgotten(record) {
            return Ok(false);
        }
        let typed_quiet_ms = ms(&snapshot, "typedQuietMs");
        let output_quiet_ms = ms(&snapshot, "outputQuietMs");
        // None is a window the human never typed in.
        if typed_quiet_ms.is_some_and(|quiet| quiet < settle) {
            return Ok(false);
        }
        let fallback = !window.idle_word();
        if fallback && output_quiet_ms.is_none_or(|quiet| quiet < settle) {
            return Ok(false);
        }
        // A paste the host writes still is no rest of the window's.
        if js::truthy(snapshot.get("pasteInFlight")) {
            return Ok(false);
        }
        record.delivery.borrow_mut().paced = Some(Paced {
            message: message.id,
            frontier: rest.frontier,
            rest_ms,
            typed_quiet_ms,
            output_quiet_ms,
            fallback,
        });
        Ok(true)
    }

    /// What a launch put ahead of the window's queue, and an urgent row, go at the
    /// window's first rest, as they went before the pacing.
    fn goes_at_rest(&self, record: &Record, message: &MessageView) -> bool {
        let prelude = record.delivery.borrow().prelude.clone();
        message.urgent
            || prelude.is_some_and(|prelude| {
                prelude.replacement == message.id || prelude.displaced == Some(message.id)
            })
    }

    /// The fresh look just before `message`'s paste: the window must read at rest, from
    /// a record that was read, on the last item the look that let it go read, and its
    /// input box, where its harness has one it reads, must hold no words. Where it
    /// does not, its rest is over, and nothing is handed over. A message the gate did not
    /// let go (one that goes at rest, or no settle) is not looked at again.
    pub(crate) async fn still_at_rest(
        self: &Rc<Self>,
        record: &Rc<Record>,
        message: i64,
    ) -> Result<bool, EngineError> {
        let paced = record.delivery.borrow().paced.clone();
        let Some(paced) = paced.filter(|paced| paced.message == message) else {
            return Ok(true);
        };
        let Some(participant) = self.participant_of(record)? else {
            return Ok(false);
        };
        let fresh = self.observe(&participant, record).await;
        // A box that holds words (ones the human's keys counted as sent, as Devin kept a
        // line typed with its Enter in one burst) would send a paste behind them, in one
        // message with them: it takes none.
        let (window, pane) = {
            let part = record.window.borrow();
            (part.window.clone(), part.pane.clone())
        };
        let boxed = match (window, pane) {
            (Some(window), Some(pane)) => {
                returning(window.input_box(&*self.seams.host, &pane)).await
            }
            _ => None,
        };
        if self.forgotten(record) {
            return Ok(false);
        }
        let same = boxed != Some(InputBox::Holds)
            && fresh.as_ref().is_ok_and(|fresh| {
                fresh.settled
                    && fresh.waiting.is_none()
                    && read(fresh)
                    && Frontier::of(fresh) == paced.frontier
            });
        if !same {
            self.rest_ends(record);
        }
        Ok(same)
    }

    /// What the look that let `message` go read, for its `delivery.begun`.
    pub(crate) fn paced_look(&self, record: &Record, message: i64) -> Option<Value> {
        record
            .delivery
            .borrow()
            .paced
            .as_ref()
            .filter(|paced| paced.message == message)
            .map(Paced::look)
    }
}

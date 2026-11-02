//! A message its window's record shows and no turn began on (the field bug T-1130). A task
//! is working once its window's record shows its brief (`cf_ledger`'s `receipt`), and
//! every wait of the delivery's is over then (`arrival`): what a window does with what it
//! was given is its turn's to say. A window can keep the words and begin nothing on them:
//! Claude Code puts a message back in its input box when its turn is cut before a word
//! (its record ends on the message), Devin keeps a paste a Stop cancelled in its
//! conversation, Pi resumed on a conversation its quit cut waits at its prompt. Its reader
//! then reads a turn in flight for ever (Claude Code's, Codex's, Pi's) or the window at rest
//! with no answer (Devin's, Muse's), and a turn's end has nothing to take. The board said
//! "working" for an hour while nothing ran, and nobody was told.
//!
//! So the latest message of a working task's that went into a window
//! ([`Dispatcher::latest_message_at`]) is watched until something follows it in the record
//! (an answer, a call, a word of anyone's) or the harness counts work of its own. One that
//! has stood alone for a whole arrival window, the window's screen quiet ([`QUIET_MS`]: a
//! harness at work draws, its spinner if nothing else), and no dialog open, is noticed:
//! - **Once**, Enter is pressed into a window that takes pastes. The record has the words:
//!   what it lacks is what sends them, and a window that holds them in its box sends them
//!   (the words the human typed since are never sent with it: the box's `unsent` holds the
//!   key). A harness that takes its messages over a channel of its own is pressed nothing.
//! - **Then**, a whole arrival window more with nothing begun, the task leaves `working`:
//!   ConsensFlow pauses it as a stall ([`Dispatcher::stall`]), and its requester is told
//!   once, in the pause note the board shows too, which message stood alone since when,
//!   what was pressed, and what the window's screen ends with (masked, as any quote of a
//!   screen: [`Shown::after`]). `cf task resume` sends its words into the same window, where
//!   the brief is above them; the pause asks the window's stop as any pause does, which a
//!   window at rest pays at once.
//!
//! Limits. A window whose screen prints while it begins nothing (a status line that redraws
//! by itself) is waited for until its screen is quiet for [`QUIET_MS`] at a look past its
//! wait; a harness hung in a model request that draws its spinner is at work, and is the
//! stop's to settle. What is watched lives in the engine's memory: a restart watches again
//! from its first look. A read of the window that fails says nothing, and nothing is done
//! on it.

use std::rc::Rc;
use std::sync::Arc;

use cf_base::{js, time};
use cf_harness::contract::{Input, Observed};
use cf_harness::records::{Reading, Role};
use cf_ledger::{ParticipantView, ProjectView};
use cf_proto::trace::WindowEvent;
use serde_json::{json, Value};

use crate::arrival::ENTER_AGAIN_QUIET_MS;
use crate::delivery_text::header_id;
use crate::dispatcher::Dispatcher;
use crate::record::Record;
use crate::runtime::returning;
use crate::seams::EngineError;
use crate::shown::Shown;

/// How long a window's screen must have printed nothing to be taken for one that began
/// nothing: a harness at work draws within it (Claude Code's, Codex's, Pi's, Devin's and
/// Muse's spinners and timers every second or faster).
pub(crate) const QUIET_MS: f64 = 10_000.0;

/// An Enter.
const ENTER: u8 = 13;

/// A message standing alone in a record: its item, and the message by its header.
type Alone = (Arc<str>, Option<i64>);

/// A message of a working task's that its window's record shows with nothing after it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Unbegun {
    /// The record's item that is the message.
    item: Arc<str>,
    /// The message, by its header.
    message: Option<i64>,
    /// When a look first found it alone, on the engine's clock.
    since: i64,
    /// When Enter was pressed once more for it, or its once was spent on a window
    /// pressed nothing.
    entered: Option<i64>,
    /// Its task was paused for it, and its requester told.
    told: bool,
}

impl Dispatcher {
    /// A look at a member's window whose turn came to nothing this pass (`turn_end`): the
    /// latest message of its working task, where its record shows nothing after it, is
    /// watched as this module says.
    pub(crate) async fn watch_begun(
        &self,
        project: &ProjectView,
        participant: &ParticipantView,
        record: &Rc<Record>,
        observed: &Observed,
    ) -> Result<(), EngineError> {
        let Some((item, message)) = self.standing_alone(participant, record, observed)? else {
            *record.never_began.borrow_mut() = None;
            return Ok(());
        };
        let now = self.now();
        let unbegun = {
            let mut part = record.never_began.borrow_mut();
            let kept = part.take().filter(|unbegun| unbegun.item == item);
            part.insert(kept.unwrap_or(Unbegun {
                item,
                message,
                since: now,
                entered: None,
                told: false,
            }))
            .clone()
        };
        if unbegun.told {
            return Ok(());
        }
        let from = unbegun.entered.unwrap_or(unbegun.since);
        if now - from <= self.seams.limits.arrival_ms {
            return Ok(());
        }
        let Some(pane) = record.window.borrow().pane.clone() else {
            return Ok(());
        };
        // The screen's quiet, as the host says it: a look that changes nothing and that the
        // Node traces never held, awaited plainly. A host that cannot say says nothing of it.
        let quiet = self.seams.host.quiet(&pane).await.ok().flatten();
        if !quiet.is_some_and(|quiet| quiet >= QUIET_MS) {
            return Ok(());
        }
        // The window may have moved on while the host answered.
        if record.never_began.borrow().as_ref() != Some(&unbegun) || self.forgotten(record) {
            return Ok(());
        }
        let Some(entered) = unbegun.entered else {
            if let Some(unbegun) = record.never_began.borrow_mut().as_mut() {
                unbegun.entered = Some(now);
            }
            return self.press_begun(record, &unbegun).await;
        };
        // What it shows goes with the note: a snapshot of its screen.
        let Some(snapshot) = self.snapshot_now(&pane, true).await else {
            if let Some(unbegun) = record.never_began.borrow_mut().as_mut() {
                unbegun.entered = Some(now);
            }
            return Ok(());
        };
        if record.never_began.borrow().as_ref() != Some(&unbegun) || self.forgotten(record) {
            return Ok(());
        }
        if let Some(unbegun) = record.never_began.borrow_mut().as_mut() {
            unbegun.told = true;
        }
        let task = self
            .seams
            .ledger
            .borrow()
            .active_task(participant.id, false)?;
        let Some(thread) = task.filter(|thread| thread.task.state == "working") else {
            return Ok(());
        };
        let pressed = matches!(record.window.borrow().input, Input::Pasted { .. });
        // When it went in, as the ledger has it: its record's showing of it.
        let sent = match unbegun.message {
            Some(id) => self.seams.ledger.borrow().message(id)?,
            None => None,
        }
        .and_then(|message| message.delivered_at)
        .and_then(|at| time::parse(&at))
        .unwrap_or(unbegun.since);
        let because = never_began(&unbegun, sent, entered, pressed, now);
        let said = Shown::from_snapshot(&snapshot).after(&because);
        self.seams.log.warn(&format!(
            "@{}'s window began no turn on {}: T-{} is paused",
            participant.handle,
            unbegun
                .message
                .map_or_else(|| "its latest message".to_owned(), |id| format!("m-{id}")),
            thread.task.number
        ));
        self.stall(project, &thread.task, &said, &because, None, false)?;
        self.changed();
        Ok(())
    }

    /// The item of the latest message of `participant`'s working task that its record
    /// shows with nothing after it, and the message by its header: none where something
    /// follows it (an answer, a call, a word of anyone's), the harness counts work of its
    /// own, a dialog waits, the turn failed, another program shares the record, or the
    /// look could not read it. A message still on its way is the delivery's.
    fn standing_alone(
        &self,
        participant: &ParticipantView,
        record: &Record,
        observed: &Observed,
    ) -> Result<Option<Alone>, EngineError> {
        if participant.role == "chief"
            || record.delivery.borrow().delivering.is_some()
            || record.window.borrow().retiring
            || observed.failed
            || observed.shared
            || observed.waiting.is_some()
            || observed.pending().total() > 0
            || observed
                .reading
                .as_deref()
                .is_some_and(Reading::read_failed)
        {
            return Ok(None);
        }
        let task = self
            .seams
            .ledger
            .borrow()
            .active_task(participant.id, false)?;
        let Some(thread) = task.filter(|thread| thread.task.state == "working") else {
            return Ok(None);
        };
        let items = observed.items();
        let replacements = self.seams.ledger.borrow().role_notes_to(participant.id)?;
        let Some(at) = self.latest_message_at(&thread, participant, items, &replacements)? else {
            return Ok(None);
        };
        // What the harness keeps beside a message (an image's sources) is no turn begun.
        if items[at + 1..].iter().any(|item| item.role != Role::Custom) {
            return Ok(None);
        }
        let item = &items[at];
        Ok(Some((Arc::clone(&item.id), header_id(&item.text))))
    }

    /// The once of a message standing alone: Enter, into a window that takes pastes, quiet
    /// and holding none of the human's words unsent. A harness that takes its messages over
    /// a channel of its own is pressed nothing, and its once is spent all the same.
    async fn press_begun(&self, record: &Record, unbegun: &Unbegun) -> Result<(), EngineError> {
        let pane = record.window.borrow().pane.clone();
        let pasted = matches!(record.window.borrow().input, Input::Pasted { .. });
        let Some(pane) = pane.filter(|_| pasted) else {
            return Ok(());
        };
        // What the human typed and has not sent would go with it: the host says so.
        let Some(snapshot) = self.snapshot_now(&pane, false).await else {
            return Ok(());
        };
        let quiet = snapshot
            .get("outputQuietMs")
            .and_then(Value::as_f64)
            .is_some_and(|quiet| quiet >= ENTER_AGAIN_QUIET_MS);
        if !quiet || js::truthy(snapshot.get("unsent")) {
            return Ok(());
        }
        let input = json!({ "id": pane.id, "generation": pane.generation, "bytes": [ENTER] });
        // A key the host did not take is not pressed again: the next wait's end tells.
        let _ = returning(self.seams.host.request("pane.input", input)).await;
        self.seams.log.warn(&format!(
            "the window {} began no turn on {}, which its record shows: Enter was pressed once more",
            pane.id,
            unbegun
                .message
                .map_or_else(|| "its latest message".to_owned(), |id| format!("m-{id}")),
        ));
        match unbegun.message {
            Some(message) => self.trace_window(record, WindowEvent::EnterAgain { message }),
            None => Ok(()),
        }
    }
}

/// Why a task is paused for a message its window began no turn on: which message, when it
/// went in (`sent`), what was pressed and when, in UTC, as the engine's notes say times.
fn never_began(unbegun: &Unbegun, sent: i64, entered: i64, pressed: bool, now: i64) -> String {
    let clock = |at: i64| {
        let at = time::iso(at);
        at.get(11..16).map_or_else(|| at.clone(), str::to_owned)
    };
    let message = unbegun
        .message
        .map_or_else(|| "its latest message".to_owned(), |id| format!("m-{id}"));
    let again = if pressed {
        format!("Enter was pressed once more at {} UTC", clock(entered))
    } else {
        "its harness takes no Enter".to_owned()
    };
    format!(
        "its window began no turn on {message}: it went in at {} UTC and its record shows it with nothing after it, \
         {again}, and nothing began by {} UTC",
        clock(sent),
        clock(now)
    )
}

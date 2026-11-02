//! A resumed task whose window is at work on it reads working (T-1139, field bug 8, the owner's
//! other board, 2026-11-02). `cf tell T-10 "…"` pauses the task and puts the words to its
//! window; the window answered ("ok") and went straight on with its work in the same turn
//! (T-429's and T-442's shape, `turn_end::paused`); the chief's `cf task resume T-10 "…"` then
//! queued its words behind that turn, which nothing is typed into while it is at work
//! (`pacing`), and `cf task list` read `T-10 [queued]` for as long as the window ran
//! commands on it: only the paste of those words moved a task to working (`cf-ledger`'s
//! `reconcile`). The board said the window had not taken the task up while its record showed
//! it acting on nothing else, and neither label could be trusted without the transcript.
//!
//! **The rule.** A task queued by the words that resumed it ([`cf_ledger::Ledger::resume_words_waiting`]),
//! those words not yet being pasted, reads working once the window's record shows it acting
//! after the resume: its turn is in flight, and an item of the agent's own (its words, a tool
//! call or its result) stands in the record after the last item the record held when the
//! words were first seen waiting. The ledger moves the task (`work_while_resumed`) and leaves
//! the words queued: they go in once the window rests, as ever, and the answer to them is the
//! task's result. The turn that ended before them is not (`Dispatcher::collect` leaves a
//! task whose resume words wait to the paused answer's note, `tell_paused_answer`, which tells
//! whoever gave the task what the tell's turn ended with in the words of a task that has moved
//! on).
//!
//! **Why the record's growth, and not the turn in flight alone.** A turn can read in flight
//! and be no work of the window's: a Pi window opened again on a conversation whose quit
//! cut a turn reads it open for ever, and Claude Code's reader does the same for a user
//! record with no turn after it (T-1130's sweep). The task would read working for a window
//! that does nothing, which is the other half of the bug. What the agent writes after the
//! resume is the window acting; a turn that writes nothing (one long tool call, a model
//! request, a compaction) shows the task queued until its first line, which is at most
//! the length of that wait.
//!
//! **And back.** The claim is a live one, and it is withdrawn when its evidence is gone: a
//! window that has rested for [`RESTED_MS`] with the words still not in (the human typed in the
//! window and has not sent it; a Muse window, whose pacing gate held them for the whole of a
//! live run's wait: "its screen printed within the last 3 s") is at work on nothing, and the task reads queued again until they go in (`rest_while_resumed`). A rest of
//! seconds is the settle and the paste, which the claim outlasts: no flicker where the words go
//! in as they should.
//!
//! **What it does not touch.** The daemon's own bare words that resume a task held for quota
//! are none of these ([`cf_ledger::Ledger::resume_words_waiting`]): a window that goes on by
//! itself withdraws them (`went_on_by_itself`). A window at rest is given the words as before,
//! the task working from their paste (`confirm_arrival`, which T-1130's rule about a brief
//! that began no turn also stands on). A launch's first message, a follow-up of a done task
//! and every other message are not resume words.
//!
//! Limits. The baseline is the record at the first look that found the words waiting, kept
//! in the engine's record of the window: a daemon that restarts takes a new one, and shows
//! the work from the first line it writes after that look. A record read again whose baseline
//! item is gone (a compaction rewrote it) takes a new baseline and claims nothing from that
//! look. Every harness's reader gives the items this reads; which of them keep working
//! after a tell is the live suite's to say.

use std::sync::Arc;

use cf_harness::contract::{Observed, Over};
use cf_harness::records::{Item, Reading, Role};
use cf_ledger::{ParticipantView, ProjectView};

use crate::dispatcher::Dispatcher;
use crate::record::Record;
use crate::seams::EngineError;

/// What the record of a window held when the words that resumed its task were first seen
/// waiting: the last item it had, none if it had none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResumeWatch {
    /// The resume's words, a message.
    pub(crate) words: i64,
    /// The id of the last item of the record then.
    pub(crate) last: Option<Arc<str>>,
}

/// Whether the look reads the window's turn in flight and its record whole: a turn the
/// record says is over, or may be ending (`Over`), one that failed, a window waiting on a
/// dialog of its own or starting, another conversation's, one another program of the
/// harness's writes into, or a record that could not be read, is no work shown.
fn in_flight(observed: &Observed) -> bool {
    matches!(observed.reading.as_deref(), Some(Reading::Known(_)))
        && !observed.settled
        && observed.over == Over::No
        && !observed.failed
        && !observed.took_back
        && !observed.shared
        && !observed.unnamed
        && observed.waiting.is_none()
        && observed.switched.is_none()
}

/// How long a window must have rested, with the words that resumed its task still not in,
/// for the task to read queued again. The settle and the paste take a few seconds, so only
/// words that are held (the human typed in the window and has not sent it, a screen that
/// never goes quiet) show it.
const RESTED_MS: i64 = 10_000;

/// Whether the look reads the window's turn over, by its record whole: the window is at rest
/// or may be given words (a dialog of its own, another conversation, a record that could
/// not be read, and a turn that failed are none).
fn at_rest(observed: &Observed) -> bool {
    matches!(observed.reading.as_deref(), Some(Reading::Known(_)))
        && (observed.settled || observed.over == Over::Held)
        && !observed.failed
        && !observed.unnamed
        && observed.waiting.is_none()
        && observed.switched.is_none()
}

/// Whether an item is the agent's own work: its words or a tool, not what the harness writes
/// in its place (Claude Code's close of a cut turn) or a compaction's summary.
fn is_work(item: &Item) -> bool {
    matches!(item.role, Role::Assistant | Role::Tool) && !item.synthetic && !item.compaction
}

/// Whether the record shows work after `watch`'s last item: none where that item is gone
/// from the record (it was rewritten), which the caller takes a new baseline for.
fn worked_since(watch: &ResumeWatch, items: &[Item]) -> Option<bool> {
    let from = match &watch.last {
        None => 0,
        Some(last) => items.iter().rposition(|item| item.id == *last)? + 1,
    };
    Some(items[from..].iter().any(is_work))
}

impl Dispatcher {
    /// One look at a member's window, as far as the words that resumed its task go: the
    /// task reads working once the record shows the window acting on it (see above).
    pub(crate) fn show_resumed_at_work(
        &self,
        project: &ProjectView,
        participant: &ParticipantView,
        record: &Record,
        observed: &Observed,
    ) -> Result<(), EngineError> {
        let task = self.seams.ledger.borrow().task_in_hand(participant.id)?;
        let words = task
            .as_ref()
            .filter(|thread| matches!(thread.task.state.as_str(), "queued" | "working"))
            .map(|thread| {
                self.seams
                    .ledger
                    .borrow()
                    .resume_words_waiting(project.id, thread.task.number)
            })
            .transpose()?
            .flatten();
        let (Some(thread), Some(words)) = (task, words) else {
            record.delivery.borrow_mut().resumed = None;
            return Ok(());
        };
        // Their paste is begun: its arrival moves the task, as it moves any.
        if record.delivery.borrow().delivering.is_some() {
            return Ok(());
        }
        // Shown working for them, and the window has rested without them going in: no
        // window is at work on it, and it reads queued until they are.
        if thread.task.state == "working" {
            record.delivery.borrow_mut().resumed = None;
            let rested = self.rested_for(record).is_some_and(|ms| ms >= RESTED_MS);
            if rested && at_rest(observed) && project.state == "open" {
                match self
                    .seams
                    .ledger
                    .borrow_mut()
                    .rest_while_resumed(project.id, thread.task.number)
                {
                    Ok(_) => self.changed(),
                    Err(cf_ledger::LedgerError::Refused(_)) => {}
                    Err(failed) => return Err(failed.into()),
                }
            }
            return Ok(());
        }
        if !matches!(observed.reading.as_deref(), Some(Reading::Known(_))) {
            return Ok(());
        }
        let items = observed.items();
        let baseline = || ResumeWatch {
            words,
            last: items.last().map(|item| Arc::clone(&item.id)),
        };
        let watch = record
            .delivery
            .borrow()
            .resumed
            .clone()
            .filter(|watch| watch.words == words);
        let Some(watch) = watch else {
            record.delivery.borrow_mut().resumed = Some(baseline());
            return Ok(());
        };
        let worked = match worked_since(&watch, items) {
            Some(worked) => worked,
            None => {
                record.delivery.borrow_mut().resumed = Some(baseline());
                return Ok(());
            }
        };
        if !worked || !in_flight(observed) || project.state != "open" {
            return Ok(());
        }
        match self
            .seams
            .ledger
            .borrow_mut()
            .work_while_resumed(project.id, thread.task.number)
        {
            Ok(_) => {}
            // The words went in, or the task moved, since the look: nothing to show.
            Err(cf_ledger::LedgerError::Refused(_)) => return Ok(()),
            Err(failed) => return Err(failed.into()),
        }
        record.delivery.borrow_mut().resumed = None;
        self.changed();
        Ok(())
    }

    /// Whether the words that resumed T-`number` are on their way to the window still: such
    /// a task reads working while its window is at work on it, and what its turn ends with is
    /// not its result (see above).
    pub(crate) fn resume_words_wait(&self, project: i64, number: i64) -> Result<bool, EngineError> {
        Ok(self
            .seams
            .ledger
            .borrow()
            .resume_words_waiting(project, number)?
            .is_some())
    }
}

//! A turn that failed is tried again twice before its task fails (D3's N35). A try is a hold of
//! the task and main's own Resume: where main would fail a working task for a failed end, the
//! task is held for 30 seconds the first time and for 2 minutes the second, and the pass that
//! finds the hold due resumes it with main's own words. Nothing of a retry is pending anywhere
//! else: no row with a due time, no message of its own kind, no timer, no deadline.
//!
//! **The count is stored, never read from the order of messages.** The tries a task has used in
//! a row are a number on the task (`task.turn_retries`), raised by the ledger in the write that
//! queues a numbered Resume and put back to zero on exactly three things: a person's or the
//! chief's action on the task (a Resume or reopen, a tell, an answer), its result, and a failure
//! 30 minutes or more after `task.turn_retry_at`, the last moment anything happened to a
//! numbered retry of the task (one was charged, delivered or withdrawn: written in the same
//! write as its cause, by nothing else). A clock that reads earlier than that time never renews.
//! A task given to another window, by the daemon to its member's backup or by a person's
//! Reassign, is a fresh run and begins at zero: each window gets at most two automatic Resumes
//! in a row. A quota's or a busy model's own Resume is neither: the count stands across it, and
//! the same window coming back from such a hold begins nothing again. No harness's reading of a
//! turn's end resets it. The daemon alone can never try more than twice in a row; if the
//! computer's clock jumps forward, the 30 minutes can end early.
//!
//! **Nothing reads why.** No code, no kind and no words of the failure decide: a missing model
//! gets the same two tries.

use std::rc::Rc;

use cf_harness::contract::Observed;
use cf_harness::records::{Quota, Reading, Settlement};
use cf_ledger::{NewNote, ParticipantView, ProjectView, RetryFact, RetryHold, RetryPlan};
use cf_proto::ledger::TaskView;

use crate::dispatcher::Dispatcher;
use crate::record::Record;
use crate::seams::EngineError;
use crate::turn_end::Watching;

/// How many units a failed turn's second try waits (30 seconds, 2 minutes: four to one).
const SECOND_TRY_UNITS: i64 = 4;
/// How long after `turn_retry_at` (the last moment anything happened to a numbered retry) a failed
/// turn gets a fresh pair of tries.
const RENEWAL_MS: i64 = 30 * 60 * 1000;

/// Whether the look shows a turn positively at work: its record's settlement says in flight.
/// A failed end its harness still works on (Codex's parent turn errored with a child agent at
/// work) decides nothing until it settles.
pub(crate) fn turn_at_work(observed: &Observed) -> bool {
    !observed.settled
        && matches!(
            observed.reading.as_deref(),
            Some(Reading::Known(record)) if record.settlement == Settlement::InFlight
        )
}

/// A wait in the words a note says it: whole minutes, else whole seconds, else milliseconds.
fn spoken(ms: i64) -> String {
    let (count, unit) = if ms % 60_000 == 0 {
        (ms / 60_000, "minute")
    } else if ms % 1_000 == 0 {
        (ms / 1_000, "second")
    } else {
        (ms, "millisecond")
    };
    format!("{count} {unit}{}", if count == 1 { "" } else { "s" })
}

/// What a failed turn comes to for its task.
pub(crate) enum Tried {
    /// The task is held, and main's Resume at its time is the try.
    Held,
    /// The task fails, with this reason.
    Fails(String),
}

impl Dispatcher {
    /// A failed turn where main would fail its working task: the task is held for a retry if
    /// it has a try left, in one look and one write against this moment (the clock read once),
    /// the first try told once to whoever gave the task; else it fails with the turn's reason
    /// ending "(tried again twice)". Where the tries are off (`Limits::turn_retry_ms` 0) it
    /// fails as before.
    pub(crate) fn failed_end(
        &self,
        project: &ProjectView,
        participant: &ParticipantView,
        task: &TaskView,
        said: &str,
        cause: &str,
    ) -> Result<Tried, EngineError> {
        let unit = self.seams.limits.turn_retry_ms;
        if unit == 0 || participant.role == "chief" {
            return Ok(Tried::Fails(said.to_owned()));
        }
        // The tries used are a number on the task, kept by the ledger in the writes that queue
        // a numbered Resume and in those that start the count again (a person's or the chief's
        // action on the task, its result, or a failure 30 minutes after `turn_retry_at`). The ledger decides in this one write; nothing here orders
        // messages by their times or reads a harness's idea of how a turn ended.
        let held = self.seams.ledger.borrow_mut().hold_task_for_retry(
            project.id,
            task.number,
            &format!("its turn failed: {said}"),
            &RetryFact {
                session: participant.id,
                kind: "failed",
                said,
                cause,
            },
            RetryPlan {
                unit_ms: unit,
                second_units: SECOND_TRY_UNITS,
                renew_after_ms: RENEWAL_MS,
            },
        )?;
        let which = match held {
            RetryHold::Spent => return Ok(Tried::Fails(format!("{said} (tried again twice)"))),
            RetryHold::Held { which } => which,
        };
        if which == 1 {
            // One note, at the first try, to whoever gave the task: shown on the board where
            // that is the chief (it costs no turn), typed to anyone else.
            let words = format!(
                "T-{n}'s turn failed: {said}. ConsensFlow tries it again in its window in {first}, and once more {second} after a second failure. If it fails a third time in a row, T-{n} fails; its count of tries starts again with a word to it from you or the chief, a result, or another window (its member's backup, or a Reassign); by time it starts again on a failure 30 minutes or more after the last moment anything happened to one of its automatic tries. To change what it is asked meanwhile, resume it with the work rephrased: cf task resume T-{n} \"…\"",
                n = task.number,
                first = spoken(unit),
                second = spoken(SECOND_TRY_UNITS * unit),
            );
            let aside = format!("its turn failed: ConsensFlow tries it again ({said})");
            self.seams.ledger.borrow_mut().show(
                project.id,
                &NewNote {
                    from: None,
                    to: task.requester.clone(),
                    body: words,
                    task: Some(task.number),
                },
                &aside,
            )?;
        }
        self.changed();
        Ok(Tried::Held)
    }

    /// A count at its limit is no refusal of a turn: where the turn ended with a proven final
    /// answer, that answer is collected once, before main's hold (T-1920's finding 22).
    pub(crate) async fn keep_proven_answer(
        self: &Rc<Self>,
        project: &ProjectView,
        participant: &ParticipantView,
        record: &Rc<Record>,
        observed: &Observed,
    ) -> Result<(), EngineError> {
        let reported = record.quota.borrow().reported.clone();
        if participant.role == "chief"
            || !matches!(reported.as_deref(), Some(Quota::Usage { .. }))
            || !observed.settled
            || observed.failed
            || observed.shared
        {
            return Ok(());
        }
        // Its brief's own receipt may show in this very look: read before the answer is.
        if record.delivery.borrow().delivering.is_some() {
            self.confirm_arrival(record, observed)?;
        }
        self.collect(project, participant, record, observed, Watching::Undecided)
            .await?;
        Ok(())
    }
}

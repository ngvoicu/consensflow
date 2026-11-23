//! A turn that failed is tried again twice before its task fails (D3's N35). A try is a hold of
//! the task and main's own Resume: where main would fail a working task for a failed end, the
//! task is held for 30 seconds the first time and for 2 minutes the second, and the pass that
//! finds the hold due resumes it with main's own words. Nothing of a retry is pending anywhere
//! else: no row with a due time, no message of its own kind, no timer, no deadline.
//!
//! **The count is read, never kept.** `n` is the try that the message last pasted into the
//! window about the task carried (`Ledger::turn_retry_tries`): 0 for any message but a retry's
//! Resume. A try counts only once its paste is received, and only a turn that fails again
//! counts; an answer, a tell or a person's Resume pasted since makes `n` 0 again. The daemon
//! alone can never try more than twice in a row.
//!
//! **Nothing reads why.** No code, no kind and no words of the failure decide: a missing model
//! gets the same two tries.

use std::rc::Rc;

use cf_base::time;
use cf_harness::contract::Observed;
use cf_harness::records::{Quota, Reading, Settlement};
use cf_ledger::{NewNote, ParticipantView, ProjectView, RetryFact};
use cf_proto::ledger::TaskView;

use crate::dispatcher::Dispatcher;
use crate::record::Record;
use crate::seams::EngineError;
use crate::turn_end::Watching;

/// How many units a failed turn's second try waits (30 seconds, 2 minutes: four to one).
const SECOND_TRY_UNITS: i64 = 4;
/// How many tries a turn gets.
const TRIES: i64 = 2;

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
        let n = {
            let ledger = self.seams.ledger.borrow();
            match ledger.last_pasted(participant.id, task.id)? {
                Some(message) => ledger.turn_retry_tries(message.id)?,
                None => 0,
            }
        };
        if n >= TRIES {
            return Ok(Tried::Fails(format!("{said} (tried again twice)")));
        }
        let wait = if n == 0 {
            unit
        } else {
            SECOND_TRY_UNITS * unit
        };
        let until = time::iso(self.now() + wait);
        self.seams.ledger.borrow_mut().hold_task_for_retry(
            project.id,
            task.number,
            &until,
            &format!("its turn failed: {said}"),
            &RetryFact {
                session: participant.id,
                kind: "failed",
                which: n + 1,
                said,
                cause,
            },
        )?;
        if n == 0 {
            // One note, at the first try, to whoever gave the task: shown on the board where
            // that is the chief (it costs no turn), typed to anyone else.
            let words = format!(
                "T-{n}'s turn failed: {said}. ConsensFlow tries it again in its window in 30 seconds, and once more 2 minutes after a second failure. If that fails too, T-{n} fails.",
                n = task.number
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

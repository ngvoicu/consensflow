//! What a launch that says the role first owes, and what comes of one that
//! cannot. A launch whose conversation is to be told a changed role
//! (`role_change`) opens its window with no first message: the replacement of the
//! role is its first delivery, by paste once the window is up, and the message it
//! displaced (a handoff, a brief, the note that its turn was cut) goes after it.
//! The launch keeps what a launch with a first message on its command line has:
//!
//! - Its ownership, until the replacement is told. A window that exits before
//!   (the replacement waiting, or on its way) settles as a launch whose first
//!   message the window never showed: a member's displaced message fails, once,
//!   and the replacement is withdrawn, so that no launch follows at the next pass
//!   and no replacement is written for it. A chief's displaced message stays in
//!   its queue, as a chief's first message does, and the chief is tried again at
//!   the backoff (`launch_failed`): its window went before it came up, and that
//!   exit closes no project, as the exit of a chief that was up does. The
//!   human's own Close, which has the project closed first, is no such failure.
//! - Its deadline, until the replacement's delivery begins. A member's window
//!   that is not ready for a paste within `launch_ms` of the launch is closed and
//!   the launch settled the same way (`watch_prelude`); the time its member is
//!   out of quota is not counted. A chief's window is the human's, and is never
//!   closed for being slow.
//!
//! A replacement whose delivery is given up (refused, never shown in the record,
//! its window closed while it was handed over: as many attempts as a message is
//! given) does not let the conversation go on under the role it has. The next
//! look settles it as a launch that did not come up (`abandon_launch`), before
//! anything else of the window's queue goes in: the window is closed; a member's
//! displaced message fails, once; a chief's stays in its queue and the chief is
//! tried again at the backoff (`launch_failed`), a new replacement written for
//! each launch and the human told once. A chief's window the host would not kill
//! is still its window, under the role it has: the next look tries again, and
//! nothing goes in meanwhile.
//!
//! What a chief's launch put ahead of its queue (the replacement and the message
//! it displaced) is its first messages, as its handoff is on the command line:
//! given back to the queue with their attempt where the window goes, and never
//! given up for being slow. What goes into a member's window after the
//! replacement goes as any message does.

use std::rc::Rc;

use cf_ledger::{LedgerError, MessageView, ParticipantView, ProjectView};

use crate::deliveries::Delivering;
use crate::dispatcher::Dispatcher;
use crate::record::Record;
use crate::runtime::begin;
use crate::seams::EngineError;

/// What a launch put ahead of its window's queue: the replacement of the role,
/// and the message the launch would have begun with, which goes after it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Prelude {
    /// The replacement of the role, the first to go in.
    pub(crate) replacement: i64,
    /// The message the launch would have begun with, which goes after it.
    pub(crate) displaced: Option<i64>,
    /// The window is a chief's: it is the human's, and is never closed for being slow.
    pub(crate) chief: bool,
    /// When the launch began, on the engine's clock.
    pub(crate) since: i64,
    /// The replacement's delivery began: the launch's deadline is over, and the
    /// delivery's own rules go on.
    pub(crate) begun: bool,
}

impl Prelude {
    /// Whether `message` is one of the two the launch put ahead of its queue.
    pub(crate) fn holds(&self, message: i64) -> bool {
        message == self.replacement || self.displaced == Some(message)
    }
}

impl Dispatcher {
    /// The next message to go into `record`'s window: what the launch put ahead of
    /// its queue, in its order, while it waits and may go, else the head of the
    /// queue. The replacement of a role goes whatever its recipient holds (a
    /// note about no task would not be the head of a member's queue); the message
    /// it displaced goes by the rules the head goes by. Nothing goes while a
    /// delivery is on its way. What the launch put ahead of its queue and has
    /// been settled since (delivered or withdrawn) is passed over; one given up
    /// is settled by the look that comes before any message goes in
    /// ([`Dispatcher::watch_prelude`]).
    pub(crate) fn next_in_line(&self, record: &Record) -> Result<Option<MessageView>, EngineError> {
        let head = self.head_of_line(record)?;
        // A message of another task than the one whose work the window holds waits for a
        // window of its own (`ownership`): what is of the window's task goes before it.
        let owner = record.window.borrow().owner;
        let (Some(owner), Some(other)) = (owner, head.as_ref()) else {
            return Ok(head);
        };
        if other.task_number.is_none_or(|number| number == owner) {
            return Ok(head);
        }
        let ledger = self.seams.ledger.borrow();
        for waiting in ledger.pending(record.id)? {
            if waiting.state == "queued"
                && waiting.task_number == Some(owner)
                && ledger.next_delivery_of(record.id, waiting.id)?.is_some()
            {
                return Ok(Some(waiting));
            }
        }
        Ok(head)
    }

    /// The head of `record`'s queue, the prelude a launch put ahead of it first.
    fn head_of_line(&self, record: &Record) -> Result<Option<MessageView>, EngineError> {
        let ledger = self.seams.ledger.borrow();
        let prelude = record.delivery.borrow().prelude.clone();
        let Some(prelude) = prelude else {
            return Ok(ledger.next_delivery(record.id)?);
        };
        let mut waiting = Vec::new();
        for id in [Some(prelude.replacement), prelude.displaced]
            .into_iter()
            .flatten()
        {
            let found = ledger.message(id)?;
            waiting.extend(
                found.filter(|message| matches!(message.state.as_str(), "queued" | "delivering")),
            );
        }
        let busy = !waiting.is_empty()
            && ledger
                .pending(record.id)?
                .iter()
                .any(|other| other.state == "delivering");
        if !busy {
            for message in waiting {
                let goes = message.state == "queued"
                    && (message.id == prelude.replacement
                        || ledger.next_delivery_of(record.id, message.id)?.is_some());
                if goes {
                    return Ok(Some(message));
                }
            }
        }
        Ok(ledger.next_delivery(record.id)?)
    }

    /// The prelude of `record`'s window while its replacement has not been told:
    /// it waits, is on its way, or was given up. A launch with one is not done
    /// with.
    pub(crate) fn owed(&self, record: &Record) -> Result<Option<Prelude>, EngineError> {
        let prelude = record.delivery.borrow().prelude.clone();
        prelude.map_or(Ok(None), |prelude| self.still_owed(prelude))
    }

    /// `prelude` while its replacement has not been told, as the ledger has it now.
    pub(crate) fn still_owed(&self, prelude: Prelude) -> Result<Option<Prelude>, EngineError> {
        let told = self
            .seams
            .ledger
            .borrow()
            .message(prelude.replacement)?
            .is_some_and(|message| {
                !matches!(message.state.as_str(), "queued" | "delivering" | "failed")
            });
        Ok((!told).then_some(prelude))
    }

    /// Notes that `message` is going into `record`'s window, and says whether it
    /// is one of a chief's launch: the replacement or what it displaced.
    pub(crate) fn begins(&self, record: &Record, message: i64) -> bool {
        let mut delivery = record.delivery.borrow_mut();
        let Some(prelude) = delivery
            .prelude
            .as_mut()
            .filter(|prelude| prelude.holds(message))
        else {
            return false;
        };
        prelude.begun |= message == prelude.replacement;
        prelude.chief
    }

    /// The message a member's launch was for, which the replacement of a role went
    /// ahead of: what the launch's failure settles, as it would have as the
    /// launch's first message, and what the plan of a launch gives back to its
    /// queue (a member's message is begun before its harness is asked, and the
    /// role is compared after). None for a chief's, which waits in its queue for
    /// its next window, and none where no message was displaced.
    pub(crate) fn displaced(
        &self,
        participant: &ParticipantView,
        prelude: Option<&Prelude>,
    ) -> Option<Delivering> {
        let message = prelude?.displaced?;
        (participant.role != "chief").then(|| Delivering {
            message,
            since: self.now(),
            // A launch's first message is waited for by the launch's own time.
            rested_since: None,
            launch: true,
            chief: false,
            queued: false,
            free_since: None,
            entered_again: false,
        })
    }

    /// The message a launch was for, which the replacement of a role went ahead
    /// of, is begun where it waits still, so that the launch's failure settles it
    /// as it would have as the launch's first message. One that rides in another
    /// message's paste by now (its task was paused and resumed while the window
    /// waited) goes with its carrier, and there is nothing of it to fail.
    pub(crate) fn begin_displaced(&self, delivering: &Delivering) -> Result<(), EngineError> {
        let queued = self
            .seams
            .ledger
            .borrow()
            .message(delivering.message)?
            .is_some_and(|message| message.state == "queued");
        if queued {
            let begun = self
                .seams
                .ledger
                .borrow_mut()
                .begin_delivery(delivering.message);
            if let Err(error) = begun {
                if !matches!(error, LedgerError::Refused(_)) {
                    return Err(error.into());
                }
            }
        }
        Ok(())
    }

    /// A window that exited with `prelude` owed (`owed`): the launch did not come
    /// up. A member's displaced message fails with it, as it would have as the
    /// launch's first message, and the replacement is withdrawn. A chief's stays
    /// in its queue, and the chief is tried again at the backoff
    /// (`launch_failed`); its exit closes no project, where the exit was neither
    /// the human's (a project already closed) nor the engine's own.
    pub(crate) fn settle_exited_launch(
        &self,
        record: &Rc<Record>,
        project: &ProjectView,
        participant: &ParticipantView,
        prelude: &Prelude,
        because: &str,
    ) -> Result<(), EngineError> {
        if participant.role == "chief" {
            let went_by_itself = project.state == "open" && !record.window.borrow().own_exit;
            return if went_by_itself {
                self.launch_failed(record, project, participant, None, because)
            } else {
                Ok(())
            };
        }
        match self.displaced(participant, Some(prelude)) {
            Some(delivering) => {
                self.launch_failed(record, project, participant, Some(delivering), because)
            }
            None => {
                self.withdraw_replacements(participant)?;
                Ok(())
            }
        }
    }

    /// One look at what a launch that says the role first still owes, with its
    /// window open: the replacement given up is a launch that did not come up, and
    /// so is a member's window that has not been ready for it within the time a
    /// launch is given. Says whether the launch was given up, its window closing.
    pub(crate) async fn watch_prelude(
        self: &Rc<Self>,
        project: &ProjectView,
        participant: &ParticipantView,
        record: &Rc<Record>,
    ) -> Result<bool, EngineError> {
        let Some(prelude) = record.delivery.borrow().prelude.clone() else {
            return Ok(false);
        };
        let state = self
            .seams
            .ledger
            .borrow()
            .message(prelude.replacement)?
            .map(|message| message.state);
        let slow = !prelude.begun
            && !prelude.chief
            && self.now() - prelude.since > self.seams.limits.launch_ms;
        let because = match state.as_deref() {
            Some("failed") => "the note that says its role did not reach it",
            Some("queued") if slow => "the window was never ready for the note that says its role",
            _ => return Ok(false),
        };
        self.abandon_launch(project, participant, record, because)
            .await?;
        Ok(true)
    }

    /// A launch that was to say the role first and cannot: its window is closed
    /// and the launch settled as one that did not come up. What the window shows is
    /// asked first, for the screen goes with it. An exit that came while the host
    /// answered settled the launch already.
    pub(crate) async fn abandon_launch(
        self: &Rc<Self>,
        project: &ProjectView,
        participant: &ParticipantView,
        record: &Rc<Record>,
        because: &str,
    ) -> Result<(), EngineError> {
        let pane = record.window.borrow().pane.clone();
        let (shown, prelude) = self.look_before_abandoning(record, pane.as_ref()).await;
        let Some(prelude) = prelude else {
            return Ok(());
        };
        let said = self.unstarted_because(record, because, &shown);
        let failing = self.displaced(participant, Some(&prelude));
        if participant.role == "chief" {
            // The chief's window goes without taking its project with it, and the
            // chief is tried again. A window the host would not kill is still its
            // window, under the role it has: the launch is looked at again at the
            // next look, and nothing goes in until then.
            let closed = match &pane {
                Some(pane) => self.close_own(record, pane).await?,
                None => true,
            };
            if !closed {
                record.delivery.borrow_mut().prelude = Some(prelude);
                return Ok(());
            }
            return self.launch_failed(record, project, participant, failing, &said);
        }
        let (this, held) = (Rc::clone(self), Rc::clone(record));
        let closing = begin(&*self.seams.spawn, async move { this.retire(&held).await }).await;
        self.launch_failed(record, project, participant, failing, &said)?;
        closing.await?;
        Ok(())
    }
}

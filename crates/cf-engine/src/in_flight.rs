//! What was on its way into a window when the daemon or the window went, and what
//! the window's harness recorded of it, read with no window open. A message whose
//! header ConsensFlow's copy of the window shows had arrived, and so had one the
//! harness's own record shows (the copy can lag the record): the conversation that
//! goes on holds it, so it is confirmed, never sent again. That is how a start
//! settles what the previous process left on its way, and how a window's end settles
//! the paste it went with: its program ended (a crash, a close) after its harness took
//! the paste and before a look confirmed it, or while the paste was being handed over.
//! Sent again, that paste was read twice when the conversation went on (found on
//! 2026-10-16, after the field reports of the day before: windows that closed in the
//! middle of their tasks, and a result a chief read twice). What the record does not
//! show goes back to its queue, as it always did.
//!
//! And the turn a participant's record ends in, read the same way: whether the quit
//! or a Close cut it.

use std::rc::Rc;
use std::sync::Arc;

use cf_harness::records::{Options, Reading, Role};
use cf_ledger::MessageView;
use cf_proto::agents::Harness;
use serde_json::json;

use crate::deliveries::Delivering;
use crate::delivery_text::{is_paste_of, marker_of};
use crate::dispatcher::Dispatcher;
use crate::record::Record;
use crate::runtime::{caught, returning};
use crate::seams::EngineError;

impl Dispatcher {
    /// Once, at start: what was on its way to a window when the previous process
    /// ended. One that arrived ([`Dispatcher::arrived`]) is confirmed; any other goes
    /// back to its queue with its attempt, as does one whose record cannot be read.
    /// What a message's paste carried goes with it either way (the ledger keeps the
    /// set of a carrier on its way when it is opened). Each is settled apart: one that
    /// fails is written down, the first is the start's answer, and the rest are settled
    /// all the same (the reviews of 2026-10-16: one brief's failure left the next on its
    /// way, with no window to watch it, and its task queued for good).
    pub(crate) async fn settle_in_flight(self: &Rc<Self>) -> Result<(), EngineError> {
        // No door lives through a start: what one claimed is the ledger's again.
        self.seams.ledger.borrow_mut().release_all_claims()?;
        let messages = self.seams.ledger.borrow().in_flight()?;
        let mut failed = None;
        for message in messages {
            if let Err(cause) = self.settle_one_in_flight(&message).await {
                match failed {
                    None => failed = Some(cause),
                    Some(_) => self.write_down(&cause),
                }
            }
        }
        failed.map_or(Ok(()), Err)
    }

    /// One message on its way at the start, settled by what showed of it. The message
    /// is read again once the record is: one that is on its way no more (its task
    /// cancelled meanwhile, as the human may while the start reads) is left as it is.
    async fn settle_one_in_flight(&self, message: &MessageView) -> Result<(), EngineError> {
        let arrived = self.arrived(message).await?;
        let Some(message) = self.still_delivering(message.id)? else {
            return Ok(());
        };
        match arrived {
            None => {
                self.seams.ledger.borrow_mut().retry_delivery(
                    message.id,
                    "the daemon stopped before it arrived",
                    true,
                )?;
            }
            Some(item) => {
                self.confirm_delivered(&message, &json!({ "item": item }))?;
            }
        }
        Ok(())
    }

    /// The paste `delivering` was on its way into a window that went: its program
    /// ended, or the window closed while its harness took it. One that arrived
    /// ([`Dispatcher::arrived`]) is confirmed; any other is settled by `otherwise`, as
    /// it was before the window's record was asked. So are the chief's first messages
    /// (its handoff, and what its launch put ahead of its queue), which `otherwise`
    /// gives back to wait for its next window, on the same conversation: one its
    /// record shows went there was given again (a review of 2026-10-16: a Codex chief
    /// that recorded its handoff and closed before a look was given it twice). A
    /// member's launch's first message is `otherwise`'s alone: it fails with the
    /// launch, and is never pasted again. The message is read
    /// again once the record is: one that is on its way no more (its task cancelled
    /// meanwhile, or taken back) is left as it is, and the window's end goes on (a
    /// review of 2026-10-16: a confirm of a message cancelled meanwhile failed, and the
    /// chief's exit with it, which left the project open and its windows running).
    /// What the participant is may have moved on meanwhile
    /// ([`Dispatcher::still_as_it_went`]).
    pub(crate) async fn settle_gone(
        self: &Rc<Self>,
        delivering: Delivering,
        otherwise: impl FnOnce(Delivering) -> Result<(), EngineError>,
    ) -> Result<(), EngineError> {
        if delivering.launch && !delivering.chief {
            return otherwise(delivering);
        }
        let Some(message) = self.still_delivering(delivering.message)? else {
            return Ok(());
        };
        let arrived = self.arrived(&message).await?;
        let Some(message) = self.still_delivering(message.id)? else {
            return Ok(());
        };
        match arrived {
            Some(item) => {
                self.confirm_delivered(&message, &json!({ "item": item }))?;
                self.changed();
            }
            None => otherwise(delivering)?,
        }
        Ok(())
    }

    /// The message `id` as the ledger has it, while it is on its way.
    fn still_delivering(&self, id: i64) -> Result<Option<MessageView>, EngineError> {
        let message = self.seams.ledger.borrow().message(id)?;
        Ok(message.filter(|found| found.state == "delivering"))
    }

    /// Whether the participant of a window that went is still as the window left it
    /// once its record was read with no window open: kept, with no window asked for
    /// it since the exit began, which counted `launches` then (the window part's
    /// count). Meanwhile the human may have switched the chief, whose new window's
    /// launch begins with the switch, and whose project the old window's exit then
    /// suspended, its workers' windows closed; or opened a session's window again,
    /// whose task the old one's exit then paused (the reviews of 2026-10-16); or
    /// closed the project and resumed it, whose chief the Resume brings back as the
    /// old exit suspends it, so the chief runs in a project that is suspended (a review
    /// of 2026-10-16: a window asked for and not yet launched, its Resume reading
    /// whether the turn was cut, is counted too).
    pub(crate) fn still_as_it_went(&self, record: &Rc<Record>, launches: u64) -> bool {
        record.window.borrow().launches == launches && !self.forgotten(record)
    }

    /// The item that proves `message` reached its recipient's window: the first that
    /// is its paste ([`is_paste_of`]) in ConsensFlow's copy of the window's
    /// conversation, else in the harness's own record of it, behind the brief a fresh
    /// window is given first where it was its launch's first
    /// ([`Dispatcher::brief_given_first`]). None where neither shows it, or the record
    /// cannot be read.
    async fn arrived(&self, message: &MessageView) -> Result<Option<String>, EngineError> {
        let first = self.brief_given_first(message)?;
        let copied = self
            .seams
            .ledger
            .borrow()
            .copied_items_with(message.recipient_id, &marker_of(message.id))?
            .into_iter()
            .find(|(_, text)| is_paste_of(text, message.id, first.as_deref()));
        match copied {
            Some((item, _)) => Ok(Some(item)),
            // An `async` method: its answer reaches this a turn after it was made.
            None => {
                returning(self.recorded_paste_of(message.recipient_id, message.id, first)).await
            }
        }
    }

    /// The first item a participant's harness recorded it was given that is
    /// the paste of `message` ([`is_paste_of`]), behind `first` where that is
    /// what its window was given ahead of it, read with no window open: none
    /// when none is, or when its record cannot be read.
    pub(crate) async fn recorded_paste_of(
        &self,
        participant: i64,
        message: i64,
        first: Option<String>,
    ) -> Result<Option<String>, EngineError> {
        let Some((harness, session)) = self.readable(participant)? else {
            return Ok(None);
        };
        // The adapter's `record` is an `async` function, and `.catch(() =>
        // null)` made a promise of its own to wait on.
        let reading = caught(
            self.seams
                .records
                .look(harness, &session, &Options::default()),
        )
        .await;
        let Reading::Known(record) = &*reading else {
            return Ok(None);
        };
        Ok(record
            .items
            .iter()
            .find(|item| {
                item.role == Role::User && is_paste_of(&item.text, message, first.as_deref())
            })
            .map(|item| item.id.to_string()))
    }

    /// The record of `session`, a conversation of a participant's harness other than the
    /// one it is on now, read with no window open: none where its harness is none an
    /// adapter opens, or the participant has no conversation.
    pub(crate) async fn record_left(
        &self,
        participant: i64,
        session: &str,
    ) -> Result<Option<Arc<Reading>>, EngineError> {
        let Some((harness, _)) = self.readable(participant)? else {
            return Ok(None);
        };
        Ok(Some(
            caught(
                self.seams
                    .records
                    .look(harness, session, &Options::default()),
            )
            .await,
        ))
    }

    /// Whether a participant's harness's record of its current conversation,
    /// read with no window open, ends in a turn with no end (the one the app's
    /// quit cut, or a Close), and none where it cannot tell: the record cannot,
    /// or there is none to read.
    pub(crate) async fn turn_cut(&self, participant: i64) -> Result<Option<bool>, EngineError> {
        let Some((harness, session)) = self.readable(participant)? else {
            return Ok(None);
        };
        Ok(self.seams.records.turn_cut(harness, &session).await)
    }

    /// The harness and the session a participant's current conversation is on,
    /// where its record can be read with no window open: none when it has no
    /// conversation, no session yet, or a harness no adapter opens.
    fn readable(&self, participant: i64) -> Result<Option<(Harness, String)>, EngineError> {
        let conversation = self
            .seams
            .ledger
            .borrow()
            .current_conversation(participant)?;
        let Some(conversation) = conversation else {
            return Ok(None);
        };
        let Some(session) = conversation
            .native_session
            .filter(|session| !session.is_empty())
        else {
            return Ok(None);
        };
        Ok(Harness::from_kind(&conversation.harness)
            .filter(|_| self.seams.adapters.adapter(&conversation.harness).is_some())
            .map(|harness| (harness, session)))
    }
}

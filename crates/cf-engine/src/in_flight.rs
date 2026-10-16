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
    /// set of a carrier on its way when it is opened).
    pub(crate) async fn settle_in_flight(self: &Rc<Self>) -> Result<(), EngineError> {
        // No door lives through a start: what one claimed is the ledger's again.
        self.seams.ledger.borrow_mut().release_all_claims()?;
        let messages = self.seams.ledger.borrow().in_flight()?;
        for message in messages {
            match self.arrived(&message).await? {
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
        }
        Ok(())
    }

    /// The paste `delivering` was on its way into a window that went: its program
    /// ended, or the window closed while its harness took it. One that arrived
    /// ([`Dispatcher::arrived`]) is confirmed; any other is settled by `otherwise`, as
    /// it was before the window's record was asked. A launch's first message, and what
    /// a chief's launch put ahead of its queue, are `otherwise`'s alone: the window
    /// they began goes on with them by their launch's own rules. The message is read
    /// again once the record is: one that is on its way no more (its task cancelled
    /// meanwhile, or taken back) is left as it is, and the window's end goes on (a
    /// review of 2026-10-16: a confirm of a message cancelled meanwhile failed, and the
    /// chief's exit with it, which left the project open and its windows running).
    /// Whether it read the record: what the participant is may have moved on
    /// meanwhile ([`Dispatcher::still_as_it_went`]).
    pub(crate) async fn settle_gone(
        self: &Rc<Self>,
        delivering: Delivering,
        otherwise: impl FnOnce(Delivering) -> Result<(), EngineError>,
    ) -> Result<bool, EngineError> {
        if delivering.launch || delivering.chief {
            otherwise(delivering)?;
            return Ok(false);
        }
        let Some(message) = self.still_delivering(delivering.message)? else {
            return Ok(false);
        };
        let arrived = self.arrived(&message).await?;
        let Some(message) = self.still_delivering(message.id)? else {
            return Ok(true);
        };
        match arrived {
            Some(item) => {
                self.confirm_delivered(&message, &json!({ "item": item }))?;
                self.changed();
            }
            None => otherwise(delivering)?,
        }
        Ok(true)
    }

    /// The message `id` as the ledger has it, while it is on its way.
    fn still_delivering(&self, id: i64) -> Result<Option<MessageView>, EngineError> {
        let message = self.seams.ledger.borrow().message(id)?;
        Ok(message.filter(|found| found.state == "delivering"))
    }

    /// The conversation a participant is on, which the record of a window that
    /// went is read for.
    pub(crate) fn conversation_on(&self, participant: i64) -> Result<Option<i64>, EngineError> {
        let conversation = self
            .seams
            .ledger
            .borrow()
            .current_conversation(participant)?;
        Ok(conversation.map(|conversation| conversation.id))
    }

    /// Whether the participant of a window that went is still as the window left it
    /// once its record was read with no window open: kept, with no window opened for
    /// it since, nor one being launched, which a reopening is from before it asks
    /// its harness (`launching`), and on the conversation `on` it was on. Meanwhile
    /// the human may have switched the chief, whose project the old window's exit
    /// then suspended, its workers' windows closed, or opened a session's window
    /// again, whose task the old one's exit then paused (the reviews of 2026-10-16).
    pub(crate) fn still_as_it_went(
        &self,
        record: &Rc<Record>,
        on: Option<i64>,
    ) -> Result<bool, EngineError> {
        let opened = {
            let window = record.window.borrow();
            window.pane.is_some() || window.opening.is_some() || window.launching
        };
        if opened || self.forgotten(record) {
            return Ok(false);
        }
        Ok(self.conversation_on(record.id)? == on)
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

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
//! show goes back to its queue, as it always did. Where, at a start, the harness's own
//! store says its window took it, it is sent again saying so in its header
//! ([`SENT_AGAIN`]): Devin holds a paste queued behind a turn, and writes it to its record
//! only when the turn takes it in, and its window died with the app's quit, most likely
//! losing what its queue held. Its word is that the TUI took the paste, not that it
//! holds it still, and a message confirmed and lost is a result nobody reads; one read
//! twice is seen as such (the chief's call on T-280). A harness that keeps no such word
//! (all but Devin) cannot tell a paste it held queued from one it never had: what its
//! record does not show at the start is sent again with no word.
//!
//! [`SENT_AGAIN`]: crate::delivery_text::SENT_AGAIN
//!
//! And the turn a participant's record ends in, read the same way: whether the quit
//! or a Close cut it.

use std::rc::Rc;
use std::sync::Arc;

use cf_harness::contract::TurnLeft;
use cf_harness::records::{Options, Reading, Role};
use cf_ledger::MessageView;
use cf_proto::agents::Harness;
use serde_json::json;

use crate::deliveries::Delivering;
use crate::delivery_text::{delivery_text, delivery_text_sent_again, is_paste_of, marker_of};
use crate::dispatcher::Dispatcher;
use crate::record::Record;
use crate::runtime::{caught, returning};
use crate::seams::EngineError;

/// Why a start gives back a paste whose record does not show it, though its window's
/// harness said the window took it: it goes again saying so ([`Dispatcher::delivery_text_of`]).
pub(crate) const TOOK_BEFORE_A_RESTART: &str =
    "the daemon stopped before its record showed it, and its window took it";

impl Dispatcher {
    /// How `message` reads in its window, with the rows it `carried`: as it always does
    /// ([`delivery_text`]), but for one a start gave back though its window took it
    /// ([`TOOK_BEFORE_A_RESTART`]), which says it is sent again
    /// ([`delivery_text_sent_again`]), whatever gave it back since (Resume after Close
    /// turns a paste away at its hand-over, its project closed): that first copy may be
    /// there still.
    pub(crate) fn delivery_text_of(
        &self,
        message: &MessageView,
        carried: &[MessageView],
    ) -> Result<String, EngineError> {
        let again = self
            .seams
            .ledger
            .borrow()
            .given_back_because(message.id, TOOK_BEFORE_A_RESTART)?;
        Ok(if again {
            delivery_text_sent_again(message, carried)
        } else {
            delivery_text(message, carried)
        })
    }

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

    /// One message on its way at the start, settled by what showed of it, in the
    /// conversation it was pasted into, which the ledger kept (the review of 6c8fc595:
    /// a paste a conversation the human left had run was read for in the new one, given
    /// back and pasted again). The message is read again once the record is: one that is
    /// on its way no more (its task cancelled meanwhile, as the human may while the start
    /// reads) is left as it is.
    async fn settle_one_in_flight(&self, message: &MessageView) -> Result<(), EngineError> {
        let pasted = self.seams.ledger.borrow().pasted_into(message.id)?;
        let arrived = self.arrived(message, pasted.as_deref()).await?;
        // One its harness took, which its record shows only once the turn ahead of it
        // takes it in: its window, gone with the daemon's stop, most likely lost it.
        let took = match arrived {
            Some(_) => false,
            None => self.took_with_no_window(message, pasted.as_deref()).await?,
        };
        let Some(message) = self.still_delivering(message.id)? else {
            return Ok(());
        };
        match arrived {
            None => {
                let reason = if took {
                    TOOK_BEFORE_A_RESTART
                } else {
                    "the daemon stopped before it arrived"
                };
                self.seams
                    .ledger
                    .borrow_mut()
                    .retry_delivery(message.id, reason, true)?;
            }
            Some(item) => {
                self.confirm_delivered(&message, &json!({ "item": item }))?;
            }
        }
        Ok(())
    }

    /// Whether the harness of `message`'s recipient says, with no window open, that its
    /// window took the paste in the conversation it went into (`pasted`, else the
    /// participant's current one) since this attempt began ([`Records::took`]): no
    /// where it cannot tell.
    ///
    /// [`Records::took`]: cf_harness::contract::Records::took
    async fn took_with_no_window(
        &self,
        message: &MessageView,
        pasted: Option<&str>,
    ) -> Result<bool, EngineError> {
        let Some((harness, current)) = self.readable(message.recipient_id)? else {
            return Ok(false);
        };
        let Some(begun) = self.seams.ledger.borrow().delivery_begun_ms(message.id)? else {
            return Ok(false);
        };
        let session = pasted.unwrap_or(&current);
        let took = self
            .seams
            .records
            .took(harness, session, &marker_of(message.id), begun)
            .await;
        Ok(took == Some(true))
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
        // The record of the conversation it was pasted into, which the window may
        // have been switched away from since (the review of c2e29f2d).
        let arrived = self
            .arrived(&message, delivering.session.as_deref())
            .await?;
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
    /// `pasted`, where known, is the conversation it was pasted into: its record is read
    /// where it is another than the participant's current conversation, else the current
    /// one's, as it always was.
    async fn arrived(
        &self,
        message: &MessageView,
        pasted: Option<&str>,
    ) -> Result<Option<String>, EngineError> {
        let current = self
            .seams
            .ledger
            .borrow()
            .current_conversation(message.recipient_id)?
            .and_then(|conversation| conversation.native_session);
        let pasted = pasted.filter(|pasted| current.as_deref() != Some(*pasted));
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
            None => match pasted {
                // Boxed, so that the work of an operation that settles it stays small.
                Some(session) => {
                    returning(Box::pin(self.recorded_paste_in(
                        message.recipient_id,
                        session,
                        message.id,
                        first,
                    )))
                    .await
                }
                None => {
                    returning(self.recorded_paste_of(message.recipient_id, message.id, first)).await
                }
            },
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
        Ok(paste_in(&reading, message, first.as_deref()))
    }

    /// The record of `session`, one of a participant's harness's conversations, read
    /// with no window open: none where its harness is none an adapter opens, or the
    /// participant has no conversation.
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

    /// The first item a participant's harness recorded it was given in `session`, one
    /// of its conversations, that is the paste of `message` ([`is_paste_of`]), read with
    /// no window open: none when none is, or when that record cannot be read.
    async fn recorded_paste_in(
        &self,
        participant: i64,
        session: &str,
        message: i64,
        first: Option<String>,
    ) -> Result<Option<String>, EngineError> {
        let reading = self.record_left(participant, session).await?;
        Ok(reading.and_then(|reading| paste_in(&reading, message, first.as_deref())))
    }

    /// Whether a participant's harness's record of its current conversation,
    /// read with no window open, ends in a turn with no end (the one the app's
    /// quit cut, or a Close), with the message that began it, or at rest; and none
    /// where it cannot tell: the record cannot, or there is none to read.
    pub(crate) async fn turn_cut(&self, participant: i64) -> Result<Option<TurnLeft>, EngineError> {
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

/// The first item of `reading`, a record, that is the paste of `message`
/// ([`is_paste_of`]), behind `first` where that is what its window was given ahead of
/// it: none when none is, or when the record could not be read.
fn paste_in(reading: &Reading, message: i64, first: Option<&str>) -> Option<String> {
    let Reading::Known(record) = reading else {
        return None;
    };
    record
        .items
        .iter()
        .find(|item| item.role == Role::User && is_paste_of(&item.text, message, first))
        .map(|item| item.id.to_string())
}

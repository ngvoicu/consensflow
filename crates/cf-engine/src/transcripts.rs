//! ConsensFlow's copy of each window's conversation: at every look, what is new
//! in the window's record goes into the ledger (the last item again, and from
//! the first when the record shrank); a window the human switched to another
//! conversation is followed there. It owns the record's count of what is
//! copied.

use cf_harness::contract::Observed;
use cf_ledger::ParticipantView;
use serde_json::Value;

use crate::dispatcher::Dispatcher;
use crate::record::Record;
use crate::seams::EngineError;

/// What the looks at a window copied: which conversation, and how many of
/// its items.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Copied {
    pub(crate) conversation: i64,
    pub(crate) count: usize,
}

impl Dispatcher {
    /// ConsensFlow's own copy of the window's conversation, readable on the
    /// card once the window is gone: each look copies what is new and the
    /// item still being written; a record that shrank (a resumed window
    /// rewrote it) is copied over from the start. A look that wrote anything
    /// says so ([`Dispatcher::wrote`]): a view of what the window did reads
    /// it again.
    pub(crate) fn copy(
        &self,
        participant: &ParticipantView,
        record: &Record,
        observed: &Observed,
    ) -> Result<(), EngineError> {
        let conversation = self
            .seams
            .ledger
            .borrow()
            .current_conversation(participant.id)?;
        let Some(conversation) = conversation else {
            return Ok(());
        };
        let items = observed.items();
        let copied = record
            .copied
            .borrow()
            .filter(|copied| copied.conversation == conversation.id)
            .map_or(0, |copied| copied.count);
        let from = if items.len() < copied {
            0
        } else {
            copied.saturating_sub(1)
        };
        if items.len() > from {
            let new: Vec<Value> = items[from..]
                .iter()
                .map(|item| serde_json::to_value(item).unwrap_or(Value::Null))
                .collect();
            let changed = self.seams.ledger.borrow_mut().copy_transcript(
                conversation.id,
                &new,
                i64::try_from(from).unwrap_or(i64::MAX),
            )?;
            if changed > 0 {
                self.wrote();
            }
        }
        *record.copied.borrow_mut() = Some(Copied {
            conversation: conversation.id,
            count: items.len(),
        });
        Ok(())
    }

    /// A window the human switched to another conversation (/clear, /new,
    /// /resume) is followed: the participant's conversation is the one it
    /// shows now, and its launch names it, so later looks, deliveries and
    /// the copy go there.
    pub(crate) fn follow(
        &self,
        participant: &ParticipantView,
        record: &Record,
        session: &str,
    ) -> Result<(), EngineError> {
        self.seams.ledger.borrow_mut().follow_conversation(
            participant.id,
            participant.harness.as_deref().unwrap_or_default(),
            session,
        )?;
        let window = record.window.borrow().window.clone();
        if let Some(window) = window {
            window.follow(session);
        }
        *record.copied.borrow_mut() = None;
        self.changed();
        Ok(())
    }
}

#[cfg(test)]
mod tests;

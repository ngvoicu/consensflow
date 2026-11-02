//! ConsensFlow's copy of each window's conversation: at every look, what is new
//! in the window's record goes into the ledger (the last item again, and from
//! the first when the record shrank); a window the human switched to another
//! conversation is followed there. It owns the record's count of what is
//! copied.

use cf_harness::contract::Observed;
use cf_harness::records::{Detail, Item};
use cf_ledger::ParticipantView;
use serde_json::{Map, Value};

use crate::dispatcher::Dispatcher;
use crate::record::Record;
use crate::seams::EngineError;
use crate::turn_end::Rests;

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
    /// rewrote it) is copied over from the start. What is copied is the record's
    /// timeline: its items and those only the history shows, each where it stands
    /// ([`Observed::timeline`]). A look that wrote anything
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
        let timeline = observed.timeline();
        let copied = record
            .copied
            .borrow()
            .filter(|copied| copied.conversation == conversation.id)
            .map_or(0, |copied| copied.count);
        let from = if timeline.len() < copied {
            0
        } else {
            copied.saturating_sub(1)
        };
        if timeline.len() > from {
            let new: Vec<Value> = timeline
                .from(from)
                .map(|(item, drawn_only)| to_copy(item, drawn_only))
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
            count: timeline.len(),
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
        let followed = self.seams.ledger.borrow_mut().follow_conversation(
            participant.id,
            participant.harness.as_deref().unwrap_or_default(),
            session,
        )?;
        // Its program writes to that one now: its end is kept there (`ownership`).
        record.window.borrow_mut().conversation = Some(followed.id);
        let window = record.window.borrow().window.clone();
        if let Some(window) = window {
            window.follow(session);
        }
        *record.copied.borrow_mut() = None;
        // Another conversation's record: what the last one lost is not this one's.
        {
            let mut delivery = record.delivery.borrow_mut();
            delivery.lost = 0;
            delivery.lost_before = None;
        }
        // The rests were the other conversation's: a monitor of this one was never seen
        // missing at them (Calliope on 136a7158).
        record.held.borrow_mut().rests = Rests::default();
        self.changed();
        Ok(())
    }
}

/// An item as the ledger copies it: as its reading writes it, and what a
/// reading never says: marked when it is a compaction's summary, with what the
/// history draws of it besides its words where its reader says more than its role
/// does (`detail`), and marked when only the history shows it (`drawnOnly`).
fn to_copy(item: &Item, drawn_only: bool) -> Value {
    let mut value = serde_json::to_value(item).unwrap_or(Value::Null);
    if let Value::Object(fields) = &mut value {
        if item.compaction {
            fields.insert("compaction".to_owned(), Value::Bool(true));
        }
        if let Some(detail) = &item.detail {
            fields.insert("detail".to_owned(), detail_of(detail));
        }
        if drawn_only {
            fields.insert("drawnOnly".to_owned(), Value::Bool(true));
        }
    }
    value
}

/// What the history draws of an item, as the ledger takes it: each field its reader says.
fn detail_of(detail: &Detail) -> Value {
    let mut fields = Map::new();
    fields.insert("kind".to_owned(), Value::from(detail.kind.word()));
    let mut say = |name: &str, value: Option<Value>| {
        if let Some(value) = value {
            fields.insert(name.to_owned(), value);
        }
    };
    say("name", detail.name.as_deref().map(Value::from));
    say("header", detail.header.as_deref().map(Value::from));
    say(
        "status",
        detail.status.map(|status| Value::from(status.word())),
    );
    say("tookMs", detail.took_ms.map(Value::from));
    say("tokens", detail.tokens.map(Value::from));
    say("diff", detail.diff.as_deref().map(Value::from));
    Value::Object(fields)
}

#[cfg(test)]
mod tests;

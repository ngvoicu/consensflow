//! A turn the window's old process left open.
//!
//! The quit ends every window's process with its turn at work, and the window
//! that is opened again on that conversation finds a record whose last turn
//! has no end: the user's message with no answer, or a tool's call with no
//! result, or a result with nothing after it. A record can also say nothing of
//! the end of a turn that did end: a final answer its harness never marked as the
//! last (Devin's GPT models store none, and only the old launch's wire log said it
//! ended), a turn that failed. No process is in that turn any more, so it is over,
//! whatever the record says. What a harness says of the window itself is the word
//! on whether it has begun a turn of its own (Claude: its status reads idle, at its
//! prompt, and it writes nothing of the old turn until a message comes, which it
//! answers as any).
//!
//! What tells the old turn from one the window began is what the record held
//! when the window was opened: as long as it holds no message of the user's
//! more than that, the open turn is the old one's. A window begins a turn only
//! by a message, which its record shows first, as an item of the user's; what
//! the old process writes after the open (the rows of its last step, its
//! harness's own notes: Devin's agent outlives its window by a second or two)
//! is no turn of this window's. A window opened with a message of its own has
//! no old turn to speak of, since the message is its turn, which is the
//! dispatcher's to watch arrive.

use std::sync::Arc;

use crate::records::{Item, Reading, Role, Settlement};

/// What a record held when a window was opened again on it: how many messages of the
/// user's it had, and the last of them. Nothing but a message of the user's begins a
/// turn of the window's own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LeftOpen {
    messages: usize,
    last: Option<Arc<str>>,
}

impl LeftOpen {
    /// What `reading`, a look taken before the window was opened, holds, whatever it
    /// says of the turn: none where the record cannot be read, or holds nothing.
    pub(crate) fn held(reading: &Reading) -> Option<Self> {
        let Reading::Known(record) = reading else {
            return None;
        };
        if record.items.is_empty() {
            return None;
        }
        Some(Self::of_items(&record.items))
    }

    /// The turn `reading`, a look taken before the window was opened, has not
    /// ended: in flight, or one whose end the record cannot say. None where the
    /// record says its turn is over, or cannot be read, or holds nothing.
    pub(crate) fn of(reading: &Reading) -> Option<Self> {
        match reading {
            Reading::Known(record) if record.settlement != Settlement::Settled => {
                Self::held(reading)
            }
            _ => None,
        }
    }

    fn of_items(items: &[Item]) -> Self {
        let mut users = items.iter().filter(|item| item.role == Role::User);
        let messages = users.clone().count();
        Self {
            messages,
            last: users.next_back().map(|item| Arc::clone(&item.id)),
        }
    }

    /// Whether `reading` holds no message of the user's more than the record did when
    /// the window was opened: none was given to the window since, and it began no turn.
    pub(crate) fn unchanged(&self, reading: &Reading) -> bool {
        let Reading::Known(record) = reading else {
            return false;
        };
        *self == Self::of_items(&record.items)
    }

    /// Whether `reading` shows that turn still not ended and nothing begun after
    /// it.
    pub(crate) fn still(&self, reading: &Reading) -> bool {
        let Reading::Known(record) = reading else {
            return false;
        };
        record.settlement != Settlement::Settled && self.unchanged(reading)
    }
}

#[cfg(test)]
mod tests;

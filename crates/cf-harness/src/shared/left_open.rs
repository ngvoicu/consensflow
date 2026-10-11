//! A turn the app's quit left open.
//!
//! The quit ends every window's process with its turn at work, and the window
//! that is opened again on that conversation finds a record whose last turn
//! has no end: the user's message with no answer, or a tool's call with no
//! result, or a result with nothing after it. No process is in that turn any
//! more, so it is over, whatever the record says. What a harness says of the
//! window itself is the word on whether it has begun a turn of its own (Claude:
//! its status reads idle, at its prompt, and it writes nothing of the old turn
//! until a message comes, which it answers as any).
//!
//! What tells the old turn from one the window began is what the record held
//! when the window was opened: as long as it holds no more than that, the open
//! turn is the old one's. A window opened with a message of its own has no old
//! turn to speak of, since the message is its turn, which is the dispatcher's
//! to watch arrive.

use std::sync::Arc;

use crate::records::{Reading, Settlement};

/// The turn a record had open when a window was opened again on it: the number
/// of its items and the last of them, which is all there was of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LeftOpen {
    items: usize,
    last: Arc<str>,
}

impl LeftOpen {
    /// The turn `reading`, a look taken before the window was opened, has in
    /// flight: none where the record says its turn is over, or cannot be
    /// read, or holds nothing.
    pub(crate) fn of(reading: &Reading) -> Option<Self> {
        let Reading::Known(record) = reading else {
            return None;
        };
        if record.settlement != Settlement::InFlight {
            return None;
        }
        let last = record.items.last()?;
        Some(Self {
            items: record.items.len(),
            last: Arc::clone(&last.id),
        })
    }

    /// Whether `reading` shows that turn still in flight and nothing after
    /// it: no message was given to the window since, and it began no turn.
    pub(crate) fn still(&self, reading: &Reading) -> bool {
        let Reading::Known(record) = reading else {
            return false;
        };
        record.settlement == Settlement::InFlight
            && record.items.len() == self.items
            && record.items.last().is_some_and(|last| last.id == self.last)
    }
}

#[cfg(test)]
mod tests;

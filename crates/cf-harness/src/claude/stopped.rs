//! A turn the daemon stopped before Claude wrote anything of it.
//!
//! The daemon interrupts a window at work: Escape into its pane. Claude,
//! interrupted in the first seconds of a turn, before a word of its answer,
//! writes no record of it. The transcript ends on the user's message (the
//! brief, the words that resume a task), Claude's status reads idle, and
//! Claude puts the message it was given back into its input box. Read as it
//! always was, the turn is in flight for ever, and the window never reads at
//! rest.
//!
//! Claude's status cannot tell that window from one whose turn has not begun:
//! it reads idle for as long as the hooks of a prompt run, which is seconds,
//! before the turn goes busy. So the window is read at rest only where the
//! daemon pressed the interrupt for the very turn the reading shows, and it
//! has read idle, with nothing written of the turn, for a moment since: a
//! turn just pasted has no press, and an old press is for the turn before it.

use std::sync::Arc;

use crate::records::{Reading, Record, Role, Settlement};

/// How long after the last press the window must have read idle, with
/// nothing written, before the press counts as what stopped it: a turn that
/// was about to begin is busy by then.
const HOLD_MS: i64 = 1_000;

/// The keys that clear the text Claude put back into its input box before
/// the next message is pasted: a paste goes in where the cursor is, and
/// would be sent with the old message in front of it. Ctrl+C, once: it clears
/// the whole text, where Ctrl+U clears the cursor's line and leaves the header
/// line of a two-line message, and it does no harm to an empty box, where a
/// second Escape opens Claude's rewind and swallows what is pasted next
/// (`npm run live:input-box`, Claude 2.1.292). Claude says "Press Ctrl-C
/// again to exit" for a moment, and the paste that follows cancels it.
pub(super) const CLEAR_INPUT: [u8; 1] = [0x03];

/// An interrupt the daemon pressed: when (the wall clock, in milliseconds),
/// and over which message of the user's, the one that began the turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Pressed {
    at: i64,
    over: Arc<str>,
}

impl Pressed {
    pub(super) fn new(at: i64, over: Arc<str>) -> Self {
        Self { at, over }
    }

    /// Whether this press is what ended the turn `record` shows, at `now`: it
    /// is the turn the press was over, still in flight, and Claude wrote
    /// nothing of it (a hook's context aside) in the moment since.
    pub(super) fn stopped(&self, record: &Record, now: i64) -> bool {
        now - self.at >= HOLD_MS
            && record.settlement == Settlement::InFlight
            && unanswered(record, &self.over)
    }
}

/// The id of the user's last message in what a look read: the turn that is
/// the window's now.
pub(super) fn last_user(reading: &Reading) -> Option<Arc<str>> {
    let Reading::Known(record) = reading else {
        return None;
    };
    let at = record
        .items
        .iter()
        .rposition(|item| item.role == Role::User)?;
    Some(Arc::clone(&record.items[at].id))
}

/// Whether the user's last message is `over`, and nothing but a hook's
/// context follows it: no word of the assistant's, no tool's output.
fn unanswered(record: &Record, over: &str) -> bool {
    let Some(at) = record
        .items
        .iter()
        .rposition(|item| item.role == Role::User)
    else {
        return false;
    };
    &*record.items[at].id == over
        && record.items[at + 1..]
            .iter()
            .all(|item| item.role == Role::Custom)
}

#[cfg(test)]
mod tests;

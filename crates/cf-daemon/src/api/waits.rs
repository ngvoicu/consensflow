//! What a window reads of the board while its project is closed (the human's
//! Close, or the chief's window ending): the text of a message no window has been
//! handed yet is not handed through a read either. It waits for the Resume, as the
//! paste and the question door hold it, and the reader is told so in its place
//! (both reviews of 2026-10-16, round 6: `cf inbox read` and `cf task get` gave a
//! window the Close could not close the answer the door held for the Resume, and
//! `cf` then said it received it). Only a message still `queued` is held: one
//! being pasted, or claimed by a door, is a hand-over begun, the window's whatever
//! came since, and a receipt of it is taken as ever. What the reader wrote itself
//! is its own to read.

use cf_ledger::MessageView;
use serde_json::Value;

use super::answer::Failure;
use super::callers::Caller;
use super::context::Context;
use super::views::MessageSummary;

/// What a summary of a message whose text waits says in place of its first line.
pub const WAITS: &str = "(this waits until the human resumes the project)";

/// What of a message a window of a closed project may read, as it stands now.
pub struct Waits<'a> {
    /// The reader's project is closed.
    closed: bool,
    /// The reader's handle.
    reader: &'a str,
}

impl<'a> Waits<'a> {
    /// For `caller`, its project as the ledger has it now.
    pub fn now(context: &Context, caller: &'a Caller) -> Result<Self, Failure> {
        let project = context.ledger.borrow().project(caller.project.id)?;
        Ok(Self {
            closed: project.is_none_or(|project| project.state != "open"),
            reader: &caller.participant.handle,
        })
    }

    /// Whether the text of `message` waits for the Resume: its project is closed,
    /// no window was handed it, and the reader did not write it.
    pub fn holds(&self, message: &MessageView) -> bool {
        self.closed && message.state == "queued" && message.sender.as_deref() != Some(self.reader)
    }

    /// A summary of `message` for the reader: its first line, its questions and
    /// its choices left out where its text waits, and [`WAITS`] said instead.
    pub fn summary(&self, message: &MessageView) -> MessageSummary {
        let mut summary = MessageSummary::from(message);
        if self.holds(message) {
            summary.preview = WAITS.to_owned();
            summary.questions = Value::Null;
            summary.choices = Value::Null;
        }
        summary
    }

    /// `message` for a reader that reads its gist (the chief's history): its body
    /// [`WAITS`] where its text waits.
    pub fn shown(&self, message: MessageView) -> MessageView {
        if self.holds(&message) {
            return MessageView {
                body: WAITS.to_owned(),
                questions: Value::Null,
                choices: Value::Null,
                ..message
            };
        }
        message
    }
}

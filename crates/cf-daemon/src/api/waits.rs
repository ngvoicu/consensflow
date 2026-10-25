//! What a window reads of the board while its project is closed (the human's
//! Close, or the chief's window ending): the text of a message no window has been
//! handed yet is not handed through a read either. It waits for the Resume, as the
//! paste and the question door hold it, and the reader is told so in its place
//! (both reviews of 2026-10-16, round 6: `cf inbox read` and `cf task get` gave a
//! window the Close could not close the answer the door held for the Resume, and
//! `cf` then said it received it). A message still to be given is held until its
//! hand-over has begun at its window: let go by its transport (the paste asked
//! for, the rename made, the request given to its connection, the command line a
//! window opens with; the ledger's `admit`), or claimed by a question's door.
//! One whose delivery is only begun in the ledger is held too: what is still to
//! come of it may never come (Sol's review of 2026-10-16, round 7: a delivery begun
//! before the Close, and refused at its hand-over after it, was read whole). A
//! receipt of a hand-over begun is taken as ever, and what the reader wrote itself
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
    /// The messages of the project whose hand-over has begun at their window,
    /// while it is closed.
    handed: Vec<i64>,
}

impl<'a> Waits<'a> {
    /// For `caller`, its project as the ledger has it now.
    pub fn now(context: &Context, caller: &'a Caller) -> Result<Self, Failure> {
        let ledger = context.ledger.borrow();
        let project = ledger.project(caller.project.id)?;
        let closed = project.is_none_or(|project| project.state != "open");
        let handed = if closed {
            ledger.handed_over(caller.project.id)?
        } else {
            Vec::new()
        };
        Ok(Self {
            closed,
            reader: &caller.participant.handle,
            handed,
        })
    }

    /// Whether the reader's project is closed: only then does a text wait for the
    /// Resume.
    pub fn closed(&self) -> bool {
        self.closed
    }

    /// Whether the text of `message` waits for the Resume: its project is closed,
    /// it is still to be given (queued, or on its way) and no window was handed it,
    /// and the reader did not write it.
    pub fn holds(&self, message: &MessageView) -> bool {
        self.closed
            && matches!(message.state.as_str(), "queued" | "delivering")
            && !self.handed.contains(&message.id)
            && message.sender.as_deref() != Some(self.reader)
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

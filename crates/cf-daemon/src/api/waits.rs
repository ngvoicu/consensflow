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
//!
//! So too what a meeting with the chief holds, while the project is open (3.0.7
//! chief-meeting, CMT-14): what is queued for the chief and the meeting holds
//! (the ledger's `held_by_meeting`) is not the chief's to read until the meeting
//! ends, as it is not typed into its window; the chief is told it waits for that.
//!
//! Where a read lists or shows such a message it says so in its place, in one
//! sentence ([`Until::note`], and [`Until::refusal`] for `cf inbox read`): a task's
//! thread and the chief's history carry the note as the message's body, where before
//! a thread left the message out and said nothing (T-942).

use cf_ledger::MessageView;
use serde_json::Value;

use super::answer::Failure;
use super::callers::Caller;
use super::context::Context;
use super::views::MessageSummary;

/// What a summary of a message whose text waits says in place of its first line.
pub const WAITS: &str = "(this waits until the human resumes the project)";

/// What a summary of a message a meeting holds says in place of its first line.
pub const MEETING_WAITS: &str = "(this waits until your meeting with the human ends)";

/// What a message's text waits for, where it waits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Until {
    /// The human's Resume of the closed project.
    Resume,
    /// The end of the human's meeting with the chief.
    MeetingEnds,
}

impl Until {
    /// What a summary says in place of the text's first line.
    pub fn preview(self) -> &'static str {
        match self {
            Self::Resume => WAITS,
            Self::MeetingEnds => MEETING_WAITS,
        }
    }

    /// What is said of message `id`, its text kept back: that it waits, for what, and
    /// that it comes. The refusal and the note a thread has in its place are this
    /// one sentence.
    fn said_of(self, id: i64) -> String {
        let until = match self {
            Self::Resume => "until the human resumes the project",
            Self::MeetingEnds => "until your meeting with the human ends",
        };
        format!("m-{id} waits {until}: it comes to you then")
    }

    /// What the reader is told message `id` waits for, refused its text.
    pub fn refusal(self, id: i64) -> String {
        format!("{}. Do not ask for it again now.", self.said_of(id))
    }

    /// The line that stands in a thread or a history where message `id` is, its text
    /// being kept back.
    pub fn note(self, id: i64) -> String {
        format!("({})", self.said_of(id))
    }
}

/// What of a message a window may read, as it stands now: a window of a closed
/// project, and the chief in a meeting.
pub struct Waits<'a> {
    /// The reader's project is closed.
    closed: bool,
    /// The reader's handle.
    reader: &'a str,
    /// The messages of the project whose hand-over has begun at their window,
    /// while it is closed.
    handed: Vec<i64>,
    /// What the reader's meeting with the human holds, the reader being the chief
    /// of an open project in a meeting.
    meeting: Vec<i64>,
}

impl<'a> Waits<'a> {
    /// For `caller`, its project as the ledger has it now.
    pub fn now(context: &Context, caller: &'a Caller) -> Result<Self, Failure> {
        let ledger = context.ledger.borrow();
        let project = ledger.project(caller.project.id)?;
        let closed = project
            .as_ref()
            .is_none_or(|project| project.state != "open");
        let handed = if closed {
            ledger.handed_over(caller.project.id)?
        } else {
            Vec::new()
        };
        let meeting = if !closed
            && caller.participant.role == "chief"
            && project.is_some_and(|project| project.meeting.is_some())
        {
            ledger
                .held_by_meeting(caller.project.id)?
                .iter()
                .map(|message| message.id)
                .collect()
        } else {
            Vec::new()
        };
        Ok(Self {
            closed,
            reader: &caller.participant.handle,
            handed,
            meeting,
        })
    }

    /// Whether the reader's project is closed: only then does a text wait for the
    /// Resume.
    pub fn closed(&self) -> bool {
        self.closed
    }

    /// What the text of `message` waits for, if it waits: the Resume where its
    /// project is closed, it is still to be given (queued, or on its way) and no
    /// window was handed it; the end of the meeting where the reader's meeting
    /// holds it. Never where the reader wrote it.
    pub fn until(&self, message: &MessageView) -> Option<Until> {
        if message.sender.as_deref() == Some(self.reader) {
            return None;
        }
        if self.closed
            && matches!(message.state.as_str(), "queued" | "delivering")
            && !self.handed.contains(&message.id)
        {
            return Some(Until::Resume);
        }
        self.meeting
            .contains(&message.id)
            .then_some(Until::MeetingEnds)
    }

    /// Whether the text of `message` waits ([`Waits::until`]).
    pub fn holds(&self, message: &MessageView) -> bool {
        self.until(message).is_some()
    }

    /// A summary of `message` for the reader: its first line, its questions and
    /// its choices left out where its text waits, and what it waits for said instead.
    pub fn summary(&self, message: &MessageView) -> MessageSummary {
        let mut summary = MessageSummary::from(message);
        if let Some(until) = self.until(message) {
            until.preview().clone_into(&mut summary.preview);
            summary.questions = Value::Null;
            summary.choices = Value::Null;
        }
        summary
    }

    /// `message` for a reader that reads its gist (the chief's history): its body the
    /// line that says it waits, where its text waits.
    pub fn shown(&self, message: MessageView) -> MessageView {
        match self.until(&message) {
            Some(until) => Self::in_words_of(until, message),
            None => message,
        }
    }

    /// `message` as a task's thread has it for this reader: as it is, or, where its
    /// text waits, in a line that says so ([`Waits::shown`]). Two kinds are left out
    /// of the thread as they were before it said anything, because a line in their
    /// place would do harm: `cf task get` takes a queued answer it printed for
    /// received (`waiting_answers`), and it prints no brief under the head once the
    /// thread has a task message for the window that holds it (`brief_under_head`;
    /// that a brief waits is said in the task's title, `brief_waits` of the task route).
    pub fn in_thread(&self, message: MessageView) -> Option<MessageView> {
        match self.until(&message) {
            None => Some(message),
            Some(_) if matches!(message.kind.as_str(), "answer" | "task") => None,
            Some(until) => Some(Self::in_words_of(until, message)),
        }
    }

    /// `message` with the line that says it waits for `until` in the place of its words.
    fn in_words_of(until: Until, message: MessageView) -> MessageView {
        MessageView {
            body: until.note(message.id),
            questions: Value::Null,
            choices: Value::Null,
            ..message
        }
    }
}

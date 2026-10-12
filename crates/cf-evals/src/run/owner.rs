//! The owner of a run, played by a script: what it does next, given how things are.
//! The run looks at the project every few seconds ([`Seen`]) and asks the owner for
//! its next step ([`Owner::step`]); the step is the owner's alone, so the whole of
//! what the owner does is here, and tested without a harness.
//!
//! The owner answers in the chief's terminal, the same way in every arm:
//!
//! - a chief's own question dialog holds its window, waiting for input: the owner
//!   answers it there ([`Step::Dialog`]) once the board has been quiet a while;
//! - a chief that asked in its terminal hears the owner there once the board is
//!   quiet: its newest question since the owner last typed, answered from the
//!   scenario's answers, once ([`Step::Reply`]);
//! - a chief that stops in its terminal without asking anything, with nothing put on
//!   the board, hears the owner there too (twice at most), so the run sees what it
//!   does next ([`Step::Nudge`]); the report counts these nudges apart from the
//!   answers to its questions;
//! - a conversation's follow-ups are typed in turn, each after the chief is done
//!   with the last ([`Step::Next`]);
//! - the run is over once nothing has moved for the scenario's quiet time
//!   ([`Step::Over`]), or when time is up ([`Step::TimeUp`]).

use std::collections::{HashSet, VecDeque};
use std::time::Duration;

use crate::measure::OpenQuestion;
use crate::scenarios::{FollowUp, Scenario};
use crate::Result;

/// How long the chief and the board stay quiet before the owner sends the next
/// message of a conversation, or answers what the chief asked.
pub const FOLLOW_UP_AFTER: Duration = Duration::from_secs(20);

/// How long a chief's question dialog holds its window before the owner answers it.
pub const PICKER_AFTER: Duration = Duration::from_secs(10);

/// How long a chief may sit idle with nothing on the board before the owner answers
/// it in its terminal.
pub const NUDGE_AFTER: Duration = Duration::from_secs(60);

/// How long the owner waits after each key in a dialog, and after each answer, for
/// the window to draw it.
pub const KEY_PAUSE: Duration = Duration::from_millis(600);
pub const QUESTION_PAUSE: Duration = Duration::from_secs(2);

/// How many times at most the owner answers questions a chief left in its terminal,
/// or in its dialog.
pub const MAX_ANSWERS: i64 = 8;

/// How many times at most the owner nudges a chief that stopped without asking.
pub const MAX_NUDGES: i64 = 2;

/// How things are at one look at the project.
#[derive(Debug, Clone, Copy)]
pub struct Seen {
    /// How long the run has been going.
    pub elapsed: Duration,
    /// How long nothing has changed: the chief's state, the tasks', the windows'.
    pub quiet: Duration,
    /// Whether the chief is at work, or a task is.
    pub busy: bool,
    /// Whether the chief's window waits on a dialog of its own harness's.
    pub dialog: bool,
    /// Whether nothing is on the board.
    pub board_empty: bool,
}

/// What the owner does next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Answer the chief's own question dialog, in its window.
    Dialog,
    /// Answer the question the chief left in its terminal.
    Reply(OpenQuestion),
    /// Type this, because the chief stopped without asking anything.
    Nudge(String),
    /// Say the next thing of the conversation.
    Next(FollowUp),
    /// The run is over: nothing has moved for the scenario's quiet time.
    Over,
    /// The run is over: time is up.
    TimeUp,
    /// Nothing yet.
    Wait,
}

/// The owner at a point of a run.
#[derive(Debug)]
pub struct Owner {
    follow_ups: VecDeque<FollowUp>,
    replied_to: HashSet<String>,
    /// How many times it nudged the chief.
    pub nudges: i64,
    /// How many questions of the chief's, in its terminal, it answered.
    pub replies: i64,
    /// How many times it answered the chief's question dialog.
    pub dialogs: i64,
    /// How many follow-up messages it typed.
    pub follow_ups_sent: i64,
}

impl Owner {
    /// The owner of `scenario`, who has said nothing yet.
    pub fn new(scenario: &Scenario) -> Self {
        Self {
            follow_ups: scenario.follow_ups.iter().cloned().collect(),
            replied_to: HashSet::new(),
            nudges: 0,
            replies: 0,
            dialogs: 0,
            follow_ups_sent: 0,
        }
    }

    /// The owner's next step. `question` is the chief's open question in its
    /// terminal, which the owner asks for only where it could answer it: it reads
    /// the ledger. What a step is for is counted when it is given.
    pub fn step(
        &mut self,
        scenario: &Scenario,
        seen: &Seen,
        timeout: Duration,
        question: &mut dyn FnMut() -> Result<Option<OpenQuestion>>,
    ) -> Result<Step> {
        if seen.dialog && self.dialogs < MAX_ANSWERS && seen.quiet > PICKER_AFTER {
            self.dialogs += 1;
            return Ok(Step::Dialog);
        }
        if !seen.busy && self.replies < MAX_ANSWERS && seen.quiet > FOLLOW_UP_AFTER {
            if let Some(end) = question()? {
                if self.replied_to.insert(end.id.clone()) {
                    self.replies += 1;
                    return Ok(Step::Reply(end));
                }
            }
        }
        if let Some(nudge) = scenario.nudge {
            if self.nudges < MAX_NUDGES
                && self.replies == 0
                && self.dialogs == 0
                && !seen.busy
                && seen.board_empty
                && seen.quiet > NUDGE_AFTER
            {
                self.nudges += 1;
                return Ok(Step::Nudge(nudge.to_owned()));
            }
        }
        if !self.follow_ups.is_empty() && !seen.busy && seen.quiet > FOLLOW_UP_AFTER {
            if let Some(next) = self.follow_ups.pop_front() {
                if matches!(next, FollowUp::Say(_)) {
                    self.follow_ups_sent += 1;
                }
                return Ok(Step::Next(next));
            }
        }
        if self.follow_ups.is_empty() && !seen.busy && seen.quiet > scenario.quiet {
            return Ok(Step::Over);
        }
        if seen.elapsed > timeout {
            return Ok(Step::TimeUp);
        }
        Ok(Step::Wait)
    }
}

#[cfg(test)]
mod tests;

//! A worker's window has one task: the one its first task message was about (turn-end's
//! smallest rule, the chief's decision of 2026-10-21, on the advisor's answer of T-8).
//!
//! A task's turn may leave work running that its harness wakes the agent for (a job, a
//! monitor, a background agent: `turn_end`), and a task may be released from its window
//! while that work runs: by its result, by Finish, by a cancel, a failure, `task done` or
//! a reassign. The next task's message must not go into that window: the earlier work's
//! end would wake a turn there, and that turn's words would be taken for the later task's
//! result (both reviews of round 7: a cancel let T2 in, and T1's job's end replaced T2's
//! answer). So the rule is kept where a message goes in, not where a task is released,
//! and holds whatever released it and whatever is known of what still runs: **no message
//! of another task goes into a window that a task's message went into**
//! ([`Dispatcher::barred`]). The window is closed first, as a free worker's window is
//! after its result, and the message opens a window of its own ([`Dispatcher::keep_apart`]);
//! no look at what still runs lets it in earlier (clearance-based reuse is not 3.0.6's).
//! The message waits, and whoever gave its task is told once, where the window stays
//! open: the human keeps it open (Show), its close was refused, or its task is still in
//! the session's hands (paused, whose window stays for the resumption). A pause and a
//! resume of the same task keep its window.
//!
//! The window that opens for the other task goes on with the session's conversation only
//! where the host proved the program of the window before it gone (`last_exit`): Claude
//! Code's record reader then takes what the old process queued and started as gone with
//! it (`Launch::previous_gone`). Where it did not prove it (the window's exit said nothing,
//! or said it ran on, or this daemon never saw it close: a restart), the other task opens
//! a new conversation, as a session's first task does, and whoever gave it is told once
//! that it does not go on in the earlier task's ([`Dispatcher::kept_apart`]).
//!
//! The window knows its task by the launch that opened it (its first message's task, the
//! message a role said first displaced among them, or the task its conversation was last
//! given where it goes on with one) and by the first task message that goes into it
//! (`WindowPart::owner`); a window that knows none takes the first that comes. A chief's
//! window is every task's, and has none.

use std::rc::Rc;

use cf_ledger::{ConversationView, MessageView, NewNote, ParticipantView, ProjectView, TaskThread};

use crate::dispatcher::Dispatcher;
use crate::record::Record;
use crate::seams::EngineError;

/// The states of a task that a session it is given to still has in hand, whose window
/// stays for it.
const IN_HAND: [&str; 4] = ["queued", "working", "waiting", "paused"];

impl Dispatcher {
    /// The participant of a record, as the ledger has it now: none for one forgotten.
    pub(crate) fn participant_of(
        &self,
        record: &Record,
    ) -> Result<Option<ParticipantView>, EngineError> {
        Ok(self.project_of(record.id)?.and_then(|project| {
            project
                .participants
                .into_iter()
                .find(|participant| participant.id == record.id)
        }))
    }

    /// The task of the window of `participant` that `message` is not about, where the
    /// window has one and the message is another task's: it does not go in.
    pub(crate) fn barred(
        &self,
        participant: &ParticipantView,
        record: &Record,
        message: &MessageView,
    ) -> Option<i64> {
        if participant.role == "chief" {
            return None;
        }
        let number = message.task_number?;
        let owner = record.window.borrow().owner?;
        (owner != number).then_some(owner)
    }

    /// The window holds the work of T-`owner`, and `message`, another task's, is next for
    /// it: the window closes, unless it is kept, and the message waits for the window that
    /// opens after; where it is kept, whoever gave the message's task is told once why it
    /// waits.
    pub(crate) async fn keep_apart(
        self: &Rc<Self>,
        project: &ProjectView,
        participant: &ParticipantView,
        record: &Rc<Record>,
        message: &MessageView,
        owner: i64,
    ) -> Result<(), EngineError> {
        // Still in this session's hands: given to it, and not done with (Calliope on
        // 0eec91d2: a task another session took is no longer this window's to keep).
        let still = self
            .seams
            .ledger
            .borrow()
            .task(project.id, owner)?
            .filter(|thread| {
                IN_HAND.contains(&thread.task.state.as_str())
                    && thread.task.assignee.as_deref() == Some(participant.handle.as_str())
            });
        let (pinned, hidden) = {
            let window = record.window.borrow();
            (window.pinned, window.hidden)
        };
        let why = if let Some(thread) = still {
            format!("T-{owner} is still in its hands ({})", thread.task.state)
        } else if pinned && !hidden {
            "the human keeps it open: Hide it, and it closes once its turn is over".to_owned()
        } else if self.retire(record).await? {
            return Ok(());
        } else {
            "it would not close: ConsensFlow tries again".to_owned()
        };
        let Some(number) = message.task_number else {
            return Ok(());
        };
        if record.window.borrow().told_waiting == Some(number) {
            return Ok(());
        }
        record.window.borrow_mut().told_waiting = Some(number);
        let Some(waiting) = self.seams.ledger.borrow().task(project.id, number)? else {
            return Ok(());
        };
        self.seams.ledger.borrow_mut().note(
            project.id,
            &NewNote {
                from: None,
                to: waiting.task.requester.clone(),
                task: Some(number),
                body: format!(
                    "T-{number} waits for a window of its own: @{}'s window worked on T-{owner}, and no other task goes into it, as what T-{owner} left running may still answer there. It stays open, as {why}; T-{number} goes in once it has closed.",
                    participant.handle
                ),
            },
        )?;
        self.changed();
        Ok(())
    }

    /// The conversation a member's window that opens for `first` goes on with: `found`,
    /// the session's, unless `first` is a message of another task than the one that
    /// conversation was last given (`before`) and the host did not prove the program of
    /// the window before gone, in which case a new one (none), whoever gave `first`'s
    /// task told once.
    pub(crate) fn kept_apart(
        &self,
        project: &ProjectView,
        participant: &ParticipantView,
        record: &Record,
        found: Option<ConversationView>,
        before: Option<&TaskThread>,
        first: Option<&MessageView>,
    ) -> Result<Option<ConversationView>, EngineError> {
        let Some(before) = before else {
            return Ok(found);
        };
        let Some(conversation) = found else {
            return Ok(None);
        };
        let other = first
            .and_then(|first| first.task_number)
            .filter(|number| *number != before.task.number);
        let Some(number) = other else {
            return Ok(Some(conversation));
        };
        let gone = record
            .window
            .borrow()
            .last_exit
            .as_ref()
            .is_some_and(|(_, gone)| *gone == Some(true));
        if gone {
            return Ok(Some(conversation));
        }
        let waiting = self.seams.ledger.borrow().task(project.id, number)?;
        if let Some(waiting) = waiting {
            self.seams.ledger.borrow_mut().note(
                project.id,
                &NewNote {
                    from: None,
                    to: waiting.task.requester.clone(),
                    task: Some(number),
                    body: format!(
                        "T-{number} goes to @{} in a new conversation: its window that worked on T-{} closed with no proof its program was gone, which may still write to that conversation, so T-{number} does not go on in it.",
                        participant.handle, before.task.number
                    ),
                },
            )?;
        }
        Ok(None)
    }
}

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
//! no look at what still runs lets it in earlier (clearance-based reuse is not 3.0.6's). A
//! window whose task is done waits to be closed until its screen has been drawn, for a
//! bounded time, as it does when nothing is next for it
//! ([`Dispatcher::still_drawing`]); the message waits that time too, and its requester is
//! told nothing of it.
//! The message waits, and whoever gave its task is told once, where the window stays
//! open: the human keeps it open (Show), its close was refused, or its task is still in
//! the session's hands (paused, whose window stays for the resumption). A pause and a
//! resume of the same task keep its window.
//!
//! The window that opens for the other task goes on with the session's conversation only
//! where the host proved the program of the last window on it gone: Claude Code's record
//! reader then takes what the old process queued and started as gone with it
//! (`Launch::previous_gone`). The proof is kept on the conversation in the ledger
//! (`program_gone`, migration 0024), so a restart keeps it: before, the daemon kept it in
//! memory alone, and after a restart every follow-up of a window that had closed went to
//! a new conversation (T-541). A window that opens on the conversation clears it before
//! its program runs, and so does the human's switch of a window to it. Where the host did
//! not prove it (the window's exit said nothing, or said it ran on, or this daemon never
//! saw it close: a window open when the app quit, which the window's quit ends after the
//! daemon has gone and tells no one, `cf_panes::headless::reap_all`), the other task opens
//! a new conversation, as a session's first task does, and whoever gave it is told once
//! that it does not go on in the earlier task's ([`Dispatcher::kept_apart`]). A follow-up, a
//! task queued for the window that has never been in a window, is not given to a window of its
//! own unasked, though: its brief may be written for a window that remembers the earlier task
//! ("your T-65 snapshot", the field issue of 2026-10-29). It is held for the chief's word
//! (paused, `Ledger::hold_for_context`), told once with how to go on: rewrite the brief with
//! `cf task edit`, and send it on with `cf task resume`, which is the chief's knowing: the task
//! then goes to a conversation of its own, and is not held a second time. A task that was in a
//! window before (resumed after a restart, sent back with words) is not held: the chief's word
//! to resume or reopen it was given knowing the window, and it goes to a conversation of its
//! own as it did, told once.
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

/// What a launch does with the conversation of the session's earlier task.
pub(crate) enum Kept {
    /// The window goes on with this conversation, or opens a new one (none).
    On(Option<ConversationView>),
    /// The task waits for the chief's word instead: it is paused, and the launch is over.
    Held,
}

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
        } else if self.still_drawing(record).await? {
            // The task that held the window is done and its screen is still being drawn:
            // the window closes within a moment, and the message waits for it as before,
            // with nothing to tell its requester.
            return Ok(());
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
    /// the last window on it gone, as the ledger keeps it. Then a follow-up, a task still
    /// queued for the window that no window ever had, is held for the chief's word
    /// ([`Kept::Held`]), once: told to whoever gave it, with how to go on. A task the chief
    /// sent on after that goes to a new conversation (none), knowingly, and so does one that
    /// a window had before or that is not queued, whoever gave it told once.
    pub(crate) fn kept_apart(
        &self,
        project: &ProjectView,
        participant: &ParticipantView,
        found: Option<ConversationView>,
        before: Option<&TaskThread>,
        first: Option<&MessageView>,
    ) -> Result<Kept, EngineError> {
        let Some(before) = before else {
            return Ok(Kept::On(found));
        };
        let Some(conversation) = found else {
            return Ok(Kept::On(None));
        };
        let other = first
            .and_then(|first| first.task_number)
            .filter(|number| *number != before.task.number);
        let Some(number) = other else {
            return Ok(Kept::On(Some(conversation)));
        };
        let gone = self.seams.ledger.borrow().program_gone(conversation.id)?;
        if gone {
            return Ok(Kept::On(Some(conversation)));
        }
        let waiting = self.seams.ledger.borrow().task(project.id, number)?;
        let Some(waiting) = waiting else {
            return Ok(Kept::On(None));
        };
        let (earlier, handle) = (before.task.number, &participant.handle);
        if self
            .seams
            .ledger
            .borrow()
            .held_for_context(project.id, number)?
        {
            return Ok(Kept::On(None));
        }
        // No task message of it ever reached a window: it is a follow-up, and what its brief
        // counts on is the window it follows. A task a window had (resumed, sent back) was
        // given to a window knowingly.
        let never_in_a_window = !waiting.messages.iter().any(|message| {
            message.kind == "task" && matches!(message.state.as_str(), "delivered" | "read")
        });
        if waiting.task.state == "queued" && never_in_a_window {
            let mut ledger = self.seams.ledger.borrow_mut();
            ledger.hold_for_context(
                project.id,
                number,
                &format!(
                    "@{handle}'s window that worked on T-{earlier} closed with no proof its program was gone"
                ),
            )?;
            ledger.note(
                project.id,
                &NewNote {
                    from: None,
                    to: waiting.task.requester.clone(),
                    task: Some(number),
                    body: format!(
                        "T-{number} is paused and waits for your word: @{handle}'s window that worked on T-{earlier} closed with no proof its program was gone, which may still write to that conversation, so T-{number} does not go on in it, and a window of its own would not remember T-{earlier}. If its brief counts on that, rewrite it with cf task edit T-{number} \"…\"; send it to a window of its own with cf task resume T-{number} \"…\", or cancel it."
                    ),
                },
            )?;
            return Ok(Kept::Held);
        }
        self.seams.ledger.borrow_mut().note(
            project.id,
            &NewNote {
                from: None,
                to: waiting.task.requester.clone(),
                task: Some(number),
                body: format!(
                    "T-{number} goes to @{handle} in a new conversation: its window that worked on T-{earlier} closed with no proof its program was gone, which may still write to that conversation, so T-{number} does not go on in it."
                ),
            },
        )?;
        Ok(Kept::On(None))
    }
}

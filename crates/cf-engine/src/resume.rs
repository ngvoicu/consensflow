//! The human's Resume of a project, and what a start leaves for it. A start
//! opens no window and delivers nothing: every project that was open comes back
//! suspended (the ledger's work), its tasks and messages as they were, and the
//! engine settles only what was on its way into a window when the previous
//! process ended ([`Dispatcher::settle_after_restart`]). The human's Resume
//! brings the project back ([`Dispatcher::resume_project`]): its chief, and each
//! window that had work in hand, at work or waiting for an answer, each on its
//! own conversation. What waits is delivered by the usual rules as they come up:
//! a member with no task in hand opens when it is next given work, and a task
//! that is paused, by the human or by the system, stays so until it is resumed.
//!
//! A window whose turn the quit cut is told so, once: the harness's record of
//! its conversation, read before its window opens, has a turn with no end, and
//! a note from ConsensFlow is the first thing the window is given, with what
//! waits for it after. A window at rest when the app quit is given no note.

use std::rc::Rc;

use cf_ledger::{MessageView, NewNote, ParticipantView, ProjectView, RESUME_WORDS};

use crate::dispatcher::Dispatcher;
use crate::handoff::is_handoff;
use crate::runtime::returning;
use crate::seams::EngineError;

/// A window the human's Resume brings back, as far as telling it its turn was
/// cut goes.
pub(crate) enum Coming {
    /// The chief's.
    Chief,
    /// A member's, which has these tasks (their numbers) at work or waiting.
    Member(Vec<i64>),
}

impl Coming {
    /// What the window is told when its turn was cut. A member is told what it
    /// was working on. The chief came back by the human's Resume, not by a quit
    /// alone (a Close cuts it too), so its words say only that ConsensFlow
    /// stopped.
    fn words(&self) -> String {
        match self {
            Coming::Chief => {
                "ConsensFlow stopped in the middle of your turn, which was cut. Go on where you stopped."
                    .to_owned()
            }
            Coming::Member(tasks) => {
                let tasks: Vec<String> = tasks.iter().map(|number| format!("T-{number}")).collect();
                format!(
                    "ConsensFlow quit while you were working on {}. {RESUME_WORDS}",
                    tasks.join(", ")
                )
            }
        }
    }

    /// The task the note is about: a member's first, the chief's none.
    fn task(&self) -> Option<i64> {
        match self {
            Coming::Chief => None,
            Coming::Member(tasks) => tasks.first().copied(),
        }
    }
}

impl Dispatcher {
    /// Once, at start: what was on its way into a window when the previous
    /// process ended is settled. Nothing else of the projects that were open
    /// comes of it: they are suspended, and wait for the human's Resume.
    pub async fn settle_after_restart(self: &Rc<Self>) -> Result<(), EngineError> {
        // An `async` method: its answer reaches this a turn after it was made.
        returning(self.settle_in_flight()).await
    }

    /// The human's Resume: the project is open, and its chief comes back on its
    /// conversation, and so does each member with a task at work or waiting,
    /// on its own. A window comes up apart from this call, and what waits for
    /// it is delivered once it is up.
    pub async fn resume_project(
        self: &Rc<Self>,
        project: i64,
    ) -> Result<Option<ProjectView>, EngineError> {
        let resumed = self
            .seams
            .ledger
            .borrow_mut()
            .set_project_state(project, "open")?;
        let at_work = self.at_work(&resumed)?;
        if let Some(chief) = resumed.participants.iter().find(|p| p.role == "chief") {
            // The human asks for the chief now: a chief that failed before is tried at once.
            self.record_of(chief.id).window.borrow_mut().relaunch = None;
            self.open_soon(chief.id, Some(Coming::Chief)).await;
        }
        for (participant, tasks) in at_work {
            self.open_soon(participant, Some(Coming::Member(tasks)))
                .await;
        }
        self.changed();
        self.project_now(project)
    }

    /// The participants other than the chief and the human that hold a task at
    /// work or waiting, each with the numbers of those tasks. A member whose
    /// agent is gone from the human's agents opens no window: its work goes back
    /// to the board at the next pass, as it does whenever it is found so (the
    /// roster is asked of those with a task in hand alone).
    fn at_work(&self, project: &ProjectView) -> Result<Vec<(i64, Vec<i64>)>, EngineError> {
        let board = self.seams.ledger.borrow().board(project.id)?;
        Ok(board
            .lanes
            .into_iter()
            .filter(|lane| !matches!(lane.participant.role.as_str(), "chief" | "human"))
            .filter_map(|lane| {
                let tasks: Vec<i64> = lane
                    .tasks
                    .iter()
                    .filter(|task| {
                        matches!(task.state.as_str(), "working" | "waiting")
                            && task.assignee.as_ref() == Some(&lane.participant.handle)
                    })
                    .map(|task| task.number)
                    .collect();
                (!tasks.is_empty()).then_some((lane.participant, tasks))
            })
            .filter(|(participant, _)| !self.agent_gone(participant))
            .map(|(participant, tasks)| (participant.id, tasks))
            .collect())
    }

    /// The note that tells a window its turn was cut, when it was, to be its
    /// first message: the one that waits for it already if the app quit again
    /// before it came (so it is told once), else one written now. None when its
    /// turn was not cut: its conversation's record, read with no window open,
    /// has no turn without an end, or cannot be read; and none for a chief that
    /// is given its handoff again, which tells it where things stand.
    pub(crate) async fn cut_note(
        &self,
        project: &ProjectView,
        participant: &ParticipantView,
        coming: &Coming,
    ) -> Result<Option<MessageView>, EngineError> {
        let words = coming.words();
        let waiting = self.seams.ledger.borrow().pending(participant.id)?;
        if waiting
            .iter()
            .any(|message| is_handoff(message) && message.state == "queued")
        {
            return Ok(None);
        }
        let told = waiting.into_iter().find(|message| {
            message.sender.is_none()
                && message.kind == "note"
                && message.state == "queued"
                && message.body == words
        });
        if told.is_some() {
            return Ok(told);
        }
        if !self.turn_left_open(participant.id).await? {
            return Ok(None);
        }
        let note = self.seams.ledger.borrow_mut().note(
            project.id,
            &NewNote {
                from: None,
                to: participant.handle.clone(),
                task: coming.task(),
                body: words,
            },
        )?;
        Ok(Some(note))
    }
}

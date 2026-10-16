//! The human's Resume of a project, and what a start leaves for it. A start
//! opens no window and delivers nothing: every project that was open comes back
//! suspended (the ledger's work), its tasks and messages as they were, and the
//! engine settles only what was on its way into a window when the previous
//! process ended ([`Dispatcher::settle_after_restart`]). The human's Resume
//! brings the project back ([`Dispatcher::resume_project`]): its chief, and each
//! window that had work in hand, at work or waiting for an answer, each on its
//! own conversation, unless its harness says it holds nothing of it (Codex keeps
//! a thread only once it has had a turn): a conversation with nothing to resume
//! opens its window afresh ([`Dispatcher::conversation_to_go_on_with`]), with the
//! role it has now, and no note says a role that changed (`role_change`). What
//! waits is delivered by the usual rules as they come up: a member with no task
//! in hand opens when it is next given work.
//!
//! The work a quit leaves at work comes back so; the work the project's own
//! suspension paused (the human's Close, or the chief's window ending, closes
//! every window of it) comes back the same way: the ledger's write that opens
//! the project has those tasks at work again, before the chief's window is
//! opened, so the chief comes first and is never given the note that they were
//! paused (`cf_ledger`'s `set_project_state`). A task paused for any other reason
//! (the human's or the chief's pause, a window that closed by itself, a hold)
//! stays so until it is resumed.
//!
//! A window whose turn the quit or the Close cut is told so, once: the harness's
//! record of its conversation, read before its window opens, has a turn with no
//! end, and a note from ConsensFlow is the first thing the window is given, with
//! what waits for it after. A window at rest when the app quit is given no note. Where
//! the record cannot tell (Devin's, with no window open), a member with a task
//! in hand is told all the same: a member whose cut turn goes untold reads at
//! rest with its task at work for good, and one told that was at rest only looks
//! again.

use std::cell::RefCell;
use std::collections::HashMap;
use std::future::{poll_fn, Future};
use std::rc::Rc;
use std::task::{Poll, Waker};

use cf_ledger::{
    ConversationView, MessageView, NewNote, ParticipantView, ProjectView, RESUME_WORDS,
};

use crate::dispatcher::Dispatcher;
use crate::handoff::is_handoff;
use crate::runtime::returning;
use crate::seams::EngineError;

/// The Closes under way, by project (the human's, and the one the chief's window
/// ending makes): a Resume of a project waits for its Closes to end, every kill
/// answered, before it reads which tasks they paused. A Resume that overtook a Close
/// resumed a task whose crash a refused kill would have left paused, and left paused
/// one whose kill came after it (both reviews of 2026-10-16, round 3). A Close waits
/// for nothing without a bound: each kill has the pane host's deadline.
#[derive(Default)]
pub(crate) struct Closings {
    under_way: RefCell<HashMap<i64, usize>>,
    waiting: RefCell<Vec<Waker>>,
}

/// A Close under way. Dropping it ends it, however its work ends.
pub(crate) struct Closing<'a> {
    closings: &'a Closings,
    project: i64,
}

impl Closings {
    /// A Close of `project` begins.
    pub(crate) fn begin(&self, project: i64) -> Closing<'_> {
        *self.under_way.borrow_mut().entry(project).or_default() += 1;
        Closing {
            closings: self,
            project,
        }
    }

    /// Ready once no Close of `project` is under way: at once where none is.
    fn ended(&self, project: i64) -> impl Future<Output = ()> + '_ {
        poll_fn(move |cx| {
            if self.under_way.borrow().contains_key(&project) {
                self.waiting.borrow_mut().push(cx.waker().clone());
                Poll::Pending
            } else {
                Poll::Ready(())
            }
        })
    }
}

impl Drop for Closing<'_> {
    fn drop(&mut self) {
        let mut under_way = self.closings.under_way.borrow_mut();
        if let Some(count) = under_way.get_mut(&self.project) {
            *count -= 1;
            if *count == 0 {
                under_way.remove(&self.project);
            }
        }
        drop(under_way);
        let waiting = std::mem::take(&mut *self.closings.waiting.borrow_mut());
        for waker in waiting {
            waker.wake();
        }
    }
}

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
    /// was working on. Each came back by the human's Resume, not by a quit alone
    /// (a Close cuts a turn too), so the words say only that ConsensFlow stopped.
    fn words(&self) -> String {
        match self {
            Coming::Chief => {
                "ConsensFlow stopped in the middle of your turn, which was cut. Go on where you stopped."
                    .to_owned()
            }
            Coming::Member(tasks) => {
                format!(
                    "ConsensFlow stopped while you were working on {}. {RESUME_WORDS}",
                    named(tasks)
                )
            }
        }
    }

    /// Whether `body` is the note that tells the window its turn was cut: these
    /// words, or a member's as builds before 3.0.5 wrote them, which a ledger may
    /// still hold queued for a window that never came.
    fn told_in(&self, body: &str) -> bool {
        body == self.words()
            || matches!(self, Coming::Member(tasks) if body == format!(
                "ConsensFlow quit while you were working on {}. {RESUME_WORDS}",
                named(tasks)
            ))
    }

    /// The task the note is about: a member's first, the chief's none.
    fn task(&self) -> Option<i64> {
        match self {
            Coming::Chief => None,
            Coming::Member(tasks) => tasks.first().copied(),
        }
    }
}

/// The tasks a member's note names, as it names them.
fn named(tasks: &[i64]) -> String {
    let tasks: Vec<String> = tasks.iter().map(|number| format!("T-{number}")).collect();
    tasks.join(", ")
}

impl Dispatcher {
    /// Once, at start: what was on its way into a window when the previous
    /// process ended is settled. Nothing else of the projects that were open
    /// comes of it: they are suspended, and wait for the human's Resume.
    pub async fn settle_after_restart(self: &Rc<Self>) -> Result<(), EngineError> {
        // An `async` method: its answer reaches this a turn after it was made.
        returning(self.settle_in_flight()).await
    }

    /// The human's Resume: the project is open, with the tasks its suspension
    /// paused at work again (the ledger's write), and its chief comes back on its
    /// conversation, and then each member with a task at work or waiting, on its
    /// own. A window comes up apart from this call, and what waits for it is
    /// delivered once it is up. A Close of the project still under way ends
    /// first ([`Closings`]): the Resume answers once it has resumed.
    pub async fn resume_project(
        self: &Rc<Self>,
        project: i64,
    ) -> Result<Option<ProjectView>, EngineError> {
        self.closings.ended(project).await;
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

    /// The conversation a launch goes on with: the participant's current one,
    /// unless its harness says it holds nothing of it
    /// ([`resumable`](cf_harness::contract::Adapter::resumable)). A conversation
    /// the harness holds nothing of is not resumed, since a window opened on one
    /// ends at once (Codex's on a thread that never had a turn closed a chief's
    /// project, 2026-10-13): the window opens afresh, as a first launch does, and
    /// what waits for it is given as on any first launch. Only the harness's no
    /// does that. Where it cannot tell the window is resumed, as it was before it
    /// was asked: ConsensFlow's copy of the conversation is a look at a time
    /// behind, and a window opened afresh on a conversation that holds things
    /// leaves them behind.
    pub(crate) async fn conversation_to_go_on_with(
        &self,
        participant: &ParticipantView,
    ) -> Result<Option<ConversationView>, EngineError> {
        let current = self
            .seams
            .ledger
            .borrow()
            .current_conversation(participant.id)?;
        let Some(current) = current else {
            return Ok(None);
        };
        let session = current
            .native_session
            .as_deref()
            .filter(|session| !session.is_empty());
        let told = match (self.seams.adapters.adapter(&current.harness), session) {
            (Some(adapter), Some(session)) => adapter.resumable(session).await,
            _ => None,
        };
        Ok((told != Some(false)).then_some(current))
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
    /// ends at rest, or cannot tell and the window is the chief's; and none for a
    /// chief that is given its handoff again, which tells it where things stand.
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
                && coming.told_in(&message.body)
        });
        if told.is_some() {
            return Ok(told);
        }
        let cut = match self.turn_cut(participant.id).await? {
            Some(cut) => cut,
            None => matches!(coming, Coming::Member(_)),
        };
        if !cut {
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

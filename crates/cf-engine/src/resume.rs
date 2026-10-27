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
//! stays so until it is resumed. A window that holds more than one task (a
//! session a ledger wrote before the rule of one, or a member's own lane) comes
//! back for none: the same write leaves its tasks paused, and the chief is told
//! of each, to resume it.
//!
//! A window whose turn the quit or the Close cut is told so, once: the harness's
//! record of its conversation, read before its window opens, has a turn with no
//! end, and a note from ConsensFlow is the first thing the window is given, with
//! what waits for it after. The note names the turn: when the message that began it
//! came and its first words, where the record holds them, so that a window can tell
//! whether that turn was one it had left (T-814). A window at rest when the app quit
//! is given no note, and nor is one whose turn had ended with only background work
//! running (its harness's jobs, and its notices of them: background work is rest).
//! Where the record cannot tell (Devin's, with no window open), a member with a task
//! in hand is told all the same: a member whose cut turn goes untold reads at
//! rest with its task at work for good, and one told that was at rest only looks
//! again.
//!
//! The note says the window's state at that Resume, so it holds only for it: one an
//! earlier Resume wrote, whose window never came to be given it (the project closed
//! again first), is judged again from the record at the next, and withdrawn unless
//! the record still says it word for word. Before, such a note was given days late
//! (sites, 2026-10-23: a note written at a Resume the human closed again a second
//! later waited, queued, for the next Resume).
//!
//! A Resume that comes while the start still settles what was on its way waits
//! for it ([`ResumeState`]): what the settle confirms moves a task to work, which
//! the Resume brings a window back for. Before, a worker whose words that resume its
//! task the start was reading was given no window, and the next pass paused the
//! task it was then at work on, for a window that was gone (the rig, under load,
//! 2026-10-16).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::future::{poll_fn, Future};
use std::rc::Rc;
use std::task::{Poll, Waker};

use cf_harness::contract::{Opening, TurnLeft};
use cf_ledger::{
    continuation, continuation_in, continuation_tasks, cut_turn, ConversationView, MessageView,
    NewNote, ParticipantView, ProjectView, Stop, RESUME_WORDS,
};

use crate::dispatcher::Dispatcher;
use crate::handoff::is_handoff;
use crate::runtime::returning;
use crate::seams::EngineError;

/// The Closes under way, by project (the human's, and the one the chief's window
/// ending makes), as the closes of their windows, and the human's as a whole, to its
/// answer: a Resume of a project waits for them to end, every kill answered, before
/// it reads which tasks they paused. A Resume that overtook a Close resumed a task
/// whose crash a refused kill would have left paused, and left paused one whose kill
/// came after it (both reviews of 2026-10-16, round 3). A Close waits for nothing
/// without a bound: each kill has the pane host's deadline. Each window's close
/// counts from before the Close waits on anything until it is over, however the
/// others end: a Close answers at the first close that fails, and one that failed
/// let the Resume through while another kill was still out (Astra's review of
/// 2026-10-16, round 4).
#[derive(Default)]
pub(crate) struct Closings(Rc<UnderWay>);

/// The closes under way, and the Resumes that wait for them to end.
#[derive(Default)]
struct UnderWay {
    count: RefCell<HashMap<i64, usize>>,
    waiting: RefCell<Vec<Waker>>,
}

/// A Close under way, or a window's close in it. Dropping it ends it, however
/// its work ends.
pub(crate) struct Closing {
    closings: Rc<UnderWay>,
    project: i64,
}

impl Closings {
    /// A Close of `project` begins, or a window's close in it.
    pub(crate) fn begin(&self, project: i64) -> Closing {
        *self.0.count.borrow_mut().entry(project).or_default() += 1;
        Closing {
            closings: Rc::clone(&self.0),
            project,
        }
    }

    /// Ready once no Close of `project` is under way: at once where none is.
    fn ended(&self, project: i64) -> impl Future<Output = ()> + '_ {
        poll_fn(move |cx| {
            if self.0.count.borrow().contains_key(&project) {
                self.0.waiting.borrow_mut().push(cx.waker().clone());
                Poll::Pending
            } else {
                Poll::Ready(())
            }
        })
    }
}

impl Drop for Closing {
    fn drop(&mut self) {
        let mut under_way = self.closings.count.borrow_mut();
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
    /// A member's, with the one task it holds at work or waiting, and the stops
    /// its window had paid (`stop_of`), read before anything of its return waits:
    /// it comes back only while no stop was asked of it since
    /// ([`Dispatcher::still_coming`]).
    Member { task: i64, paid: Option<Stop> },
}

/// How the chief's note that its turn was cut begins, in this build's words and in
/// those of the builds before it, which said no more.
const CHIEF_CUT: &str = "ConsensFlow stopped in the middle of your turn, which was cut";

impl Coming {
    /// What the window is told when its turn was cut: the turn, where its record
    /// shows the message that began it (`opening`). A member is told what it was
    /// working on, in a continuation (the ledger's words, which go with any move of
    /// the task). Each came back by the human's Resume, not by a quit alone (a Close
    /// cuts a turn too), so the words say only that ConsensFlow stopped.
    pub(crate) fn words(&self, opening: Option<&Opening>) -> String {
        let turn = opening.map(|opening| cut_turn(opening.at.as_deref(), &opening.words));
        match (self, turn) {
            (Coming::Chief, None) => format!("{CHIEF_CUT}. {RESUME_WORDS}"),
            (Coming::Chief, Some(turn)) => format!("{CHIEF_CUT}: {turn}. {RESUME_WORDS}"),
            (Coming::Member { task, .. }, None) => continuation(*task),
            (Coming::Member { task, .. }, Some(turn)) => continuation_in(*task, &turn),
        }
    }

    /// Whether `body` is a note that tells the window its turn was cut, in the words
    /// of this build or of one before it, which a ledger may still hold queued for a
    /// window that never came: a member's names its one task.
    fn told_in(&self, body: &str) -> bool {
        match self {
            Coming::Chief => body.starts_with(CHIEF_CUT),
            Coming::Member { task, .. } => continuation_tasks(body) == Some(vec![*task]),
        }
    }

    /// The task the note is about: a member's, the chief's none.
    pub(crate) fn task(&self) -> Option<i64> {
        match self {
            Coming::Chief => None,
            Coming::Member { task, .. } => Some(*task),
        }
    }
}

/// Whether the start's settle of what was on its way is under way, and the
/// Resumes that wait for its end.
#[derive(Default)]
pub(crate) struct ResumeState {
    settling: Cell<bool>,
    waiting: RefCell<Vec<Waker>>,
}

impl ResumeState {
    /// Once the start's settle is over: at once where none is under way.
    async fn settled(&self) {
        poll_fn(|cx| {
            if self.settling.get() {
                self.waiting.borrow_mut().push(cx.waker().clone());
                Poll::Pending
            } else {
                Poll::Ready(())
            }
        })
        .await;
    }

    /// The start's settle is over: every Resume that waits for it goes on.
    fn over(&self) {
        self.settling.set(false);
        let waiting: Vec<Waker> = self
            .waiting
            .try_borrow_mut()
            .map(|mut waiting| waiting.drain(..).collect())
            .unwrap_or_default();
        for waiting in waiting {
            waiting.wake();
        }
    }
}

/// The start's settle under way: over once this goes, however the settle ends (a
/// panic in it is contained by the daemon, which goes on, and its Resumes with it).
struct Settling<'a>(&'a ResumeState);

impl<'a> Settling<'a> {
    fn begin(state: &'a ResumeState) -> Self {
        state.settling.set(true);
        Self(state)
    }
}

impl Drop for Settling<'_> {
    fn drop(&mut self) {
        self.0.over();
    }
}

impl Dispatcher {
    /// Once, at start: what was on its way into a window when the previous
    /// process ended is settled. Nothing else of the projects that were open
    /// comes of it: they are suspended, and wait for the human's Resume, which
    /// waits for this.
    pub async fn settle_after_restart(self: &Rc<Self>) -> Result<(), EngineError> {
        let _settling = Settling::begin(&self.resumes);
        // An `async` method: its answer reaches this a turn after it was made.
        returning(self.settle_in_flight()).await
    }

    /// The human's Resume: the project is open, with the tasks its suspension
    /// paused at work again (the ledger's write), and its chief comes back on its
    /// conversation, and then each member with a task at work or waiting, on its
    /// own. A window comes up apart from this call, and what waits for it is
    /// delivered once it is up. A Close of the project still under way ends
    /// first ([`Closings`]), and one that comes while the start settles what was
    /// on its way waits for that too: the Resume answers once it has resumed.
    pub async fn resume_project(
        self: &Rc<Self>,
        project: i64,
    ) -> Result<Option<ProjectView>, EngineError> {
        self.closings.ended(project).await;
        self.resumes.settled().await;
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
        for (participant, coming) in at_work {
            self.open_soon(participant, Some(coming)).await;
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
    /// work or waiting, each as its window comes back: with that task, and the
    /// stops its window had paid, read with it, before anything of the Resume
    /// waits. A window comes back for one task: the write that opened the project
    /// left none at work of a window that holds several (`cf_ledger`'s
    /// `resume_with_project` pauses them, for the chief to resume one by one). A
    /// member whose agent is gone from the human's agents opens no window:
    /// its work goes back to the board at the next pass, as it does whenever it is
    /// found so (the roster is asked of those with a task in hand alone).
    fn at_work(&self, project: &ProjectView) -> Result<Vec<(i64, Coming)>, EngineError> {
        let board = self.seams.ledger.borrow().board(project.id)?;
        board
            .lanes
            .into_iter()
            .filter(|lane| !matches!(lane.participant.role.as_str(), "chief" | "human"))
            .filter_map(|lane| {
                let task = lane
                    .tasks
                    .iter()
                    .find(|task| {
                        matches!(task.state.as_str(), "working" | "waiting")
                            && task.assignee.as_ref() == Some(&lane.participant.handle)
                    })?
                    .number;
                Some((lane.participant, task))
            })
            .filter(|(participant, _)| !self.agent_gone(participant))
            .map(|(participant, task)| {
                let paid = self.seams.ledger.borrow().stop_of(participant.id)?;
                Ok((participant.id, Coming::Member { task, paid }))
            })
            .collect()
    }

    /// Whether a member's window the Resume brings back still comes as it was
    /// read: no stop was asked of it since the Resume read its work. A pause, a
    /// cancel, a release or an end of the task it works on that came meanwhile, as
    /// its record was read, brings it back as it was no more (Astra's review of
    /// 2026-10-16, round 4: a pause the human made while the Resume read whether
    /// the turn was cut was taken for one the window had paid, and the window
    /// opened told to go on). The chief's always comes.
    pub(crate) fn still_coming(
        &self,
        participant: &ParticipantView,
        coming: &Coming,
    ) -> Result<bool, EngineError> {
        let Coming::Member { paid, .. } = coming else {
            return Ok(true);
        };
        Ok(self.seams.ledger.borrow().stop_of(participant.id)? == *paid)
    }

    /// The note that tells a window its turn was cut, when it was, to be its
    /// first message: written now from its record, or the one an earlier Resume left
    /// waiting where the record says it still, word for word (so it is told once).
    /// None when its turn was not cut: its conversation's record, read with no window
    /// open, ends at rest (with only background work running, or none), or cannot
    /// tell and the window is the chief's; and none for a chief that is given its
    /// handoff again, which tells it where things stand. None too, and nothing
    /// written, where the work it was to come back for stopped while the record was
    /// read ([`Dispatcher::still_coming`]). What an earlier Resume left waiting and
    /// the record no longer says is withdrawn: it said the window's state then.
    pub(crate) async fn cut_note(
        &self,
        project: &ProjectView,
        participant: &ParticipantView,
        coming: &Coming,
    ) -> Result<Option<MessageView>, EngineError> {
        let handed_again = self
            .seams
            .ledger
            .borrow()
            .pending(participant.id)?
            .iter()
            .any(|message| is_handoff(message) && message.state == "queued");
        if handed_again {
            return Ok(None);
        }
        let words = match self.turn_cut(participant.id).await? {
            Some(TurnLeft::Cut(opening)) => Some(coming.words(opening.as_ref())),
            Some(TurnLeft::AtRest) => None,
            None => matches!(coming, Coming::Member { .. }).then(|| coming.words(None)),
        };
        if !self.still_coming(participant, coming)? {
            return Ok(None);
        }
        let mut ledger = self.seams.ledger.borrow_mut();
        let told: Vec<MessageView> = ledger
            .pending(participant.id)?
            .into_iter()
            .filter(|message| {
                message.sender.is_none()
                    && message.kind == "note"
                    && message.state == "queued"
                    && coming.told_in(&message.body)
            })
            .collect();
        let mut kept = None;
        for message in told {
            if kept.is_none() && words.as_deref() == Some(message.body.as_str()) {
                kept = Some(message);
            } else {
                ledger.withdraw_note(&message, "the Resume read the window's record again")?;
            }
        }
        let Some(words) = words.filter(|_| kept.is_none()) else {
            return Ok(kept);
        };
        let note = ledger.note(
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

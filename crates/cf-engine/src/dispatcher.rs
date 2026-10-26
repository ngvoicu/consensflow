//! The dispatcher: the only actor in the daemon. Agents never open panes or
//! type into them; they change the ledger (a task, a question, an answer), and
//! the dispatcher makes it happen in the panes. Its rules are the long comment
//! of Node's dispatcher, each held by a test of Node's dispatcher suite, ported
//! under its sentence.
//!
//! It answers the human's operations, steps every window on each pass, and
//! holds each participant for one piece of work at a time ([`crate::runtime::Hold`]). What
//! it orchestrates has an owner each, a module of its own on this type: who
//! takes which task and who is out of quota (`scheduler`), each window's
//! launch, looks and close (`windows`), what goes into a window and what
//! comes back out (`deliveries`), the human's Switch chief (`chief_switch`),
//! the copy of each window's conversation (`transcripts`), the task whose
//! window went away, its requester told once for the tasks of a pass (`stalls`),
//! the human's Resume of a project a start suspended, and what a start
//! leaves for it (`resume`), the human's wake-up calls, which nudge a chief
//! at rest (`wake_ups`), and a role that changed under a conversation whose
//! harness keeps its first instructions, which its resumed window is told in a
//! message (`role_change`).
//!
//! Work runs as Node's did ([`crate::runtime`]): a piece of work is begun
//! where JavaScript called it, and what JavaScript did not await goes on
//! apart. No record and no ledger borrow is held across a wait.

use std::cell::RefCell;
use std::collections::HashMap;
use std::future::Future;
use std::rc::Rc;
use std::task::{Context, Poll, Waker};

use cf_base::time::iso;
use cf_harness::contract::{Observed, Pane};
use cf_ledger::model::fits_role;
use cf_ledger::{
    DeletedProject, NewProject, ParticipantView, ProjectView, RemovedMember, TaskReleased,
};
use cf_proto::panes::PaneExit;
use cf_proto::trace::{TraceLine, Traced, WindowEvent};

use crate::background::quote;
use crate::chief_switch::SwitchTo;
use crate::record::Record;
use crate::resume::{Closing, Closings, Coming, ResumeState};
use crate::runtime::{all, begin, returning, Begun, LocalWork};
use crate::scheduler::SchedulerState;
use crate::seams::{EngineError, Seams};
use crate::shown::Shown;
use crate::stalls::StallsState;
use crate::trust::{self, TrustState, Trusting};
use crate::windows::{Activity, ActivityState, WindowsState};

/// When a Switch chief goes: now, or once the chief's turn ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwitchWhen {
    Now,
    Turn,
}

/// A closed project starts and changes no work, whatever asks: it is resumed first.
pub fn require_open(project: &ProjectView) -> Result<(), EngineError> {
    if project.state == "open" {
        Ok(())
    } else {
        Err(EngineError::said(
            "project-closed",
            format!("{} is closed: resume it first", project.name),
        ))
    }
}

/// A chief runs on one of the human's saved agents, never on a harness's
/// own default model: asked for without one, the daemon says what to pick.
pub fn require_chief_agent(agent: Option<&str>) -> Result<&str, EngineError> {
    agent.filter(|agent| !agent.is_empty()).ok_or_else(|| {
        EngineError::said(
            "no-chief-agent",
            "pick one of your saved agents for the chief: its harness, model and effort come with it",
        )
    })
}

/// The engine.
pub struct Dispatcher {
    pub(crate) seams: Seams,
    /// Each participant's record, made the first time it is asked for.
    records: RefCell<HashMap<i64, Rc<Record>>>,
    /// The records of participants forgotten while their window was still
    /// open, until it exits.
    leaving: RefCell<Vec<Rc<Record>>>,
    /// The pane of the last window of each participant forgotten since, and the
    /// conversation it wrote to: the host's word on it, which may come after, is kept on
    /// that conversation all the same ([`Dispatcher::heard_exit`]), and a session the
    /// human deleted, brought back for a follow-up, opens on that conversation by it
    /// (`ownership`), in a record of its own.
    left_exits: RefCell<HashMap<i64, (Pane, Option<i64>)>>,
    listeners: RefCell<Vec<Rc<dyn Fn()>>>,
    transcript_listeners: RefCell<Vec<Rc<dyn Fn()>>>,
    /// What the windows keep across records (`windows`).
    pub(crate) windows: WindowsState,
    /// What the scheduler keeps across records (`scheduler`).
    pub(crate) scheduler: SchedulerState,
    /// What the bursts of stalls under way keep (`stalls`).
    pub(crate) stalls: StallsState,
    /// The Closes under way, which a Resume waits for (`resume`).
    pub(crate) closings: Closings,
    /// What the human's Resume waits for after a start (`resume`).
    pub(crate) resumes: ResumeState,
    /// What the questions of windows about trusting their folder were said of (`trust`).
    pub(crate) trust: TrustState,
}

impl Dispatcher {
    pub fn new(seams: Seams) -> Rc<Self> {
        Rc::new(Self {
            seams,
            records: RefCell::new(HashMap::new()),
            leaving: RefCell::new(Vec::new()),
            left_exits: RefCell::new(HashMap::new()),
            listeners: RefCell::new(Vec::new()),
            transcript_listeners: RefCell::new(Vec::new()),
            windows: WindowsState::default(),
            scheduler: SchedulerState::default(),
            stalls: StallsState::default(),
            closings: Closings::default(),
            resumes: ResumeState::default(),
            trust: TrustState::default(),
        })
    }

    /// Hears each change of the engine's state the board shows.
    pub fn on_change(&self, listener: Rc<dyn Fn()>) {
        self.listeners.borrow_mut().push(listener);
    }

    /// Hears each look that copied something new of what a window wrote.
    pub fn on_transcript(&self, listener: Rc<dyn Fn()>) {
        self.transcript_listeners.borrow_mut().push(listener);
    }

    /// What a participant's window is doing.
    pub fn activity(&self, participant: i64) -> Activity {
        self.record(participant).map_or_else(
            || Activity::of(ActivityState::Closed),
            |record| record.window.borrow().activity.clone(),
        )
    }

    /// Whether a message waits for this participant until the human sends
    /// what they typed in its window.
    pub fn holding(&self, participant: i64) -> Result<bool, EngineError> {
        let unsent = self
            .record(participant)
            .is_some_and(|record| record.delivery.borrow().unsent);
        Ok(unsent
            && self
                .seams
                .ledger
                .borrow()
                .next_delivery(participant)?
                .is_some())
    }

    /// Whether the human hid this window and it has not closed yet: Show
    /// makes it theirs again.
    pub fn hidden(&self, participant: i64) -> bool {
        self.record(participant)
            .is_some_and(|record| record.window.borrow().hidden)
    }

    /// The Switch chief waiting for this chief's turn to end.
    pub fn pending_switch(&self, participant: i64) -> Option<SwitchTo> {
        self.record(participant).and_then(|record| {
            record
                .pending_switch
                .borrow()
                .as_ref()
                .map(|pending| pending.to.clone())
        })
    }

    /// The live window of a participant.
    pub fn pane(&self, participant: i64) -> Option<Pane> {
        self.record(participant)
            .and_then(|record| record.window.borrow().pane.clone())
    }

    /// Refuses a harness ConsensFlow has no adapter for.
    pub fn requires_adapter(&self, harness: &str) -> Result<(), EngineError> {
        self.require_adapter(harness)
    }

    /// The chief asked for: one of the human's saved agents, on its harness,
    /// whose windows ConsensFlow opens, and no image agent: it only designs.
    fn require_chief(&self, harness: &str, agent: Option<&str>) -> Result<(), EngineError> {
        let agent = require_chief_agent(agent)?;
        self.require_adapter(harness)?;
        let saved = self.seams.roster.agent(agent)?.ok_or_else(|| {
            EngineError::said("unknown-agent", format!("{agent} is not among your agents"))
        })?;
        if !fits_role(saved.designer, "chief") {
            return Err(EngineError::said(
                "image-agent-chief",
                format!("{agent} is an image agent, which can only be an image designer"),
            ));
        }
        Ok(())
    }

    /// A new project: the ledger records it with its chief, on the saved
    /// agent it names, and its staff, and with the wake-up calls and the specs
    /// the request says, all in one write; its chief window opens after the
    /// answer, on the project as that write left it, so its first launch has
    /// the specs.
    pub async fn open_project(
        self: &Rc<Self>,
        request: NewProject,
    ) -> Result<Option<ProjectView>, EngineError> {
        self.require_chief(&request.chief.harness, request.chief.agent.as_deref())?;
        for member in &request.staff {
            self.require_adapter(&member.harness)?;
        }
        let project = self.seams.ledger.borrow_mut().create_project(&request)?;
        if let Some(chief) = project.participants.iter().find(|p| p.handle == "chief") {
            self.open_soon(chief.id, None).await;
        }
        self.project_now(project.id)
    }

    /// The human's Close: the project is suspended and every window of it goes.
    pub async fn close_project(
        self: &Rc<Self>,
        project: i64,
    ) -> Result<Option<ProjectView>, EngineError> {
        // The Close as a whole, past its windows' closes: its wake-up withdrawn and
        // its answer given before a Resume opens the project again.
        let _closing = self.closings.begin(project);
        let suspended = self
            .seams
            .ledger
            .borrow_mut()
            .set_project_state(project, "suspended")?;
        let kept = self
            .close_windows(project, &suspended.participants, None)
            .await?;
        self.withdraw_wake_up(project)?;
        self.changed();
        if !kept.is_empty() {
            let windows: Vec<String> = kept
                .iter()
                .map(|handle| format!("@{handle}'s window"))
                .collect();
            return Err(EngineError::said(
                "window-kept",
                format!(
                    "{} would not close: resume the project and close it again",
                    windows.join(", ")
                ),
            ));
        }
        self.project_now(project)
    }

    /// Closes these participants' windows, each once its step in progress is
    /// over: the handles whose windows would not close. What closes is the
    /// window of the record waited on, forgotten meanwhile or not. The tasks
    /// the windows were working on are told to their requesters together, with
    /// `because` after the word that each window closed where there is one. Each
    /// window's close is part of `project`'s Close, as a Resume waits for
    /// ([`Closings`]), from before any of them waits until it is over, though the
    /// first that fails answers this.
    async fn close_windows(
        self: &Rc<Self>,
        project: i64,
        participants: &[ParticipantView],
        because: Option<&str>,
    ) -> Result<Vec<String>, EngineError> {
        let _burst = self.stalls.begin_burst();
        // A box the engine emptied counts as empty no more: what the human types into
        // a window the close could not close goes unseen until the Resume, and an
        // interrupt of ours may put a prompt back there. The human's Close and the
        // chief's own exit alike (Calliope's reviews of 2026-10-21, rounds 13 and 14).
        for participant in participants {
            self.record_of(participant.id).delivery.borrow_mut().emptied = None;
        }
        let parts: Vec<Closing> = participants
            .iter()
            .map(|_| self.closings.begin(project))
            .collect();
        let mut closing = Vec::new();
        for (participant, part) in participants.iter().zip(parts) {
            let record = self.record_of(participant.id);
            let (this, held, handle) = (
                Rc::clone(self),
                Rc::clone(&record),
                participant.handle.clone(),
            );
            let because = because.map(str::to_owned);
            let work = async move {
                let _part = part;
                let pane = held.window.borrow().pane.clone();
                let went = match pane {
                    None => true,
                    Some(pane) => {
                        held.window.borrow_mut().closed_with = because;
                        let went = this.close_own(&held, &pane).await;
                        held.window.borrow_mut().closed_with = None;
                        went?
                    }
                };
                Ok::<_, EngineError>((!went).then_some(handle))
            };
            closing.push(self.exclusive(&record, work).await);
        }
        let kept = all(closing).await?;
        Ok(kept.into_iter().flatten().collect())
    }

    /// Participants that left are forgotten at once, quota marks and all; a
    /// window one still has closes once its step in progress is over.
    async fn forget(self: &Rc<Self>, participants: &[i64]) -> Result<(), EngineError> {
        let mut leaving = Vec::new();
        for id in participants {
            let Some(record) = self.records.borrow_mut().remove(id) else {
                continue;
            };
            let last = record.window.borrow().last_exit.clone();
            if let Some(last) = last {
                self.left_exits.borrow_mut().insert(*id, last);
            }
            self.leaving.borrow_mut().push(Rc::clone(&record));
            leaving.push(record);
        }
        let mut closing = Vec::new();
        for record in leaving {
            let this = Rc::clone(self);
            let held = Rc::clone(&record);
            let work = async move { this.close_leaving(&held).await };
            closing.push(self.exclusive(&record, work).await);
        }
        all(closing).await?;
        Ok(())
    }

    /// A forgotten participant's window closes; its exit is known by the
    /// window alone (`pane_exited`).
    async fn close_leaving(self: &Rc<Self>, record: &Rc<Record>) -> Result<(), EngineError> {
        if record.window.borrow().pane.is_none() {
            self.leaving
                .borrow_mut()
                .retain(|kept| !Rc::ptr_eq(kept, record));
            return Ok(());
        }
        self.retire(record).await?;
        Ok(())
    }

    /// The human opens a session's window with nothing to deliver: it comes
    /// back on its own conversation and stays open until the human hides it,
    /// or its session or its project ends. One open already, and hidden
    /// since, is the human's again.
    pub async fn open_window(
        self: &Rc<Self>,
        project: i64,
        handle: &str,
    ) -> Result<Option<ProjectView>, EngineError> {
        let (found, participant) = self.session_of(project, handle)?;
        require_open(&found)?;
        {
            let record = self.record_of(participant.id);
            let mut window = record.window.borrow_mut();
            window.pinned = true;
            window.hidden = false;
        }
        self.open_soon(participant.id, None).await;
        self.changed();
        self.project_now(project)
    }

    /// The human hides a session's terminal: a window they opened is theirs
    /// no longer, and closes as any session's once free and its turn settled.
    pub async fn hide_window(
        self: &Rc<Self>,
        project: i64,
        handle: &str,
    ) -> Result<Option<ProjectView>, EngineError> {
        let (_, participant) = self.session_of(project, handle)?;
        {
            let record = self.record_of(participant.id);
            let mut window = record.window.borrow_mut();
            if window.pinned {
                window.pinned = false;
                window.hidden = true;
            }
        }
        self.changed();
        self.project_now(project)
    }

    /// The human says a member, or the chief, out of quota is back before its reset.
    pub fn back_from_quota(
        &self,
        project: i64,
        handle: &str,
    ) -> Result<ParticipantView, EngineError> {
        let found = self.known_project(project)?;
        let participant = found
            .participants
            .iter()
            .find(|p| p.handle == handle && p.role != "human")
            .ok_or_else(|| {
                EngineError::said(
                    "no-participant",
                    format!("no @{handle} in project {project}"),
                )
            })?;
        let member = self.member_of(&found, participant);
        let back = self
            .seams
            .ledger
            .borrow_mut()
            .mark_back(member.id, "by @human")?;
        self.changed();
        Ok(back)
    }

    /// The human gives a task in a window to another member of its tier. Its
    /// window is stopped first, even one the human opened; one that would
    /// not stop keeps the task, and the human is told.
    pub async fn reassign_task(
        self: &Rc<Self>,
        project: i64,
        number: i64,
    ) -> Result<TaskReleased, EngineError> {
        self.seams.ledger.borrow().check_release(project, number)?;
        let task = self
            .seams
            .ledger
            .borrow()
            .task(project, number)?
            .ok_or_else(|| {
                EngineError::said("no-task", format!("no T-{number} in project {project}"))
            })?;
        let assignee = task.task.assignee.clone();
        let holder = self
            .known_project(project)?
            .participants
            .into_iter()
            .find(|participant| Some(&participant.handle) == assignee.as_ref());
        // Paused before anyone took it: there is no window to stop.
        let Some(holder) = holder else {
            return self.release(project, number);
        };
        let record = self.record_of(holder.id);
        let (this, held) = (Rc::clone(self), Rc::clone(&record));
        let work = async move {
            let pinned = std::mem::replace(&mut held.window.borrow_mut().pinned, false);
            let open = held.window.borrow().pane.is_some();
            if open && !this.retire(&held).await? {
                held.window.borrow_mut().pinned = pinned;
                return Err(EngineError::said(
                    "window-kept",
                    format!(
                        "@{}'s window could not be stopped, so T-{number} stays with it: try again",
                        holder.handle
                    ),
                ));
            }
            this.release(project, number)
        };
        self.exclusive(&record, work).await.await
    }

    fn release(&self, project: i64, number: i64) -> Result<TaskReleased, EngineError> {
        let released = self
            .seams
            .ledger
            .borrow_mut()
            .release_task(project, number, "by @human")?;
        self.changed();
        Ok(released)
    }

    /// The human deletes a session: the ledger takes it off the board and
    /// keeps its conversation, and it is forgotten with its window.
    pub async fn end_session(
        self: &Rc<Self>,
        project: i64,
        handle: &str,
    ) -> Result<ProjectView, EngineError> {
        let (_, participant) = self.session_of(project, handle)?;
        let ended = self
            .seams
            .ledger
            .borrow_mut()
            .end_session(project, handle, "human")?;
        self.forget(&[participant.id]).await?;
        self.changed();
        Ok(ended)
    }

    fn session_of(
        &self,
        project: i64,
        handle: &str,
    ) -> Result<(ProjectView, ParticipantView), EngineError> {
        let found = self.seams.ledger.borrow().project(project)?;
        let participant = found.as_ref().and_then(|found| {
            found
                .participants
                .iter()
                .find(|p| p.handle == handle && p.member.is_some())
                .cloned()
        });
        match (found, participant) {
            (Some(found), Some(participant)) => Ok((found, participant)),
            _ => Err(EngineError::said(
                "no-session",
                format!("no session @{handle} in project {project}"),
            )),
        }
    }

    /// A project the ledger has; one it does not is refused.
    pub(crate) fn known_project(&self, project: i64) -> Result<ProjectView, EngineError> {
        self.seams
            .ledger
            .borrow()
            .project(project)?
            .ok_or_else(|| EngineError::said("no-project", format!("no project {project}")))
    }

    /// The project as the ledger has it now, which an operation answers
    /// with: none once it was deleted meanwhile, where Node's
    /// `this.#ledger.project(projectId)` answered null.
    pub(crate) fn project_now(&self, project: i64) -> Result<Option<ProjectView>, EngineError> {
        Ok(self.seams.ledger.borrow().project(project)?)
    }

    /// A closed project goes for good; the ledger refuses an open one. Its
    /// participants are forgotten, and a window of it whose exit has not
    /// come yet is closed.
    pub async fn delete_project(
        self: &Rc<Self>,
        project: i64,
    ) -> Result<DeletedProject, EngineError> {
        let found = self.seams.ledger.borrow().project(project)?;
        let deleted = self.seams.ledger.borrow_mut().delete_project(project)?;
        // What is remembered of its tasks and participants goes now, and so do its own lines in the trace.
        self.forget_project(deleted.id);
        self.seams.trace.forget(project);
        let ids: Vec<i64> = found
            .map(|found| found.participants.iter().map(|p| p.id).collect())
            .unwrap_or_default();
        self.forget(&ids).await?;
        // A deleted project leaves no trace but the line that says it was.
        self.seams.trace.line(TraceLine {
            at: iso(self.now()),
            what: Traced::ProjectDeleted(deleted.clone()),
        });
        self.changed();
        Ok(deleted)
    }

    /// The human takes a member off the staff once its step in progress is
    /// over; its sessions leave with it, and all are forgotten.
    pub async fn remove_member(
        self: &Rc<Self>,
        project: i64,
        handle: &str,
    ) -> Result<RemovedMember, EngineError> {
        let member = self
            .seams
            .ledger
            .borrow()
            .project(project)?
            .and_then(|found| found.participants.into_iter().find(|p| p.handle == handle));
        // Not in the staff: the ledger refuses it and says why.
        let Some(member) = member else {
            return Ok(self
                .seams
                .ledger
                .borrow_mut()
                .remove_member(project, handle)?);
        };
        let record = self.record_of(member.id);
        let (this, held, handle) = (Rc::clone(self), Rc::clone(&record), handle.to_owned());
        let work = async move {
            // One forgotten meanwhile has left already: refused as a member that left.
            if this.forgotten(&held) {
                return Err(EngineError::said(
                    "member-left",
                    format!("@{handle} left the staff"),
                ));
            }
            let mut left = vec![member.id];
            left.extend(
                this.known_project(project)?
                    .participants
                    .iter()
                    .filter(|p| p.member_id == Some(member.id))
                    .map(|p| p.id),
            );
            let removed = this
                .seams
                .ledger
                .borrow_mut()
                .remove_member(project, &handle)?;
            Ok((removed, left))
        };
        let (removed, left) = self.exclusive(&record, work).await.await?;
        self.forget(&left).await?;
        self.changed();
        Ok(removed)
    }

    /// One pass over every participant: each looks at its window, and a
    /// launch or a delivery it starts goes on apart. A project whose open
    /// tasks it gives out is read again, so their sessions open this pass.
    /// It answers at the first step that fails, the others going on. The
    /// tasks whose windows are gone are told to their requesters together.
    pub async fn pass(self: &Rc<Self>) -> Result<(), EngineError> {
        let _burst = self.stalls.begin_burst();
        self.resume_held()?;
        let listed = self.seams.ledger.borrow().projects()?;
        let mut projects = Vec::with_capacity(listed.len());
        for project in listed {
            if project.state == "open" && self.assign_open_tasks(&project)? {
                projects.push(self.known_project(project.id)?);
            } else {
                projects.push(project);
            }
        }
        let mut steps: Vec<Begun<Result<(), EngineError>>> = Vec::new();
        for project in projects {
            // One with no window and nothing on its way or in its hands is not stepped.
            let working = self.seams.ledger.borrow().with_work(project.id)?;
            let project = Rc::new(project);
            for participant in &project.participants {
                if participant.role == "human" {
                    continue;
                }
                let idle = participant.member_id.is_some()
                    && !working.contains(&participant.id)
                    && self.pane(participant.id).is_none();
                if idle {
                    continue;
                }
                let record = self.record_of(participant.id);
                let (this, at, who) = (Rc::clone(self), Rc::clone(&project), participant.clone());
                // Boxed where it is made: a step holds every wait below it, and
                // moved by value through the hold it filled the stack.
                let step = Box::pin(async move { this.step(&at, &who).await });
                steps.extend(self.try_exclusive(&record, step).await);
            }
        }
        all(steps).await?;
        Ok(())
    }

    /// A window ended (`pane.exit` from the pane host), told where the
    /// host's frame is read: what the exit changes is changed before this
    /// returns, and what it still has to do (a chief's project closing) is
    /// returned, for the reader to spawn onto the executor and run apart.
    /// Nobody awaits an exit the host sent, so one that cannot be settled is
    /// written down; the engine's own close of a window fails with it
    /// instead. The exit says how the program ended and what its screen last
    /// showed, where the host did.
    pub fn pane_exited(self: &Rc<Self>, exit: PaneExit) -> Option<LocalWork> {
        let this = Rc::clone(self);
        let pane = Pane {
            id: exit.id.clone(),
            generation: exit.generation,
        };
        let shown = Shown::from_exit(exit);
        let mut work: LocalWork = Box::pin(async move {
            if let Err(cause) = this.exited(&pane, shown).await {
                this.write_down(&cause);
            }
        });
        // Polled here, as JavaScript ran it to its first wait; the reader
        // polls the rest with its own waker.
        match work.as_mut().poll(&mut Context::from_waker(Waker::noop())) {
            Poll::Ready(()) => None,
            Poll::Pending => Some(work),
        }
    }

    /// What a window's exit settles: the engine's own close of a window
    /// awaits it and fails with it; the host's event (`pane_exited`) tells it
    /// to the log. `shown` is what the host said of the window's end, which a
    /// launch that the exit ended tells in its failure.
    pub(crate) async fn exited(
        self: &Rc<Self>,
        pane: &Pane,
        shown: Shown,
    ) -> Result<(), EngineError> {
        let ended = |candidate: Option<&Pane>| candidate.is_some_and(|candidate| candidate == pane);
        let record = {
            let records = self.records.borrow();
            let leaving = self.leaving.borrow();
            records
                .values()
                .chain(leaving.iter())
                .find(|record| {
                    let window = record.window.borrow();
                    ended(window.pane.as_ref())
                        || ended(window.opening.as_ref().map(|opening| &opening.pane))
                })
                .cloned()
        };
        let Some(record) = record else {
            // The host's word on a window the engine's own close settled already.
            return self.heard_exit(pane, &shown);
        };
        let conversation = record.window.borrow().conversation;
        record.window.borrow_mut().last_exit = Some((pane.clone(), conversation));
        // Still opening: its launch takes the exit once it has the window.
        if !ended(record.window.borrow().pane.as_ref()) {
            if let Some(opening) = record.window.borrow_mut().opening.as_mut() {
                opening.exited = Some(shown);
            }
            return Ok(());
        }
        // Whether its program is gone, kept on the conversation it wrote to for the next
        // launch on it, after a restart too (`ownership`, `Launch::previous_gone`).
        if let (Some(gone), Some(conversation)) = (shown.gone(), conversation) {
            self.keep_proof(record.id, conversation, gone, Some(&record))?;
        }
        let delivering = record.delivery.borrow_mut().delivering.take();
        // What a launch that was to say the role first still owed the window
        // that exited: the role it was never told.
        let owed = self.owed(&record)?;
        record.delivery.borrow_mut().prelude = None;
        // The task the window was given, whose requester hears what its close ended.
        let owner = record.window.borrow().owner;
        self.closed(&record);
        // No door survives its window: what one claimed and none acknowledged is the ledger's again.
        self.seams
            .ledger
            .borrow_mut()
            .release_claims(record.id, "its window exited")?;
        // A participant that left is forgotten already: its exit settles nothing more.
        let was_leaving = {
            let mut leaving = self.leaving.borrow_mut();
            let before = leaving.len();
            leaving.retain(|kept| !Rc::ptr_eq(kept, &record));
            leaving.len() != before
        };
        if was_leaving {
            let last = record.window.borrow().last_exit.clone();
            if let Some(last) = last {
                self.left_exits.borrow_mut().insert(record.id, last);
            }
            return Ok(());
        }
        let Some(project) = self.project_of(record.id)? else {
            return Ok(());
        };
        let Some(participant) = project
            .participants
            .iter()
            .find(|p| p.id == record.id)
            .cloned()
        else {
            return Ok(());
        };
        // A launch the exit ended says what its window showed.
        let at_launch = owed.is_some()
            || delivering
                .as_ref()
                .is_some_and(|delivering| delivering.launch);
        let closed = format!("@{}'s window closed", participant.handle);
        let because = if at_launch {
            self.unstarted_because(&record, &closed, &shown)
        } else {
            closed
        };
        let first = delivering
            .as_ref()
            .filter(|delivering| delivering.launch)
            .map(|delivering| delivering.message);
        let project = match delivering {
            None => project,
            Some(delivering) => {
                let launches = record.window.borrow().launches;
                // A paste its harness's record shows it took is confirmed (`in_flight`).
                let (this, told) = (Rc::clone(self), because.clone());
                self.settle_gone(delivering, move |delivering| {
                    // A chief's first message (a handoff, most often) waits for its next window.
                    if delivering.chief {
                        this.give_back(delivering.message, &told)
                    } else {
                        let retry = !delivering.launch;
                        this.settle_failure(delivering, &told, retry)
                    }
                })
                .await?;
                // The record may have been read with no window open: a participant that
                // moved on meanwhile settles nothing more of this exit, and one that did
                // not goes on with its project as it is now.
                match self.project_of(record.id)? {
                    Some(now) if self.still_as_it_went(&record, launches) => now,
                    _ => {
                        self.changed();
                        return Ok(());
                    }
                }
            }
        };
        // The role a launch said first may be told by now, its record read (`settle_gone`):
        // the launch came up, and its window went as any may (a review of 2026-10-16:
        // taken for a launch that never came up, a chief's exit left its project open and
        // its workers running, and with nothing else queued no launch followed).
        let said_first = owed.as_ref().map(|prelude| prelude.replacement);
        let owed = match owed {
            Some(prelude) => self.still_owed(prelude)?,
            None => None,
        };
        let launch_owed = owed.is_some();
        if let Some(prelude) = owed {
            self.settle_exited_launch(&record, &project, &participant, &prelude, &because)?;
        }
        if participant.role == "chief" {
            // The chief's own exit closes the project as Close does, but for the
            // window of a launch that was to say the role: it did not come up, and
            // is tried again (`settle_exited_launch`).
            let own = record.window.borrow().own_exit;
            if project.state == "open" && !own && !launch_owed {
                self.seams
                    .ledger
                    .borrow_mut()
                    .set_project_state(project.id, "suspended")?;
                // The human closed nothing: a chief that went before its first
                // message showed says why, where the host said. One whose record
                // shows it came up, and went as any chief's window may.
                let came_up = match first.or(said_first) {
                    Some(first) => self
                        .seams
                        .ledger
                        .borrow()
                        .message(first)?
                        .is_some_and(|message| message.state == "delivered"),
                    None => false,
                };
                if at_launch && !came_up && shown.is_known() {
                    self.tell_chief_closed(&project, &because)?;
                }
                let others: Vec<ParticipantView> = project
                    .participants
                    .iter()
                    .filter(|p| p.id != record.id)
                    .cloned()
                    .collect();
                // The work of the windows that go with it pauses, saying so: a worker's
                // own crash and its chief's read the same otherwise.
                let with = format!(
                    "with the project when @{}'s window closed",
                    participant.handle
                );
                self.close_windows(project.id, &others, Some(&shown.after(&with)))
                    .await?;
            }
        } else {
            let task = self.seams.ledger.borrow().active_task(record.id, false)?;
            // What the host lists is the turn's work, by its own proof, as it was when the
            // window closed.
            let left = shown.left_running();
            if let Some(thread) = task {
                // The pause note says how the window ended and what it showed last, as
                // far as the host said, and what the close ended, where there was any:
                // a harness that crashed under its task says why on its screen. A window
                // the engine closed says why it did, where it was not the human's Close,
                // and nothing of its end, which was the kill's (the host's exit can come
                // while the kill is answered).
                let besides = if left.is_empty() {
                    String::new()
                } else {
                    format!(" with background work still running: {}", quote(left))
                };
                // A window the engine closed itself: in a suspended project it closed
                // with the project (the human's Close, or the chief's window ending),
                // and the ledger marks the pause for the human's Resume to end. One
                // that closed by itself stays paused.
                let (own, with) = {
                    let window = record.window.borrow();
                    (window.own_exit, window.closed_with.clone())
                };
                let closed = match with {
                    Some(with) => format!("@{}'s window closed {with}", participant.handle),
                    None => format!("@{}'s window closed", participant.handle),
                };
                let because = if own {
                    let outlived = shown.outlived().unwrap_or_default();
                    format!("{closed}{outlived}{besides}")
                } else {
                    shown.after_with(&closed, &besides)
                };
                self.stall(&project, &thread.task, &because, own)?;
                // Told while the engine's kill waits for its answer, which decides
                // whether this exit was the kill's (`close_own`).
                if let Some(killing) = record.window.borrow_mut().killing.as_mut() {
                    killing.paused = Some((project.id, thread.task.number));
                }
            } else {
                let given = match owner {
                    Some(number) => self.seams.ledger.borrow().task(project.id, number)?,
                    None => None,
                };
                self.tell_left_running(&project, given.as_ref(), left)?;
            }
        }
        self.changed();
        Ok(())
    }

    /// What the host said of the end of `pane`, where its window was settled
    /// before (the engine's own close, which knows nothing of how it ended): the
    /// conversation that window wrote to keeps whether its program is gone, where
    /// the host said ([`Dispatcher::keep_proof`]).
    fn heard_exit(&self, pane: &Pane, shown: &Shown) -> Result<(), EngineError> {
        let Some(gone) = shown.gone() else {
            return Ok(());
        };
        let mut whose: Vec<(i64, Option<i64>)> = self
            .left_exits
            .borrow()
            .iter()
            .filter(|(_, (last, _))| last == pane)
            .map(|(id, (_, conversation))| (*id, *conversation))
            .collect();
        whose.extend(self.records.borrow().values().filter_map(|record| {
            let window = record.window.borrow();
            let (last, conversation) = window.last_exit.as_ref()?;
            (last == pane).then_some((record.id, *conversation))
        }));
        for (id, conversation) in whose {
            if let Some(conversation) = conversation {
                self.keep_proof(id, conversation, gone, None)?;
            }
        }
        Ok(())
    }

    /// Whether the program of a window of participant `id` that ended is gone, kept on
    /// `conversation`, the one it wrote to. Not where another window of that
    /// participant has opened, or is opening, on that conversation since (`ended` is the
    /// record of the one that ended, which still holds its pane): that window's program
    /// may write to it now, and its own exit says whether it is gone (Calliope's P3-1 on
    /// 0181cd9a). A window on another conversation writes nothing to this one (her P3-1 on
    /// 46664f52: a session brought back on a new conversation lost its own window's proof
    /// while the deleted session's window still waited for its kill).
    fn keep_proof(
        &self,
        id: i64,
        conversation: i64,
        gone: bool,
        ended: Option<&Rc<Record>>,
    ) -> Result<(), EngineError> {
        let windowed = {
            let records = self.records.borrow();
            let leaving = self.leaving.borrow();
            records
                .values()
                .chain(leaving.iter())
                .filter(|record| record.id == id)
                .filter(|record| ended.is_none_or(|ended| !Rc::ptr_eq(record, ended)))
                .any(|record| {
                    let window = record.window.borrow();
                    let on = Some(conversation);
                    (window.pane.is_some() && window.conversation == on)
                        || window
                            .opening
                            .as_ref()
                            .is_some_and(|opening| opening.conversation == on)
                })
        };
        if !windowed {
            self.seams
                .ledger
                .borrow_mut()
                .set_program_gone(conversation, gone)?;
        }
        Ok(())
    }

    /// One participant's step.
    async fn step(
        self: &Rc<Self>,
        project: &ProjectView,
        participant: &ParticipantView,
    ) -> Result<(), EngineError> {
        let record = self.record_of(participant.id);
        if record.window.borrow().pane.is_some() {
            return self.step_open(project, participant, &record).await;
        }
        if project.state != "open" {
            return Ok(());
        }
        // A chief whose agent is gone stays closed: its launch told the human, who switches it.
        if participant.role == "chief" && self.agent_gone(participant) {
            return Ok(());
        }
        // A chief whose window keeps failing to start is tried again ever more slowly.
        let relaunch_at = record.window.borrow().relaunch.map(|relaunch| relaunch.at);
        if relaunch_at.is_some_and(|at| at > self.now()) {
            return Ok(());
        }
        let next = self.seams.ledger.borrow().next_delivery(participant.id)?;
        // A chief whose window was given up opens again at its time whatever waits for
        // it: that window may have begun a turn as it was closed, on a first message its
        // record then holds and nothing gives again, and one closed with nothing left
        // queued stayed closed (Sol and Astra on round 2 of the chief's restart). A turn
        // its record holds with no end is told as cut, once, as the human's Resume tells
        // one the app's quit cut, and the chief goes on from it.
        let reopens = participant.role == "chief"
            && record
                .window
                .borrow()
                .relaunch
                .is_some_and(|relaunch| relaunch.reopens);
        let first = if reopens {
            self.cut_note(project, participant, &Coming::Chief)
                .await?
                .or(next)
        } else {
            next
        };
        if first.is_some() || reopens {
            let (this, held, at, who) = (
                Rc::clone(self),
                Rc::clone(&record),
                project.clone(),
                participant.clone(),
            );
            self.act(&record, async move {
                this.launch(&held, &at, &who, first).await
            })
            .await;
            return Ok(());
        }
        if participant.role == "chief" {
            return Ok(());
        }
        // A member whose agent is gone must not wait for a window that will not open.
        if self.agent_gone(participant) {
            return self.without_agent(project, participant, None);
        }
        // A member's session is its task's: with the window gone and nothing due to it, nobody is doing the work.
        let task = self
            .seams
            .ledger
            .borrow()
            .active_task(participant.id, false)?;
        if let Some(thread) = task {
            let because = format!("@{}'s window is gone", participant.handle);
            self.stall(project, &thread.task, &because, false)?;
            self.changed();
        }
        Ok(())
    }

    async fn step_open(
        self: &Rc<Self>,
        project: &ProjectView,
        participant: &ParticipantView,
        record: &Rc<Record>,
    ) -> Result<(), EngineError> {
        if record.window.borrow().retiring {
            return Ok(());
        }
        // A participant forgotten while the step waits is done with.
        let observed = match returning(self.observe(participant, record)).await {
            Ok(observed) => observed,
            Err(reason) => {
                if !self.forgotten(record) {
                    self.set_activity(record, Activity::because(ActivityState::Unknown, reason))?;
                }
                return Ok(());
            }
        };
        // Quota belongs to the member: a session that runs out takes its member out.
        let owner = self.member_of(project, participant);
        // A window with nothing in its record may still be drawing its screen.
        let unnamed = observed.unnamed;
        if !unnamed {
            record.window.borrow_mut().named = true;
        }
        // One whose harness may ask, or asks, whether to trust its folder is starting
        // still: nothing goes into it, as a paste's Enter would answer the question
        // (`trust`).
        let asking = record.window.borrow().trust != Trusting::Past;
        let drawing = asking
            || ((observed.items().is_empty() || unnamed) && !returning(self.drawn(record)).await);
        if self.forgotten(record) {
            return Ok(());
        }
        let starting = drawing || (unnamed && !record.window.borrow().named);
        // An out member's window says so, and nothing else, until the reset.
        if !self.is_out(&owner) {
            // The reason is given once the question is on its screen.
            let activity = if record.window.borrow().trust == Trusting::Asked {
                Activity::because(ActivityState::Starting, trust::HELD)
            } else if starting {
                Activity::of(ActivityState::Starting)
            } else if let Some(waiting) = &observed.waiting {
                Activity {
                    state: ActivityState::Waiting,
                    reason: waiting.reason.clone(),
                }
            } else if observed.settled {
                Activity::of(ActivityState::Idle)
            } else {
                Activity::of(ActivityState::Working)
            };
            self.set_activity(record, activity)?;
        }
        self.copy(participant, record, &observed)?;
        // The human switched the window to another conversation: this look was the old one's last.
        if let Some(session) = &observed.switched {
            if record.delivery.borrow().delivering.is_some() {
                self.confirm_arrival(record, &observed)?;
            }
            return self.follow(participant, record, session);
        }
        self.record_quota(record, &owner, observed.quota.clone());
        if self.refused_here(record, &owner) {
            return self
                .out_of_quota(project, participant, record, &owner)
                .await;
        }
        let mut out = self.is_out(&owner);
        // A window that got a turn through after its member was marked out says the quota is back.
        if out && self.answered_since(&owner, &observed) {
            self.seams
                .ledger
                .borrow_mut()
                .mark_back(owner.id, &format!("@{} answered again", participant.handle))?;
            self.changed();
            out = false;
        }
        if out {
            return self
                .while_out(project, participant, record, &owner, &observed, starting)
                .await;
        }
        if record.delivery.borrow().delivering.is_some() {
            returning(self.watch_arrival(record, &observed)).await?;
        }
        // What the launch owes the window (the role it was to say first) is
        // watched as a first message is, and a launch given up closes its window.
        if self.watch_prelude(project, participant, record).await? {
            return Ok(());
        }
        // A window that began to close in this step is not acted on.
        if record.window.borrow().retiring {
            return Ok(());
        }
        if participant.role != "chief" {
            returning(self.settle_stop(project, participant, record, &observed, starting)).await?;
            if self.forgotten(record) {
                return Ok(());
            }
            // A turn a refusal cut short, its member past it now, failed nothing: its task goes on.
            if self.cut_short(&observed) {
                self.go_on(project, participant)?;
            } else {
                // Its answer is its result, recorded now, unless its harness says the turn
                // left work running that wakes its agent: then the task goes on. Nothing the
                // turn left is ended; the window closes below if it is free (`turn_end`).
                let watching = self.watch_monitors(participant, record, &observed).await;
                self.collect(project, participant, record, &observed, watching)?;
            }
            if self.close_if_free(record).await? {
                return Ok(());
            }
        }
        let free = record.delivery.borrow().delivering.is_none()
            && observed.settled
            && observed.waiting.is_none();
        let idle = free && !drawing;
        if record.pending_switch.borrow().is_some() {
            return self
                .await_switch(project, participant, record, &observed, idle)
                .await;
        }
        // A closed project's window that its Close could not close is given nothing
        // until the Resume, as no closed project is: what waits for it is the Resume's
        // to settle first (Sol's review of 2026-10-16: the note that the work was
        // paused went into the chief's window and reached it after the Resume had
        // the work at work again).
        if idle && project.state == "open" {
            // A chief at rest may be due a wake-up: queued here, it is the next
            // message of its queue, delivered by the rules of every message.
            if participant.role == "chief" {
                self.queue_wake_up_if_due(project, participant)?;
            }
            let next = self.next_in_line(record)?;
            if let Some(next) = next {
                // Another task's message goes into a window of its own (`ownership`).
                if let Some(owner) = self.barred(participant, record, &next) {
                    return self
                        .keep_apart(project, participant, record, &next, owner)
                        .await;
                }
                let (this, held) = (Rc::clone(self), Rc::clone(record));
                self.act(record, async move { this.deliver(&held, next).await })
                    .await;
            }
        } else if free && self.ready_while_drawing(record) {
            // Nothing goes into a window that has not drawn, but one whose readiness
            // has a bound of its own is asked, and given up where it says it never
            // will be fit (`give_up_unfit_drawing`).
            if let Some(next) = self.next_in_line(record)? {
                let (this, held) = (Rc::clone(self), Rc::clone(record));
                self.act(record, async move {
                    this.give_up_unfit_drawing(&held, next).await
                })
                .await;
            }
        }
        Ok(())
    }

    /// A window of a member out of quota: a switch the human asked for goes
    /// now; otherwise it says it is out, and a held task's agent stops: by what
    /// its look found it doing, whatever the board says it is.
    async fn while_out(
        self: &Rc<Self>,
        project: &ProjectView,
        participant: &ParticipantView,
        record: &Rc<Record>,
        owner: &ParticipantView,
        observed: &Observed,
        starting: bool,
    ) -> Result<(), EngineError> {
        let pending = record
            .pending_switch
            .borrow()
            .as_ref()
            .map(|pending| pending.to.clone());
        // A closed project's switch waits for the Resume, as `await_switch`'s does.
        if let Some(to) = pending.filter(|_| project.state == "open") {
            return self.perform_switch(project, participant, record, to).await;
        }
        let until = owner.out_until.clone().unwrap_or_default();
        self.set_activity(
            record,
            Activity::because(ActivityState::Out, format!("out of quota until {until}")),
        )?;
        // The time a launch waits while its member is out is no slowness of its
        // window: the role it was to say first is told when the quota is back.
        if let Some(prelude) = record.delivery.borrow_mut().prelude.as_mut() {
            prelude.since = self.now();
        }
        if participant.role != "chief" {
            returning(self.settle_stop(project, participant, record, observed, starting)).await?;
            if !self.forgotten(record) {
                self.close_if_free(record).await?;
            }
        }
        Ok(())
    }

    /// The human's Switch chief: the chief goes on in a fresh window on the
    /// saved `agent` on `harness`, its first message the handoff.
    pub async fn switch_chief(
        self: &Rc<Self>,
        project: i64,
        to: SwitchTo,
        when: SwitchWhen,
        note: bool,
    ) -> Result<Option<ProjectView>, EngineError> {
        self.require_chief(&to.harness, Some(&to.agent))?;
        let found = self.known_project(project)?;
        let chief = found
            .participants
            .iter()
            .find(|p| p.role == "chief")
            .cloned()
            .ok_or_else(|| {
                EngineError::said("no-chief", format!("no chief in project {project}"))
            })?;
        let record = self.record_of(chief.id);
        let (this, held) = (Rc::clone(self), Rc::clone(&record));
        let work = async move {
            // Asked of the project as it is once the chief's step in progress is over.
            let now = this.known_project(project)?;
            require_open(&now)?;
            let open = held.window.borrow().pane.is_some();
            if open && !this.is_out(&chief) && (when == SwitchWhen::Turn || note) {
                return this.after_turn(project, &held, to, note);
            }
            this.perform_switch(&found, &chief, &held, to).await?;
            // One deleted while the old window was looked at or closed stopped the switch there.
            if this.forgotten(&held) {
                return Err(EngineError::said(
                    "no-project",
                    format!("no project {project}"),
                ));
            }
            if held.pending_switch.borrow().is_some() {
                return Err(EngineError::said(
                    "window-kept",
                    "the chief's window would not close: the switch waits, and is tried again after its turn",
                ));
            }
            Ok(())
        };
        self.exclusive(&record, work).await.await?;
        self.project_now(project)
    }

    /// A window whose harness just refused it: its member is out until the
    /// reset; what the window was receiving is queued again, its work goes
    /// back to the board or waits with it.
    async fn out_of_quota(
        self: &Rc<Self>,
        project: &ProjectView,
        participant: &ParticipantView,
        record: &Rc<Record>,
        owner: &ParticipantView,
    ) -> Result<(), EngineError> {
        self.handled(record);
        let reported = record.quota.borrow().reported.clone();
        let reset = self.reset_of(reported.as_deref());
        let marked = self
            .seams
            .ledger
            .borrow_mut()
            .mark_out(owner.id, &reset, "out of quota")?;
        let until = marked.out_until.clone().unwrap_or_default();
        self.set_activity(
            record,
            Activity::because(ActivityState::Out, format!("out of quota until {until}")),
        )?;
        let delivering = record.delivery.borrow_mut().delivering.take();
        if let Some(delivering) = delivering {
            self.settle_failure(delivering, "the harness ran out of quota", true)?;
        }
        self.hold_or_release(project, participant, owner, &until)?;
        if participant.role != "chief" {
            self.close_if_free(record).await?;
        }
        self.changed();
        Ok(())
    }

    // --- small helpers ---------------------------------------------------------------

    /// Runs `work` holding `record`'s participant once its turn comes: its
    /// place in the queue taken now, its answer through the [`Begun`].
    pub(crate) async fn exclusive<T: 'static>(
        &self,
        record: &Rc<Record>,
        work: impl Future<Output = Result<T, EngineError>> + 'static,
    ) -> Begun<Result<T, EngineError>> {
        record
            .hold
            .exclusive(&*self.seams.spawn, Box::pin(work))
            .await
    }

    /// Runs `work` holding `record`'s participant if nothing holds it now:
    /// none when it is held, as a pass moves on.
    pub(crate) async fn try_exclusive<T: 'static>(
        &self,
        record: &Rc<Record>,
        work: impl Future<Output = Result<T, EngineError>> + 'static,
    ) -> Option<Begun<Result<T, EngineError>>> {
        record
            .hold
            .try_exclusive(&*self.seams.spawn, Box::pin(work))
            .await
    }

    /// Begins a launch or a delivery apart from the pass, holding its
    /// participant; nobody waits for it, so a failure is written down.
    pub(crate) async fn act(
        self: &Rc<Self>,
        record: &Rc<Record>,
        work: impl Future<Output = Result<(), EngineError>> + 'static,
    ) {
        let this = Rc::clone(self);
        let work = Box::pin(work);
        let work = async move {
            if let Err(cause) = work.await {
                this.write_down(&cause);
            }
        };
        record.hold.act(&*self.seams.spawn, work).await;
    }

    /// What failed apart from any pass or request is written down.
    pub(crate) fn write_down(&self, cause: &EngineError) {
        self.seams
            .log
            .error("a launch or a delivery failed", &cause.to_string());
    }

    /// Opens a participant's window once its step in progress is over, apart
    /// from whoever asked. A window open by then, a project closed
    /// meanwhile, or a participant forgotten meanwhile opens nothing.
    ///
    /// `coming` is for a window the human's Resume brings back (`resume`): it is
    /// told, as the first thing it is given, that its turn was cut, if it was
    /// (after the note that says its role again, where it is to be told that:
    /// `role_change`).
    pub(crate) async fn open_soon(self: &Rc<Self>, participant: i64, coming: Option<Coming>) {
        let record = self.record_of(participant);
        // Asked for now, before anything is waited on: an earlier window's exit that
        // ends meanwhile settles nothing more (`still_as_it_went`).
        record.window.borrow_mut().launches += 1;
        let (this, held) = (Rc::clone(self), Rc::clone(&record));
        let work = async move {
            let project = this.project_of(participant)?;
            let open = held.window.borrow().pane.is_some();
            let Some(project) = project.filter(|project| !open && project.state == "open") else {
                return Ok(());
            };
            let Some(who) = project
                .participants
                .iter()
                .find(|p| p.id == participant)
                .cloned()
            else {
                return Ok(());
            };
            // A window the Resume brings back opens only for work still at work once
            // its cut was read: a stop asked of it meanwhile (a pause, a cancel, a
            // release or an end of its task) leaves it closed, and its task with the
            // chief (Astra's review of 2026-10-16, round 4). What moves after is the
            // launch's: the move withdraws the continuation (the ledger's), and the
            // launch watches the stops its window pays.
            let first = match &coming {
                Some(coming) => {
                    let note = this.cut_note(&project, &who, coming).await?;
                    if !this.still_coming(&who, coming)? {
                        return Ok(());
                    }
                    note
                }
                None => None,
            };
            let (launching, at) = (Rc::clone(&this), Rc::clone(&held));
            this.act(&held, async move {
                launching.launch(&at, &project, &who, first).await
            })
            .await;
            Ok(())
        };
        let this = Rc::clone(self);
        let opening = async move {
            if let Err(cause) = this.exclusive(&record, work).await.await {
                this.write_down(&cause);
            }
        };
        drop(begin(&*self.seams.spawn, opening).await);
    }

    /// A participant's record, made the first time it is asked for.
    pub(crate) fn record_of(&self, participant: i64) -> Rc<Record> {
        Rc::clone(
            self.records
                .borrow_mut()
                .entry(participant)
                .or_insert_with(|| {
                    let record = Record::new(participant);
                    record.window.borrow_mut().last_exit =
                        self.left_exits.borrow_mut().remove(&participant);
                    record
                }),
        )
    }

    /// A participant's record when it has one; asking makes none.
    pub(crate) fn record(&self, participant: i64) -> Option<Rc<Record>> {
        self.records.borrow().get(&participant).cloned()
    }

    /// Whether a record was forgotten since work took it: its participant
    /// left, or its project was deleted. That work does nothing more by the id.
    pub(crate) fn forgotten(&self, record: &Rc<Record>) -> bool {
        self.records
            .borrow()
            .get(&record.id)
            .is_none_or(|kept| !Rc::ptr_eq(kept, record))
    }

    /// Tells the trace what happened at a window, named by its project and
    /// participant. A ledger that cannot be read says nothing is traced, and
    /// why: the step or the operation that was telling it fails.
    pub(crate) fn trace_window(
        &self,
        record: &Record,
        event: WindowEvent,
    ) -> Result<(), EngineError> {
        let project = self.project_of(record.id)?;
        let participant = project.as_ref().and_then(|project| {
            project
                .participants
                .iter()
                .find(|p| p.id == record.id)
                .map(|p| p.handle.clone())
        });
        self.seams.trace.line(TraceLine {
            at: iso(self.now()),
            what: Traced::Window {
                project: project.map(|project| project.id),
                participant,
                event,
            },
        });
        Ok(())
    }

    pub(crate) fn project_of(&self, participant: i64) -> Result<Option<ProjectView>, EngineError> {
        Ok(self
            .seams
            .ledger
            .borrow()
            .projects()?
            .into_iter()
            .find(|project| project.participants.iter().any(|p| p.id == participant)))
    }

    pub(crate) fn now(&self) -> i64 {
        self.seams.time.wall_ms()
    }

    pub(crate) fn changed(&self) {
        let listeners: Vec<Rc<dyn Fn()>> = self.listeners.borrow().clone();
        for listener in listeners {
            listener();
        }
    }

    /// A look copied something new of what a window wrote: each view of a
    /// window's work hears it ([`Dispatcher::on_transcript`]).
    pub(crate) fn wrote(&self) {
        let listeners: Vec<Rc<dyn Fn()>> = self.transcript_listeners.borrow().clone();
        for listener in listeners {
            listener();
        }
    }
}

#[cfg(test)]
mod tests;

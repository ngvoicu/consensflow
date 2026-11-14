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
//! message (`role_change`), and a chief at rest whose specs or rules for the staff the
//! human changed under its open window, which it is told in a note (`role_news`).
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
use cf_ledger::{
    DeletedProject, HoldKind, NewMember, NewProject, ParticipantView, ProjectView, RemovedMember,
    TaskReleased,
};
use cf_proto::panes::PaneExit;
use cf_proto::trace::{TraceLine, Traced, WindowEvent};

use crate::background::leaving;
use crate::chief_model::held_for_no_default;
use crate::chief_switch::SwitchTo;
use crate::computer_use::ComputerUseState;
use crate::pacing;
use crate::privacy::PrivacyState;
use crate::record::Record;
use crate::resume::{Closing, Closings, Coming, ResumeState};
use crate::runtime::{all, begin, returning, Begun, LocalWork};
use crate::scheduler::{busy_turn, moved_hold, out_words, Held, News, SchedulerState, Unwell};
use crate::seams::{EngineError, Seams};
use crate::shown::{Part, Shown};
use crate::stalls::StallsState;
use crate::steps::{Step, StepsState};
use crate::trust::{self, TrustState, Trusting};
use crate::turn_end::{last_paste, lost_in_turn, Turn};
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

/// A chief runs on a harness and a model of it the human picked (CHF-04): asked for
/// without a model, the daemon says what to pick.
pub fn require_chief_model(model: Option<&str>) -> Result<&str, EngineError> {
    model.filter(|model| !model.is_empty()).ok_or_else(|| {
        EngineError::said(
            "no-chief-model",
            "pick a harness for the chief, and one of its models",
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
    /// The missing permissions each project's notes spoke of already (`computer_use`).
    pub(crate) computer_use: ComputerUseState,
    /// What the passes keep of the steps they went on without (`steps`).
    pub(crate) steps: StepsState,
    /// What was told of macOS privacy dialogs raised in the app's name (`privacy`).
    pub(crate) privacy: PrivacyState,
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
            computer_use: ComputerUseState::default(),
            steps: StepsState::default(),
            privacy: PrivacyState::default(),
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

    /// Why the message at the head of `participant`'s window's queue does not go in now,
    /// where it is held (by the pacing, the window's readiness, or a box to empty): the
    /// one place that says it (T-842's P2). None where nothing is held.
    pub fn held_back(&self, participant: i64) -> Option<crate::deliveries::HeldBack> {
        self.record(participant)
            .and_then(|record| record.delivery.borrow().held.clone())
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

    /// The harness the participant's live window was launched with: what it runs, whatever
    /// the participant is configured to run now (a member edited while its window is open runs
    /// the new harness from its next window, T-1143). None while it has no window, or its
    /// launch did not say.
    pub fn window_harness(&self, participant: i64) -> Option<String> {
        self.record(participant).and_then(|record| {
            let part = record.window.borrow();
            part.pane.as_ref()?;
            // The window keeps the name its programs go by (`claude`); the board says `claude-code`.
            cf_proto::agents::Harness::ALL
                .into_iter()
                .find(|harness| harness.as_str() == part.harness)
                .map(|harness| harness.kind().to_owned())
        })
    }

    /// Whether the participant has a window whose open went unanswered and which the
    /// host has not said it ended ([`crate::windows::WindowPart::abandoned`]).
    fn abandoned(&self, participant: i64) -> bool {
        self.record(participant)
            .is_some_and(|record| record.window.borrow().abandoned.is_some())
    }

    /// Refuses a harness ConsensFlow has no adapter for.
    pub fn requires_adapter(&self, harness: &str) -> Result<(), EngineError> {
        self.require_adapter(harness)
    }

    /// The chief asked for: a harness whose windows ConsensFlow opens, and a model of
    /// it. Whether the harness lists that model is the daemon's to ask, which reads the
    /// lists ([`cf_proto::models`]).
    fn require_chief(&self, harness: &str, model: Option<&str>) -> Result<(), EngineError> {
        let model = require_chief_model(model)?;
        self.require_adapter(harness)?;
        cf_ledger::model::require_model_id(model)?;
        Ok(())
    }

    /// A new project: the ledger records it with its chief, on the harness and the
    /// model it names, and its staff, and with the wake-up calls and the specs
    /// the request says, all in one write; its chief window opens after the
    /// answer, on the project as that write left it, so its first launch has
    /// the specs.
    pub async fn open_project(
        self: &Rc<Self>,
        request: NewProject,
    ) -> Result<Option<ProjectView>, EngineError> {
        self.require_chief(&request.chief.harness, request.chief.model.as_deref())?;
        for member in &request.staff {
            self.require_adapter(&member.harness)?;
        }
        let project = self.seams.ledger.borrow_mut().create_project(&request)?;
        if let Some(chief) = project.participants.iter().find(|p| p.handle == "chief") {
            self.open_soon(chief.id, None).await;
        }
        self.project_now(project.id)
    }

    /// Open a project's validated staff while preserving its source backup links.
    pub async fn open_project_with_staff(
        self: &Rc<Self>,
        request: NewProject,
        source: serde_json::Value,
    ) -> Result<Option<ProjectView>, EngineError> {
        self.require_chief(&request.chief.harness, request.chief.model.as_deref())?;
        for member in &request.staff {
            self.require_adapter(&member.harness)?;
        }
        let project = self
            .seams
            .ledger
            .borrow_mut()
            .create_project_with_staff(&source)?;
        if let Some(chief) = project.participants.iter().find(|p| p.handle == "chief") {
            self.open_soon(chief.id, None).await;
        }
        self.project_now(project.id)
    }

    /// The human's Close: the project is suspended and every window of it goes. It is the
    /// human's own shutdown, no pause: a window kept because it ignored a pause's rounds goes
    /// with the rest (T-1199: a pause never closes a window, and the Close and the app's quit
    /// still close them all; the task stays paused for the project's Resume).
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

    /// The human says a member, or the chief, out of quota is back before its reset: the
    /// login works again, for every member on it in every project. A member whose model is
    /// busy for a moment is none of that (T-1357): it is left to its back-off, and the
    /// answer says so, so a stale row's button, or a call of the API, spends no try of its
    /// task for nothing; the others on its login are cleared all the same.
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
        let busy = self.model_busy_until(&member);
        let back = if busy.is_some() {
            member.clone()
        } else {
            self.seams
                .ledger
                .borrow_mut()
                .mark_back(member.id, "by @human")?
        };
        // The quota the human renewed is the account's: every project's members on the
        // harness's one login are back (T-1357), this project's first.
        let projects: Vec<i64> = self
            .seams
            .ledger
            .borrow()
            .projects()?
            .into_iter()
            .map(|other| other.id)
            .collect();
        self.back_on_its_account(project, &member, &News::Human)?;
        for other in projects.into_iter().filter(|other| *other != project) {
            self.back_on_its_account(other, &member, &News::Human)?;
        }
        self.changed();
        match busy {
            Some(until) => Err(EngineError::said(
                "model-busy",
                format!("Its model is busy: ConsensFlow tries it again at {until}"),
            )),
            None => Ok(back),
        }
    }

    /// The member and the sessions of `member` in `found`: what leaves when it does.
    fn rows_of(&self, found: &ProjectView, member: i64) -> Vec<i64> {
        std::iter::once(member)
            .chain(
                found
                    .participants
                    .iter()
                    .filter(|p| p.member_id == Some(member))
                    .map(|p| p.id),
            )
            .collect()
    }

    /// The rows of the backup `member` has now, with its sessions: what leaves with it.
    fn backup_rows(&self, found: &ProjectView, member: i64) -> Vec<i64> {
        found
            .participants
            .iter()
            .filter(|p| p.backup_of == Some(member) && p.left_at.is_none())
            .flat_map(|backup| self.rows_of(found, backup.id))
            .collect()
    }

    /// A member's backup is taken off the staff, and its sessions and the tasks in them go as
    /// a member's own do: their windows close.
    pub async fn remove_backup(
        self: &Rc<Self>,
        project: i64,
        handle: &str,
    ) -> Result<ParticipantView, EngineError> {
        let before = self.known_project(project)?;
        let removed = self
            .seams
            .ledger
            .borrow_mut()
            .remove_backup(project, handle)?;
        self.forget(&self.rows_of(&before, removed.id)).await?;
        self.changed();
        Ok(removed)
    }

    /// A member's backup is replaced by another: the old one's windows close as it leaves.
    pub async fn change_backup(
        self: &Rc<Self>,
        project: i64,
        handle: &str,
        backup: &NewMember,
    ) -> Result<ParticipantView, EngineError> {
        let before = self.known_project(project)?;
        let old = before
            .participants
            .iter()
            .find(|p| p.handle == handle)
            .map(|member| self.backup_rows(&before, member.id))
            .unwrap_or_default();
        let added = self
            .seams
            .ledger
            .borrow_mut()
            .change_backup(project, handle, backup)?;
        self.forget(&old).await?;
        self.changed();
        Ok(added)
    }

    /// The human sends a member, or a backup, on Vacation: it takes no task from now on. A task
    /// it was given whose window never opened goes back to the board for another member; one in
    /// a window it has, or was named for it, stays where it is. A session whose window may be
    /// opening is proved unopened only while its step is held, so a launch that began is never
    /// taken from under its window.
    pub async fn send_on_vacation(
        self: &Rc<Self>,
        project: i64,
        handle: &str,
    ) -> Result<ParticipantView, EngineError> {
        let found = self.known_project(project)?;
        let member = found
            .participants
            .iter()
            .find(|p| p.handle == handle && p.role != "human")
            .ok_or_else(|| {
                EngineError::said(
                    "no-participant",
                    format!("no @{handle} in project {project}"),
                )
            })?;
        // The mark first: nothing is given it from here on, and the sessions are looked at one
        // by one while their steps are held.
        let sent = self
            .seams
            .ledger
            .borrow_mut()
            .send_on_vacation(project, handle, &[])?;
        self.release_vacation_sessions(project, &found, member.id, handle)
            .await?;
        self.changed();
        Ok(self
            .project_now(project)?
            .and_then(|now| now.participants.into_iter().find(|p| p.id == sent.id))
            .unwrap_or(sent))
    }

    /// The never-opened tasks of a member's sessions on Vacation go back to the board, each
    /// session looked at while its step is held. It sets no mark and does nothing for a member
    /// brought back meanwhile, and it is safe to run again (the Resume runs it for a member
    /// whose Vacation the app's stop cut short).
    pub(crate) async fn release_vacation_sessions(
        self: &Rc<Self>,
        project: i64,
        found: &ProjectView,
        member: i64,
        handle: &str,
    ) -> Result<(), EngineError> {
        for session in found
            .participants
            .iter()
            .filter(|p| p.member_id == Some(member))
            .map(|p| p.id)
        {
            let record = self.record_of(session);
            let (this, held, handle) = (Rc::clone(self), Rc::clone(&record), handle.to_owned());
            let work = async move {
                if this.forgotten(&held) || !this.unopened(&held) {
                    return Ok(());
                }
                this.seams
                    .ledger
                    .borrow_mut()
                    .release_unopened(project, &handle, &[session])?;
                Ok::<(), EngineError>(())
            };
            self.exclusive(&record, work).await.await?;
        }
        Ok(())
    }

    /// Whether a session has never had a window: none open, none being opened or ended, and no
    /// message on its way. Asked only while its step is held.
    fn unopened(&self, record: &Record) -> bool {
        record.window.borrow().pane.is_none()
            && record.window.borrow().abandoned.is_none()
            && record.delivery.borrow().delivering.is_none()
    }

    /// The human brings a member, or a backup, back from Vacation: its quota hold is as it
    /// was, and the next pass gives it tasks again.
    pub fn bring_back_from_vacation(
        &self,
        project: i64,
        handle: &str,
    ) -> Result<ParticipantView, EngineError> {
        let back = self.seams.ledger.borrow_mut().bring_back(project, handle)?;
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
            let found = this.known_project(project)?;
            let mut left = this.rows_of(&found, member.id);
            // A backup leaves with its member, and so do its sessions.
            left.extend(this.backup_rows(&found, member.id));
            let removed = this
                .seams
                .ledger
                .borrow_mut()
                .remove_member(project, &handle)?;
            // What the ledger says departed is what is forgotten, with what was seen before.
            left.extend(removed.departed.iter().copied());
            left.sort_unstable();
            left.dedup();
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
    /// It waits for its steps [`STEP_WAIT`] at most (`steps`).
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
        let mut steps = Vec::new();
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
                    && self.pane(participant.id).is_none()
                    && !self.abandoned(participant.id);
                if idle {
                    continue;
                }
                let record = self.record_of(participant.id);
                let (this, at, who) = (Rc::clone(self), Rc::clone(&project), participant.clone());
                // Boxed where it is made: a step holds every wait below it, and
                // moved by value through the hold it filled the stack.
                let step = Box::pin(async move { this.step(&at, &who).await });
                if let Some(begun) = self.try_exclusive(&record, step).await {
                    steps.push(Step {
                        handle: participant.handle.clone(),
                        record,
                        begun,
                    });
                }
            }
        }
        self.wait_for_steps(steps).await
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
        // The conversation the window ran in: the id its harness may have printed as it quit is
        // no secret in the window's own note (`Shown::with_own_session`).
        let shown = shown.with_own_session(self.native_session_of(record.id));
        // Whether its program is gone, kept on the conversation it wrote to for the next
        // launch on it, after a restart too (`ownership`, `Launch::previous_gone`).
        if let (Some(gone), Some(conversation)) = (shown.gone(), conversation) {
            self.keep_proof(record.id, conversation, gone, Some(&record))?;
        }
        let delivering = record.delivery.borrow_mut().delivering.take();
        // What was pasted into the window, read before a delivery it did not take goes back
        // to its queue: its screen may show it (Calliope's P2c, T-994).
        let pasted = self.pastes_on_screen(record.id)?;
        // What a launch that was to say the role first still owed the window
        // that exited: the role it was never told.
        let owed = self.owed(&record)?;
        {
            let mut delivery = record.delivery.borrow_mut();
            delivery.prelude = None;
            delivery.for_human = None;
            // A greeting still to go was the exited window's session's: the next launch
            // withdraws it with every role note that waits (`withdraw_replacements`).
            delivery.greeting = None;
        }
        // The task the window was given, whose requester hears what its close ended.
        let owner = record.window.borrow().owner;
        let said_yes = trust::yes_at_exit(&record, &shown);
        // What the engine had sent this window, told before `closed` clears it.
        let sent = record.window.borrow().sent_to.contains(pane);
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
        // The human's yes in a window that was gone before the watch looked again is still
        // theirs for the project's other windows that ask (`trust::yes_at_exit`).
        if let Some((folder, sure)) = said_yes {
            self.trust.said_yes(project.id, &folder, sure);
        }
        // A launch the exit ended says what its window showed.
        let at_launch = owed.is_some()
            || delivering
                .as_ref()
                .is_some_and(|delivering| delivering.launch);
        // A window whose program is ConsensFlow's own, standing in for its harness's, and which
        // could not start the harness, ended with a status that says so (Codex's): its note says
        // that, and not what its screen showed (`Adapter::never_ran`, `Shown::with_start`).
        let harness = participant.harness.as_deref().unwrap_or_default();
        let start = shown.exit_status().and_then(|status| {
            self.seams
                .adapters
                .adapter(harness)
                .and_then(|adapter| adapter.never_ran(status))
        });
        let who = crate::handoff::name_of(harness);
        // The note says the kind and nothing the window showed; what it showed is in the log,
        // masked, for whoever must find out why (T-1328).
        if let Some(failure) = start {
            self.seams.log.warn(&format!(
                "@{}'s window never ran {who}: {}; {}",
                participant.handle,
                failure.words(who),
                shown.in_the_log()
            ));
        }
        let shown = shown.with_start(start, who);
        let closed = format!("@{}'s window closed", participant.handle);
        let because = if at_launch {
            self.unstarted_because(&record, &closed, &shown)
        } else {
            closed.clone()
        };
        let first = delivering
            .as_ref()
            .filter(|delivering| delivering.launch)
            .map(|delivering| delivering.message);
        let project = match delivering {
            None => project,
            Some(delivering) => {
                let launches = record.window.borrow().launches;
                // A window that closed with its project (the human's Close, or the
                // chief's own exit closing its workers' windows) fails nothing that was
                // on its way to it: what its record does not show it took (`settle_gone`)
                // reached no conversation the Resume goes on with, and waits for the
                // Resume with its attempt given back, as one turned away at its hand-over
                // does (`hand_first`). So does a launch's first message, however it went
                // in: owed still, pasted a moment before, or read from the file its
                // command line named as the window opened (Devin's `--prompt-file`, which
                // T-523 left out: the alpha.1 release check T-1068 saw its brief and task
                // fail at a Close, T-1076). One whose window ended by itself in an open
                // project fails, as a launch that did not come up does.
                let closed_with_project = project.state != "open";
                // A paste its harness's record shows it took is confirmed (`in_flight`).
                let (this, told) = (Rc::clone(self), because.clone());
                self.settle_gone(delivering, closed_with_project, move |delivering| {
                    // A chief's first message (a handoff, most often) waits for its next window.
                    if delivering.chief || closed_with_project {
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
            let own = record.window.borrow().own_exit_of(pane).is_some();
            self.settle_exited_launch(&record, &project, &participant, &prelude, &because, own)?;
        }
        if participant.role == "chief" {
            // What the chief's window left running is the human's to know: no task's giver
            // is there to tell.
            self.tell_human_left_running(
                &project,
                &participant,
                shown.left_running(),
                shown.possibly_left(),
            )?;
            // The chief's own exit closes the project as Close does, but for the
            // window of a launch that was to say the role: it did not come up, and
            // is tried again (`settle_exited_launch`).
            let own = record.window.borrow().own_exit_of(pane).is_some();
            // A launch whose harness ended it with a code of failure before it came up,
            // its screen saying it no longer has the model the launch asked for, closes
            // nothing: the chief opens again at once on the harness's default (CHF-06).
            // A refusal at launch has no record to say it in, so its screen is read; a
            // window that came up, or ended with no failure, is read in no screen's words
            // (Calliope's P1, T-955). A window came up where its record showed a message it
            // was handed (`confirm_delivered`), its launch's first one among them: not where its
            // program started (Claude Code's `started` answers at once), nor where its record
            // shows anything (a resumed conversation's shows its history at once). The
            // screen is read only at the launch, an exit within its budget of the window's
            // open (a refusal ends a launch within a second; a window handed nothing that
            // lived on was no launch refused), and without what ConsensFlow pasted into it,
            // which may quote the words (Calliope's P2c, T-994).
            let (up, opened) = {
                let window = record.window.borrow();
                (window.took, window.opened_at)
            };
            let at_its_launch =
                opened.is_some_and(|opened| self.now() - opened <= self.seams.limits.launch_ms);
            if project.state == "open" && !own && !up && at_its_launch && shown.ended_failing() {
                let said = shown.clone().without_pastes(&pasted);
                if let Some(gone) = self.gone_model(&participant, &said.evidence())? {
                    self.reopen_without(&record, gone);
                    return Ok(());
                }
            }
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
                        // A role the window read the moment it had it is `read`, not `delivered`.
                        .is_some_and(|message| {
                            matches!(message.state.as_str(), "delivered" | "read")
                        }),
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
            // What the window was doing when it closed under its task, from its conversation's
            // record, where the close is not the engine's own (which says why it closed
            // instead): the note names the last tool call, which no screen of the window says
            // (T-823, kluris T-153). A participant that moved on while the record was read
            // settles nothing more of this exit, as after `settle_gone`. No stop and no close of
            // the engine's own was under way when it is not an own close: a signal that ended
            // the program came from outside ConsensFlow, which the note says.
            let own_close = record.window.borrow().own_exit_of(pane).is_some();
            let last_call = if own_close
                || self
                    .seams
                    .ledger
                    .borrow()
                    .active_task(record.id, false)?
                    .is_none()
            {
                None
            } else {
                let launches = record.window.borrow().launches;
                let call = self.last_call_of(participant.id).await;
                if !self.still_as_it_went(&record, launches) {
                    self.changed();
                    return Ok(());
                }
                call
            };
            let shown = if own_close {
                shown
            } else {
                let part = if sent { Part::Sent } else { Part::NotSent };
                shown.with_part(part).with_last_call(last_call)
            };
            let task = self.seams.ledger.borrow().active_task(record.id, false)?;
            // A task shown working (or waiting) for the words that resumed it, which had not
            // gone in, goes back to queued with them: the claim was its window's, and the
            // words reopen the window. Paused here, the window's exit would undo the chief's
            // resume (T-1153's F1).
            let task = match task {
                Some(thread)
                    if self.withdraw_resume_claim(
                        project.id,
                        (thread.task.number, thread.task.state.as_str()),
                        "its window closed",
                    )? =>
                {
                    None
                }
                other => other,
            };
            // What the host lists is the turn's work, by its own proof, as it was when the
            // window closed.
            let left = shown.left_running();
            if let Some(thread) = task {
                // The pause note says how the window ended and what it showed last, as
                // far as the host said, and what the close ended, where there was any, in
                // the words of every close note: a harness that crashed under its task says
                // why on its screen. A window the engine closed says why it did, where it
                // was not the human's Close, and nothing of its end, which was the kill's
                // (the host's exit can come while the kill is answered).
                let besides = leaving(left, shown.possibly_left());
                // A window the engine closed itself: in a suspended project it closed
                // with the project (the human's Close, or the chief's window ending),
                // and the ledger marks the pause for the human's Resume to end. One
                // that closed by itself stays paused.
                let (own, with) = {
                    let window = record.window.borrow();
                    (
                        window.own_exit_of(pane).cloned(),
                        window.closed_with.clone(),
                    )
                };
                let closed = match with {
                    Some(with) => format!("@{}'s window closed {with}", participant.handle),
                    None => format!("@{}'s window closed", participant.handle),
                };
                let because = if own.is_some() {
                    let outlived = shown.outlived().unwrap_or_default();
                    format!("{closed}{outlived}{besides}")
                } else {
                    shown.after_with(&closed, &besides)
                };
                // The words told to the window that wait for it still wait for the resume,
                // for why a give-up closed it where one did.
                let is_own = own.is_some();
                let (why, except) = match own {
                    Some(own) => (own.why.unwrap_or_else(|| because.clone()), own.settling),
                    None => (because.clone(), None),
                };
                let stop = self.stall(&project, &thread.task, &because, &why, except, is_own)?;
                // Told while the engine's kill of this window waits for its answer, which
                // decides whether this exit was the kill's (`close_own`): the pause of an
                // own exit, by its stop, which a refusal takes back only while it is that
                // pause (Calliope's P3-2 of T-510). An earlier window's kill, answered late,
                // is no answer of this one's (T-509), and a pause made in an open project
                // holds no mark to take back.
                if let Some((stop, killing)) = stop.zip(
                    record
                        .window
                        .borrow_mut()
                        .killing
                        .iter_mut()
                        .find(|killing| killing.pane == *pane),
                ) {
                    killing.paused = Some((project.id, thread.task.number, stop));
                }
            } else {
                let given = match owner {
                    Some(number) => self.seams.ledger.borrow().task(project.id, number)?,
                    None => None,
                };
                self.tell_left_running(&project, given.as_ref(), left, shown.possibly_left())?;
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
        // A window whose open went unanswered is asked to end again at each step until
        // the host says it has: a member with nothing more for it is still stepped.
        if !self.end_abandoned(&record).await? {
            return Ok(());
        }
        if project.state != "open" {
            return Ok(());
        }
        // A chief held for want of a model to start on, as the ledger keeps it, is stepped
        // to nothing: what waits for it stays queued for the chief the human switches in,
        // and no delivery is begun to be given back, nor its harness read, pass after pass
        // (Calliope's P2a, T-994). The launch's own hold stays behind it.
        if participant.role == "chief" && held_for_no_default(participant) {
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
        // A member's session is its task's: with the window gone and nothing due to it, nobody is doing the work.
        let task = self
            .seams
            .ledger
            .borrow()
            .active_task(participant.id, false)?;
        if let Some(thread) = task {
            let because = format!("@{}'s window is gone", participant.handle);
            self.stall(project, &thread.task, &because, &because, None, false)?;
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
        // A window on its way out is not looked at; one whose exit is overdue is settled.
        if record.window.borrow().retiring {
            return self.exit_overdue(record).await;
        }
        // A participant forgotten while the step waits is done with.
        let observed = match returning(self.observe(participant, record)).await {
            Ok(observed) => observed,
            Err(reason) => {
                if !self.forgotten(record) {
                    // A look that failed says nothing of what is on its way in (`arrival`),
                    // nor of the window's rest (`pacing`).
                    self.unknown(record, "a look at it failed");
                    self.rest_ends(record);
                    self.set_activity(record, Activity::because(ActivityState::Unknown, reason))?;
                }
                return Ok(());
            }
        };
        // A dialog of its harness's that waits for a key is told of, and one only its screen
        // shows holds the window as one its harness says it waits on (`dialogs`).
        let observed = self
            .watch_dialog(project, participant, record, observed)
            .await?;
        // A command of the window that waits on a macOS privacy dialog holds it (`privacy`).
        let observed = self.watch_privacy(record, observed).await;
        if self.forgotten(record) {
            return Ok(());
        }
        record.delivery.borrow_mut().lost = observed.lost().len();
        // Which look this is, for a rest to be read over looks that follow each other.
        let looked = self.looked(record);
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
        // A window read at rest by the fallback is asked whether it is drawn whatever its
        // record holds: its harness's prompt is its only sign that it is up (`drawn`).
        let drawing = asking
            || ((observed.items().is_empty() || unnamed || !self.idle_word(record))
                && !returning(self.drawn(record)).await);
        if self.forgotten(record) {
            return Ok(());
        }
        let starting = drawing || (unnamed && !record.window.borrow().named);
        // An out member's window says so, and nothing else, until the reset.
        if !self.is_out(&owner) {
            // The reason is given once the question is on its screen.
            let trusting = record.window.borrow().trust;
            let activity = if trusting == Trusting::Asked {
                Activity::because(ActivityState::Starting, trust::HELD)
            } else if trusting == Trusting::AskedServers {
                Activity::because(ActivityState::Starting, trust::HELD_SERVERS)
            } else if starting {
                Activity::of(ActivityState::Starting)
            } else if let Some(waiting) = &observed.waiting {
                Activity {
                    state: ActivityState::Waiting,
                    reason: waiting.reason.clone(),
                }
            } else if let Some(words) = record
                .delivery
                .borrow()
                .for_human
                .clone()
                .filter(|_| observed.settled)
            {
                // Held for what only the human can do in it: said as the window's wait,
                // and asked again at every look (`Held::ForHuman`).
                Activity::because(ActivityState::Waiting, words)
            } else {
                let state = if observed.settled {
                    ActivityState::Idle
                } else {
                    ActivityState::Working
                };
                // A line of the record lost in the turn is said while the turn is the last.
                match lost_in_turn(record, &observed, last_paste(&observed)) {
                    Some(lost) => Activity::because(
                        state,
                        format!(
                            "a line of @{}'s record was lost (record {lost})",
                            participant.handle
                        ),
                    ),
                    // A record that cannot be read lets no message in (CMT-05), though the
                    // look reads the window at rest: its lane says so (T-842's P2).
                    None if state == ActivityState::Idle
                        && observed
                            .reading
                            .as_deref()
                            .is_some_and(cf_harness::records::Reading::read_failed) =>
                    {
                        Activity::because(state, pacing::unreadable(&observed))
                    }
                    // A window whose harness says nothing of its rest is read at rest by
                    // the pacing's fallback, and its lane says so.
                    None if state == ActivityState::Idle && !self.idle_word(record) => {
                        Activity::because(state, pacing::FALLBACK)
                    }
                    // A chief read at work while a message has waited ten minutes for it: its
                    // lane says so, as the human is told (`held_not_at_rest`, T-958).
                    None if state == ActivityState::Working && participant.role == "chief" => {
                        match self.held_long(record) {
                            Some(message) => Activity::because(
                                state,
                                format!("m-{message} has waited ten minutes or more for this window, which reads at work: the human is told"),
                            ),
                            None => Activity::of(state),
                        }
                    }
                    None => Activity::of(state),
                }
            };
            // A look of the host's own, out of the Node traces: it costs no turn.
            let activity = self.fallback_lane(record, activity).await;
            if self.forgotten(record) {
                return Ok(());
            }
            self.set_activity(record, activity)?;
        }
        self.copy(participant, record, &observed)?;
        // The human switched the window to another conversation: this look was the old one's last.
        if let Some(session) = &observed.switched {
            if record.delivery.borrow().delivering.is_some() {
                self.confirm_arrival(record, &observed)?;
            }
            // What is on its way still went into the conversation left: its rest is that
            // record's from the next look (`arrival`).
            let mut part = record.delivery.borrow_mut();
            if let Some(delivering) = part.delivering.as_mut() {
                delivering.rested_since = None;
            }
            part.left_at_work = None;
            drop(part);
            self.follow(participant, record, session)?;
            return self.greet_switched(project, participant, record, session);
        }
        self.record_quota(record, observed.quota.clone());
        self.tell_drift(project, participant, record, &observed)?;
        // A chief's turn its busy model refused is the human's to hear, as before (T-1112): the
        // chief has no task to hold and try again, and a hold would lose its turn unheard.
        let chief_busy = participant.role == "chief" && busy_turn(&observed);
        if !chief_busy && self.refused_here(record, &owner) {
            return self
                .out_of_quota(project, participant, record, &owner, &observed)
                .await;
        }
        let mut out = self.is_out(&owner);
        let mut returned = None;
        // A window that got a turn through after its member was marked out says the quota is
        // back, and so does the harness's own count of an allowance, for an account that
        // was unavailable with no reset to wait for (MUS-23: Muse's `usage`). Not for a member
        // held for its model's move, which only the human's word for it brings back, whatever
        // another of its windows answered (T-816).
        let answered = out
            && !owner.out_until.as_deref().is_some_and(moved_hold)
            && self.answered_since(&owner, &observed);
        if answered || (out && self.shows_an_allowance(&owner, &observed)) {
            let because = if answered {
                format!("@{} answered again", participant.handle)
            } else {
                format!("@{} shows an allowance again", participant.handle)
            };
            self.seams
                .ledger
                .borrow_mut()
                .mark_back(owner.id, &because)?;
            returned.clone_from(&owner.out_until);
            self.changed();
            out = false;
        }
        // A turn that got through is its account's news: whoever else is out on it is back.
        if !out {
            let news = News::Turn {
                record,
                observed: &observed,
                returned,
            };
            self.back_on_its_account(project.id, &owner, &news)?;
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
        // A turn that ended with the window's answer ends the outage its last failure told of.
        self.answered_again(record, &observed);
        if participant.role != "chief" {
            returning(self.settle_stop(project, participant, record, &observed, starting)).await?;
            if self.forgotten(record) {
                return Ok(());
            }
            // A held task's window that went on with it by itself: its task goes on.
            self.went_on_by_itself(project, participant, &observed)?;
            // A resumed task whose window is at work on it reads working while its words
            // wait for the window's rest.
            self.show_resumed_at_work(project, participant, record, &observed)?;
            // A task the daemon resumed whose turn got through: its requester hears it goes on.
            self.tell_it_went_on(project, participant, record, &observed)?;
            // A turn a refusal cut short, its member past it now, failed nothing: its task goes on.
            // Not over words given since the cut, which begin another turn.
            if self.cut_short(&observed) {
                if !self.words_since_the_cut(participant, &observed)? {
                    self.go_on(project, participant)?;
                }
            } else {
                // Its answer is its result, recorded now, unless its harness says the turn
                // left work running that wakes its agent: then the task goes on. Nothing the
                // turn left is ended; the window closes below if it is free (`turn_end`), once
                // its screen is drawn where its task is done (`still_drawing`).
                let watching = self.watch_monitors(participant, record, &observed).await;
                let turn = self
                    .collect(project, participant, record, &observed, watching)
                    .await?;
                // A turn that came to nothing may be one that never began on what its
                // window was given (`never_began`).
                if matches!(turn, Turn::Open) {
                    self.watch_begun(project, participant, record, &observed)
                        .await?;
                }
            }
            if self.close_if_free(record).await? {
                return Ok(());
            }
        } else if chief_busy {
            self.tell_chief_failed(project, participant, record, &observed)
                .await?;
        } else if observed.failed && !self.cut_short(&observed) {
            // A refusal for quota is the quota's to say; any other failure is the human's to hear.
            self.tell_chief_failed(project, participant, record, &observed)
                .await?;
        }
        // A launch's first message that its window is delivered once it is up goes
        // in as soon as the window is past any question of trusting its folder, has
        // drawn and is at rest, its project open (`hand_first`).
        let owed = record
            .delivery
            .borrow()
            .delivering
            .as_ref()
            .is_some_and(|delivering| delivering.owed.is_some());
        if owed {
            if !drawing && observed.settled && observed.waiting.is_none() && project.state == "open"
            {
                let (this, held) = (Rc::clone(self), Rc::clone(record));
                self.act(record, async move { this.hand_first(&held).await })
                    .await;
            }
            return Ok(());
        }
        let free = record.delivery.borrow().delivering.is_none()
            && observed.settled
            && observed.waiting.is_none();
        let idle = free && !drawing;
        self.note_rest(record, &observed, idle, looked);
        self.recheck_if_due(record, &observed, idle).await;
        if self.forgotten(record) {
            return Ok(());
        }
        if record.pending_switch.borrow().is_some() {
            return self
                .await_switch(project, participant, record, &observed, idle)
                .await;
        }
        // A setting its role is built from may have changed under the window: it is told
        // now, as a wake-up is queued, and the note goes in by the way every message does,
        // ahead of the wake-up's. Only a chief's window was told a role at its launch that a
        // setting changes (`role_news`).
        if project.state == "open" && idle {
            self.tell_role_news(project, participant, record)?;
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
            // A hold is the head's: a message that left the head of its queue (withdrawn,
            // its task paused or cancelled) is held back no more (Calliope's re-check of
            // `a0aecd192`, T-854), and the wait its window's readiness said for it is no
            // more the lane's: the words only the human could act on, or words typed and
            // not sent (Calliope's P3-1 of T-945). And what a look read of the human's
            // input is this look's.
            {
                let mut part = record.delivery.borrow_mut();
                part.input = None;
                let head = next.as_ref().map(|next| next.id);
                if part
                    .held
                    .as_ref()
                    .is_some_and(|held| Some(held.message) != head)
                {
                    part.held = None;
                    part.for_human = None;
                    part.unsent = false;
                }
            }
            if let Some(next) = next {
                // Another task's message goes into a window of its own (`ownership`).
                if let Some(owner) = self.barred(participant, record, &next) {
                    return self
                        .keep_apart(project, participant, record, &next, owner)
                        .await;
                }
                // Typed in only once the window has rested long enough (`pacing`); a wait
                // says why, once a reason.
                match self.paced(record, &next).await? {
                    pacing::Pace::Go => {}
                    pacing::Pace::Wait(why) => return self.held_once(record, next.id, &why).await,
                    pacing::Pace::Gone => return Ok(()),
                }
                if self.forgotten(record) {
                    return Ok(());
                }
                let (this, held) = (Rc::clone(self), Rc::clone(record));
                self.act(record, async move { this.deliver(&held, next).await })
                    .await;
            }
        } else {
            // A chief's window not at rest holds its queue: from ten minutes on, the human
            // is told what waits and why, and its lane says it (`held_not_at_rest`, T-958).
            if participant.role == "chief" && project.state == "open" {
                self.held_not_at_rest(record, &observed, drawing).await?;
            }
            if free && self.ready_while_drawing(record) {
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
        let reported = record.quota.borrow().reported.clone();
        let unwell = Unwell::of(reported.as_deref());
        // A window of a member out for its busy model whose own record shows no refusal says
        // so too (T-1112).
        let words = match unwell {
            None if self.kind_of_hold(owner.id, &until) == Held::Busy => {
                format!("model busy, tried again at {until}")
            }
            _ => out_words(&until, unwell),
        };
        self.set_activity(record, Activity::because(ActivityState::Out, words))?;
        // A refusal of its own, dated before its member was marked out, holds the
        // window's work as the first did, with no note of its own.
        if participant.role != "chief" && self.refused_unhandled(record) {
            self.handled(record);
            self.hold_work(project, participant, owner, &until, None, unwell)?;
            self.changed();
        }
        self.tell_what_waits(project, participant, record, owner)?;
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

    /// The human's Switch chief: the chief goes on in a fresh window on `harness` and
    /// `model`, its first message the handoff.
    pub async fn switch_chief(
        self: &Rc<Self>,
        project: i64,
        to: SwitchTo,
        when: SwitchWhen,
        note: bool,
    ) -> Result<Option<ProjectView>, EngineError> {
        self.require_chief(&to.harness, Some(&to.model))?;
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
                return this.after_turn(&held, to, note);
            }
            // On the heap, as the dispatcher's other big steps are: inline it took the
            // switch's future past its budget (3.0.6-alpha.3's integration B, T-664).
            Box::pin(this.perform_switch(&found, &chief, &held, to)).await?;
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
    /// reset; what the window was receiving arrived if its record shows it
    /// (the refusal answered it), else it is queued again with its attempt,
    /// and its work waits with it.
    async fn out_of_quota(
        self: &Rc<Self>,
        project: &ProjectView,
        participant: &ParticipantView,
        record: &Rc<Record>,
        owner: &ParticipantView,
        observed: &Observed,
    ) -> Result<(), EngineError> {
        self.handled(record);
        // A refused turn leaves nothing running that wakes its agent: its task waits for
        // the quota now, not for a job of an earlier turn (`turn_end`).
        record.held.borrow_mut().reset();
        let reported = record.quota.borrow().reported.clone();
        let unwell = Unwell::of(reported.as_deref());
        // A busy model is tried again after a backoff of the engine's, by its tries (T-1112).
        let reset = if let Some(Unwell::Busy(_)) = unwell {
            self.busy_reset(project, participant)?
        } else {
            self.reset_of(reported.as_deref())
        };
        let kind = match unwell {
            Some(Unwell::Account(_)) => HoldKind::Account,
            Some(Unwell::Moved(_)) => HoldKind::Moved,
            Some(Unwell::Busy(_)) => HoldKind::Busy,
            None => HoldKind::Quota,
        };
        let marked = self.seams.ledger.borrow_mut().mark_out_with_kind(
            owner.id,
            &reset,
            unwell.map_or("out of quota", Unwell::hold),
            kind,
        )?;
        let until = marked.out_until.clone().unwrap_or_default();
        self.set_activity(
            record,
            Activity::because(ActivityState::Out, out_words(&until, unwell)),
        )?;
        self.confirm_arrival(record, observed)?;
        let delivering = record.delivery.borrow_mut().delivering.take();
        if let Some(delivering) = delivering {
            self.give_back(
                delivering.message,
                match unwell {
                    Some(Unwell::Account(_)) => "its account is unavailable",
                    Some(Unwell::Moved(_)) => "its model moved on its own",
                    Some(Unwell::Busy(_)) => "its model was busy",
                    None => "the harness ran out of quota",
                },
            )?;
        }
        let refused_at = self.refused_at(reported.as_deref());
        self.hold_work(
            project,
            participant,
            owner,
            &until,
            Some(refused_at),
            unwell,
        )?;
        if let Some(unwell) = unwell {
            self.tell_unwell(project, participant, &marked, unwell)?;
        }
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

    /// The record whose window has the pane `pane`, if one does (not the records of
    /// participants forgotten while their window was open).
    pub(crate) fn record_of_pane(&self, pane: &Pane) -> Option<Rc<Record>> {
        self.records
            .borrow()
            .values()
            .find(|record| record.window.borrow().pane.as_ref() == Some(pane))
            .cloned()
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

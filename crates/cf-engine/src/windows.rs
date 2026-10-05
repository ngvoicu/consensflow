//! A window's lifecycle (`src/core/windows.js`): launch (prepare, token,
//! `pane.open`, conversation, `started`), look (`observe`, `drawn`),
//! interrupt (Escape rounds), and close (`retire`, `close_if_free`,
//! `close_own`). It owns the record's window part, the generation of each
//! pane, and the chief's relaunch backoff.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use cf_harness::contract::{Interrupt, Launch, LaunchId, Observed, Pane, Window};
use cf_harness::records::Role;
use cf_harness::seams::Time;
use cf_ledger::{MessageView, NewNote, ParticipantView, ProjectView};
use cf_proto::trace::WindowEvent;
use serde_json::{json, Value};

use crate::deliveries::Delivering;
use crate::delivery_text::{delivery_text, marker_of};
use crate::dispatcher::Dispatcher;
use crate::host::{EngineHost, Killed, OpenPane, Opened};
use crate::record::Record;
use crate::runtime::{begin, turn};
use crate::seams::{EngineError, SavedAgent};

/// The key that interrupts a harness's current turn, how often it is pressed
/// again for a window still working, how soon, and the gap of a double press.
const ESCAPE: u8 = 27;
const INTERRUPT_ROUNDS: u32 = 3;
const INTERRUPT_AGAIN_MS: i64 = 3_000;
const DOUBLE_PRESS: Duration = Duration::from_millis(150);

/// How long a fresh window's output must hold still before its screen counts as drawn.
const DRAWN_QUIET_MS: f64 = 1_500.0;

/// How soon a chief that could not start is tried again; the wait doubles
/// with each failure, up to the most.
const RELAUNCH_MS: i64 = 5_000;
const RELAUNCH_MAX_MS: i64 = 5 * 60_000;

/// What a window is doing, as the board and the trace say it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityState {
    Starting,
    Working,
    Idle,
    Waiting,
    Out,
    Unknown,
    Closed,
}

impl ActivityState {
    pub fn as_str(self) -> &'static str {
        match self {
            ActivityState::Starting => "starting",
            ActivityState::Working => "working",
            ActivityState::Idle => "idle",
            ActivityState::Waiting => "waiting",
            ActivityState::Out => "out",
            ActivityState::Unknown => "unknown",
            ActivityState::Closed => "closed",
        }
    }
}

/// A window's activity, and why where it says (a question it waits on, the
/// reset it is out of quota until, a look that failed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Activity {
    pub state: ActivityState,
    pub reason: Option<String>,
}

impl Activity {
    pub fn of(state: ActivityState) -> Self {
        Self {
            state,
            reason: None,
        }
    }

    pub fn because(state: ActivityState, reason: impl Into<String>) -> Self {
        Self {
            state,
            reason: Some(reason.into()),
        }
    }
}

/// A pane on its way open, and whether its exit came before the host's
/// answer (the host watches a window's process before it answers).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Opening {
    pub(crate) pane: Pane,
    pub(crate) exited: bool,
}

/// The chief's window failing to start, tried again ever more slowly: how
/// often it failed, and when it may be tried next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Relaunch {
    pub(crate) failures: u32,
    pub(crate) at: i64,
}

/// The rounds of Escape one stop of a task has had: its key, how many, and
/// when the last was.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Interrupted {
    stop: String,
    rounds: u32,
    at: i64,
}

/// The record's window part.
pub(crate) struct WindowPart {
    /// The harness's window on its launch (the adapter's `launch` bag).
    pub(crate) window: Option<Rc<dyn Window>>,
    /// The keys that interrupt a turn in it (its adapter's `interrupt`).
    pub(crate) keys: Option<Interrupt>,
    pub(crate) pane: Option<Pane>,
    pub(crate) opening: Option<Opening>,
    pub(crate) launch_id: Option<LaunchId>,
    /// The window's token, for its `cf`.
    pub(crate) token: Option<String>,
    /// A kill is on its way.
    pub(crate) retiring: bool,
    /// Its exit is the engine's own (`close_own`): a chief's closes no project.
    pub(crate) own_exit: bool,
    /// The human opened it (Show): it stays open.
    pub(crate) pinned: bool,
    /// The human hid it since: it closes once free and its turn settled.
    pub(crate) hidden: bool,
    /// Whether its last look said its turn settled.
    pub(crate) settled: bool,
    pub(crate) relaunch: Option<Relaunch>,
    /// It printed, then held still: its screen is drawn.
    pub(crate) drawn: bool,
    /// It named its first conversation.
    pub(crate) named: bool,
    pub(crate) interrupted: Option<Interrupted>,
    pub(crate) activity: Activity,
}

impl Default for WindowPart {
    fn default() -> Self {
        Self {
            window: None,
            keys: None,
            pane: None,
            opening: None,
            launch_id: None,
            token: None,
            retiring: false,
            own_exit: false,
            pinned: false,
            hidden: false,
            settled: false,
            relaunch: None,
            drawn: false,
            named: false,
            interrupted: None,
            activity: Activity::of(ActivityState::Closed),
        }
    }
}

/// What the windows keep across records: the last pane's generation.
#[derive(Debug, Default)]
pub(crate) struct WindowsState {
    generation: Cell<i64>,
}

/// What a part of the engine not ported yet answers.
pub(crate) fn not_ported(what: &str) -> EngineError {
    EngineError::said("not-ported", format!("{what} is not ported yet"))
}

/// A pane as the pane host's requests name it.
fn pane_body(pane: &Pane) -> Value {
    json!({ "id": pane.id, "generation": pane.generation })
}

/// A harness's interrupt as keys into its window (`pressInterrupt`): Escape
/// as many times in a row as it asks for, and, where those presses open a
/// dialog at a turn that ended just before them (Devin's rewind), one more
/// after a pause, which closes it and is nothing anywhere else. A key the
/// host did not take is not pressed again.
pub(crate) async fn press_interrupt(
    host: &dyn EngineHost,
    time: &dyn Time,
    pane: &Pane,
    keys: Interrupt,
) {
    let escape = || {
        let mut body = pane_body(pane);
        body["bytes"] = json!([ESCAPE]);
        host.request("pane.input", body)
    };
    for press in 0..keys.presses {
        if press > 0 {
            time.sleep(DOUBLE_PRESS).await;
        }
        let _ = escape().await;
    }
    if let Some(after) = keys.close_after {
        time.sleep(after).await;
        let _ = escape().await;
    }
}

/// `env` with `key` set as an object spread sets it: in its first place
/// where it is there already, last where it is not.
fn assign(env: &mut Vec<(String, String)>, key: &str, value: &str) {
    match env.iter_mut().find(|(name, _)| name == key) {
        Some((_, old)) => value.clone_into(old),
        None => env.push((key.to_owned(), value.to_owned())),
    }
}

/// What a launch was prepared with: the window, the keys that interrupt it,
/// and how its pane opens.
struct Planned {
    window: Rc<dyn Window>,
    keys: Interrupt,
    argv: Vec<String>,
    env: Vec<(String, String)>,
    drop_env: Vec<String>,
    native_session: Option<String>,
}

impl Dispatcher {
    /// Refuses a harness ConsensFlow has no adapter for: none of its windows
    /// could open.
    pub(crate) fn require_adapter(&self, harness: &str) -> Result<(), EngineError> {
        match self.seams.adapters.adapter(harness) {
            Some(_) => Ok(()),
            None => Err(EngineError::said(
                "no-adapter",
                format!("ConsensFlow cannot open {harness} windows"),
            )),
        }
    }

    /// A window that exited: its token is revoked, its files in the home go
    /// with it, and its part of the record holds no window.
    pub(crate) fn closed(&self, record: &Record) {
        let (token, launch) = {
            let mut window = record.window.borrow_mut();
            let taken = (window.token.take(), window.launch_id.take());
            window.window = None;
            window.pane = None;
            window.retiring = false;
            window.hidden = false;
            window.settled = false;
            window.activity = Activity::of(ActivityState::Closed);
            taken
        };
        if let Some(token) = token {
            self.seams.credentials.revoke(&token);
        }
        if let Some(launch) = launch {
            self.seams.launch_files.forget(&launch);
        }
    }

    /// The window of a record and its pane, as they are now.
    pub(crate) fn window_of(&self, record: &Record) -> (Option<Rc<dyn Window>>, Option<Pane>) {
        let part = record.window.borrow();
        (part.window.clone(), part.pane.clone())
    }

    /// The next pane's generation: past the last one, and no earlier than
    /// now, so a window opened after a restart takes no old one's.
    fn next_generation(&self) -> i64 {
        let next = (self.windows.generation.get() + 1).max(self.now());
        self.windows.generation.set(next);
        next
    }

    /// What a window does now, told to the trace and the board when it changed.
    pub(crate) fn set_activity(&self, record: &Record, activity: Activity) {
        {
            let mut window = record.window.borrow_mut();
            if window.activity == activity {
                return;
            }
            window.activity = activity.clone();
        }
        self.trace_window(
            record,
            WindowEvent::Activity {
                state: activity.state.as_str().to_owned(),
                reason: activity.reason,
            },
        );
        self.changed();
    }

    /// Launches `participant`'s window on its harness, `message` its first if
    /// there is one (a chief's is its handoff, most often). One that does not
    /// come up says why ([`Dispatcher::launch_failed`]); a participant
    /// forgotten while its launch waits gets nothing more under its ids.
    pub(crate) async fn launch(
        self: &Rc<Self>,
        record: &Rc<Record>,
        project: &ProjectView,
        participant: &ParticipantView,
        message: Option<MessageView>,
    ) -> Result<(), EngineError> {
        let harness = participant.harness.clone().unwrap_or_default();
        let conversation = self
            .seams
            .ledger
            .borrow()
            .current_conversation(participant.id)?;
        let resume = conversation
            .as_ref()
            .and_then(|conversation| conversation.native_session.clone());
        let first = if participant.role == "chief" {
            self.chief_first(project, participant, conversation.as_ref(), message)?
        } else {
            message
        };
        let launch_id = self.seams.launch_ids.draw();
        let generation = self.next_generation();
        let delivering = first.as_ref().map(|first| Delivering {
            message: first.id,
            marker: marker_of(first.id),
            since: self.now(),
            launch: true,
            chief: participant.role == "chief",
            queued: false,
            entered_again: false,
        });
        let planned = match self
            .plan(
                record,
                project,
                participant,
                first.as_ref(),
                resume.as_deref(),
                &launch_id,
                delivering.as_ref(),
            )
            .await
        {
            Ok(Some(planned)) => planned,
            Ok(None) => return Ok(()),
            Err(cause) => {
                // An adapter may fail after writing the launch's files; no window will read them.
                self.seams.launch_files.forget(&launch_id);
                return self.launch_failed(
                    record,
                    project,
                    participant,
                    delivering,
                    &format!("the launch failed: {cause}"),
                );
            }
        };
        // A participant forgotten while its launch waits (it left, or its
        // project was deleted) gets nothing more under its ids, whose rows
        // may be gone: no window opens for it.
        if self.forgotten(record) {
            self.seams.launch_files.forget(&launch_id);
            return Ok(());
        }
        let token = self.seams.credentials.issue(project.id, participant.id);
        let pane = Pane {
            id: format!("p{}-{}", project.id, participant.handle),
            generation: u64::try_from(generation).unwrap_or_default(),
        };
        let mut env = self.seams.pane_env.env(participant, project);
        for (key, value) in &planned.env {
            assign(&mut env, key, value);
        }
        assign(&mut env, "CONSENSFLOW_TOKEN", &token);
        // The host watches a window's process before it answers the open, so
        // an exit can come first, even in the same read: the exit finds it here.
        record.window.borrow_mut().opening = Some(Opening {
            pane: pane.clone(),
            exited: false,
        });
        let opened = self
            .seams
            .host
            .open(OpenPane {
                pane: pane.clone(),
                cwd: project.directory.clone(),
                argv: planned.argv.clone(),
                env,
                drop_env: planned.drop_env.clone(),
            })
            .await;
        let exited = record
            .window
            .borrow_mut()
            .opening
            .take()
            .is_some_and(|opening| opening.exited);
        let pid = match opened {
            Ok(Opened::Open { pid }) => pid,
            Ok(Opened::Refused { error }) => {
                return self.not_opened(
                    record,
                    project,
                    participant,
                    delivering,
                    &token,
                    &launch_id,
                    &error,
                );
            }
            Err(error) => {
                return self.not_opened(
                    record,
                    project,
                    participant,
                    delivering,
                    &token,
                    &launch_id,
                    &error.message,
                );
            }
        };
        // The window's own process: an adapter may find the harness's own
        // status by it from its first look.
        planned.window.opened(pid);
        if self.forgotten(record) {
            {
                let mut part = record.window.borrow_mut();
                part.pane = Some(pane.clone());
                part.launch_id = Some(launch_id);
                part.token = Some(token);
            }
            // One that exited before its open was answered has gone already.
            if exited {
                if let Some(rest) = self.pane_exited(pane) {
                    rest.await;
                }
            }
            return Ok(());
        }
        let resumed = resume.is_some() && planned.native_session == resume;
        let mut conversation_id = conversation.as_ref().map(|conversation| conversation.id);
        if !resumed {
            let started = self
                .seams
                .ledger
                .borrow_mut()
                .start_conversation(participant.id, &harness)?;
            conversation_id = Some(started.id);
            if let Some(native) = planned.native_session.as_deref() {
                self.seams
                    .ledger
                    .borrow_mut()
                    .bind_conversation(started.id, native)?;
            }
        }
        {
            let mut part = record.window.borrow_mut();
            part.window = Some(Rc::clone(&planned.window));
            part.keys = Some(planned.keys);
            part.pane = Some(pane.clone());
            part.launch_id = Some(launch_id);
            part.token = Some(token);
            part.activity = Activity::of(ActivityState::Starting);
            part.drawn = false;
            part.named = false;
        }
        record.delivery.borrow_mut().delivering = delivering.clone();
        // A window that exited before its open was answered goes as any exit does.
        if exited {
            if let Some(rest) = self.pane_exited(pane) {
                rest.await;
            }
            return Ok(());
        }
        let started = planned.window.started().await;
        if let (Err(error), Some(delivering)) = (&started, delivering) {
            record.delivery.borrow_mut().delivering = None;
            // The chief's window goes without taking its project with it: the chief is tried again.
            if participant.role == "chief" {
                self.close_own(record, &pane).await?;
            } else {
                let this = Rc::clone(self);
                drop(
                    begin(&*self.seams.spawn, async move {
                        let _ = this.seams.host.kill(&pane).await;
                    })
                    .await,
                );
            }
            return self.launch_failed(
                record,
                project,
                participant,
                Some(delivering),
                &format!("the window could not take its first message: {error}"),
            );
        }
        if self.forgotten(record) {
            return Ok(());
        }
        record.window.borrow_mut().relaunch = None;
        if let (Ok(Some(native)), Some(conversation)) = (&started, conversation_id) {
            if !native.is_empty() && Some(native) != planned.native_session.as_ref() {
                self.seams
                    .ledger
                    .borrow_mut()
                    .bind_conversation(conversation, native)?;
            }
        }
        self.changed();
        Ok(())
    }

    /// The launch's plan, from its adapter, with its first message begun:
    /// none where the participant's saved agent is gone (the human hears
    /// it), or why the launch failed.
    #[allow(clippy::too_many_arguments)]
    async fn plan(
        self: &Rc<Self>,
        record: &Rc<Record>,
        project: &ProjectView,
        participant: &ParticipantView,
        first: Option<&MessageView>,
        resume: Option<&str>,
        launch_id: &LaunchId,
        delivering: Option<&Delivering>,
    ) -> Result<Option<Planned>, EngineError> {
        if let Some(first) = first {
            self.seams.ledger.borrow_mut().begin_delivery(first.id)?;
        }
        // A harness that lost its adapter fails what came for it, and says why.
        let harness = participant.harness.as_deref().unwrap_or_default();
        self.require_adapter(harness)?;
        let Some(adapter) = self.seams.adapters.adapter(harness) else {
            return Ok(None);
        };
        // A member runs on its saved agent's model, read now; one the human
        // has deleted from their agents must not fall back to a harness default.
        let saved: Option<SavedAgent> = match participant.agent.as_deref() {
            None => None,
            Some(name) => match self.seams.roster.agent(name)? {
                Some(saved) => Some(saved),
                None => {
                    self.without_saved_agent(record, project, participant, name, delivering)?;
                    return Ok(None);
                }
            },
        };
        // A session plays the role of the task it was started for; a member
        // or the chief its own.
        let text = first
            .map(|first| self.launch_text(project, participant, first, resume))
            .transpose()?;
        let instructions = self
            .seams
            .roles
            .instructions(participant, project)
            .map_err(|cause| EngineError::said("no-role-text", cause))?;
        let launch = Launch {
            id: launch_id,
            project: project.id,
            handle: &participant.handle,
            role: &participant.role,
            directory: &project.directory,
            resume,
            message: text.as_deref(),
            agent: saved.as_ref().map(SavedAgent::agent),
            instructions: &instructions,
        };
        let prepared = adapter
            .prepare(&launch)
            .await
            .map_err(|cause| EngineError::said("launch-failed", cause))?;
        Ok(Some(Planned {
            window: prepared.window,
            keys: adapter.interrupt(),
            argv: prepared.argv,
            env: prepared.env,
            drop_env: prepared.drop_env,
            native_session: prepared.native_session,
        }))
    }

    /// A participant whose saved agent the human has deleted does not open:
    /// the chief tells the human, a member's work goes back to the board, and
    /// a session's window the human opened, with nothing to deliver, says
    /// why it did not come.
    fn without_saved_agent(
        &self,
        record: &Rc<Record>,
        project: &ProjectView,
        participant: &ParticipantView,
        agent: &str,
        delivering: Option<&Delivering>,
    ) -> Result<(), EngineError> {
        if participant.role == "chief" {
            return self.chief_without_agent(project, participant, delivering.cloned());
        }
        self.without_agent(project, participant, delivering.cloned())?;
        if delivering.is_none() {
            self.launch_failed(
                record,
                project,
                participant,
                None,
                &format!("{agent} is no longer among your agents"),
            )?;
        }
        Ok(())
    }

    /// A window the host did not open: its token and files go, and the
    /// launch failed.
    #[allow(clippy::too_many_arguments)]
    fn not_opened(
        &self,
        record: &Rc<Record>,
        project: &ProjectView,
        participant: &ParticipantView,
        delivering: Option<Delivering>,
        token: &str,
        launch_id: &LaunchId,
        error: &str,
    ) -> Result<(), EngineError> {
        self.seams.credentials.revoke(token);
        self.seams.launch_files.forget(launch_id);
        self.launch_failed(
            record,
            project,
            participant,
            delivering,
            &format!("the window did not open: {error}"),
        )
    }

    /// A member's fresh session starts from nothing: when its first message
    /// is not the task's own brief (a reopening, an answer), the brief goes
    /// in first. A resumed conversation has it already.
    fn launch_text(
        &self,
        project: &ProjectView,
        participant: &ParticipantView,
        message: &MessageView,
        resume: Option<&str>,
    ) -> Result<String, EngineError> {
        let text = delivery_text(message);
        let Some(number) = message
            .task_number
            .filter(|_| resume.is_none() && participant.role != "chief")
        else {
            return Ok(text);
        };
        let task = self.seams.ledger.borrow().task(project.id, number)?;
        let brief = task.as_ref().and_then(|thread| {
            thread.messages.iter().find(|candidate| {
                candidate.kind == "task"
                    && candidate.recipient == participant.handle
                    && candidate.state != "cancelled"
            })
        });
        Ok(match brief {
            Some(brief) if brief.id != message.id => format!("{}\n\n{text}", delivery_text(brief)),
            _ => text,
        })
    }

    /// A launch that did not come up. A member's first message fails with
    /// it, so its task fails and the requester hears why; a window the human
    /// opened with nothing to deliver tells them why it did not come. The
    /// chief's first message goes back to its queue with its attempt, and
    /// the chief is tried again, ever more slowly while it keeps failing;
    /// the human hears why once. A participant forgotten while it launched
    /// hears nothing and settles nothing.
    fn launch_failed(
        &self,
        record: &Rc<Record>,
        project: &ProjectView,
        participant: &ParticipantView,
        delivering: Option<Delivering>,
        because: &str,
    ) -> Result<(), EngineError> {
        if self.forgotten(record) {
            return Ok(());
        }
        if participant.role != "chief" {
            match delivering {
                Some(delivering) => self.settle_failure(delivering, because, false)?,
                None => {
                    self.seams.ledger.borrow_mut().note(
                        project.id,
                        &NewNote {
                            from: None,
                            to: "human".to_owned(),
                            task: None,
                            body: format!("@{} could not start: {because}.", participant.handle),
                        },
                    )?;
                    self.changed();
                }
            }
            return Ok(());
        }
        if let Some(delivering) = delivering {
            self.give_back(delivering, because)?;
        }
        let failures = record
            .window
            .borrow()
            .relaunch
            .map_or(0, |relaunch| relaunch.failures)
            + 1;
        let wait = RELAUNCH_MS
            .saturating_mul(2_i64.saturating_pow(failures - 1))
            .min(RELAUNCH_MAX_MS);
        record.window.borrow_mut().relaunch = Some(Relaunch {
            failures,
            at: self.now() + wait,
        });
        if failures == 1 {
            self.seams.ledger.borrow_mut().note(
                project.id,
                &NewNote {
                    from: None,
                    to: "human".to_owned(),
                    task: None,
                    body: format!(
                        "The chief could not start: {because}. What comes for the chief waits for it, and ConsensFlow tries again; you may also switch the chief."
                    ),
                },
            )?;
        }
        self.changed();
        Ok(())
    }

    /// One look at a window: what its harness's record shows now, or why
    /// that could not be told. The window keeps whether the look said its
    /// agent's turn settled ([`Dispatcher::close_if_free`]).
    pub(crate) async fn observe(
        self: &Rc<Self>,
        _participant: &ParticipantView,
        record: &Rc<Record>,
    ) -> Result<Observed, String> {
        let Some(window) = record.window.borrow().window.clone() else {
            return Err("the window is closed".to_owned());
        };
        let looked = window.observe().await;
        if let Ok(observed) = &looked {
            record.window.borrow_mut().settled = observed.settled;
        }
        // JavaScript's `observe` was an async function: its caller had the
        // look a turn after the adapter answered.
        turn().await;
        looked
    }

    /// Whether a window has drawn its screen: it printed, then held still for
    /// a moment (the pane host says how long it has printed nothing). A host
    /// that cannot tell is taken as drawn.
    pub(crate) async fn drawn(self: &Rc<Self>, record: &Rc<Record>) -> bool {
        let (drawn, pane) = {
            let part = record.window.borrow();
            (part.drawn, part.pane.clone())
        };
        if drawn {
            return true;
        }
        let snapshot = match pane {
            Some(pane) => self
                .seams
                .host
                .request("pane.snapshot", pane_body(&pane))
                .await
                .ok(),
            None => None,
        };
        let drawn = match snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.get("outputQuietMs"))
        {
            None => true,
            Some(Value::Null) => false,
            Some(quiet) => quiet.as_f64().is_some_and(|quiet| quiet >= DRAWN_QUIET_MS),
        };
        if drawn {
            record.window.borrow_mut().drawn = true;
        }
        drawn
    }

    /// A window whose task stopped stops too. A paused task's window stays
    /// open, but its agent is interrupted. An agent still at work on a turn
    /// about a task cancelled under its window is interrupted as well. A
    /// turn the human began since in a window they opened is theirs, and
    /// goes on.
    pub(crate) async fn interrupt_if_stopped(
        self: &Rc<Self>,
        participant: &ParticipantView,
        record: &Rc<Record>,
        observed: &Observed,
    ) -> Result<(), EngineError> {
        let paused = self.seams.ledger.borrow().paused_task(participant.id)?;
        if let Some(paused) = paused {
            // The chief's tell reached the window during this pause: the agent
            // answers it and ends its own turn, uninterrupted.
            let told = self
                .seams
                .ledger
                .borrow()
                .told_since_paused(participant.id, paused.task.id)?;
            if !told {
                let at = paused.task.paused_at.as_deref().unwrap_or("null");
                self.interrupt(record, format!("{} paused {at}", paused.task.id))
                    .await;
            }
            return Ok(());
        }
        if record.window.borrow().activity.state != ActivityState::Working {
            return Ok(());
        }
        let cancelled = self.seams.ledger.borrow().last_task(participant.id)?;
        let Some(cancelled) = cancelled.filter(|thread| thread.task.state == "cancelled") else {
            return Ok(());
        };
        let turn = observed
            .items()
            .iter()
            .rev()
            .find(|item| item.role == Role::User);
        let about = cancelled.messages.iter().any(|message| {
            message.recipient_id == participant.id
                && turn.is_some_and(|turn| turn.text.contains(&marker_of(message.id)))
        });
        if about {
            self.interrupt(record, format!("{} cancelled", cancelled.task.id))
                .await;
        }
        Ok(())
    }

    /// Escape interrupts the turn a task's `stop` is for, and again a few
    /// seconds later while the window still reads as working, since a
    /// harness may ignore the key while it thinks: three rounds at most for
    /// each stop. Only a window at work is: Escape to an idle window opens
    /// Devin's rewind, and clears what the human was typing into Claude.
    async fn interrupt(self: &Rc<Self>, record: &Rc<Record>, stop: String) {
        let (pane, keys, done) = {
            let part = record.window.borrow();
            if part.activity.state != ActivityState::Working {
                return;
            }
            let done = part
                .interrupted
                .clone()
                .filter(|interrupted| interrupted.stop == stop);
            (part.pane.clone(), part.keys, done)
        };
        if done.as_ref().is_some_and(|done| {
            done.rounds >= INTERRUPT_ROUNDS || self.now() - done.at < INTERRUPT_AGAIN_MS
        }) {
            return;
        }
        record.window.borrow_mut().interrupted = Some(Interrupted {
            stop,
            rounds: done.map_or(0, |done| done.rounds) + 1,
            at: self.now(),
        });
        let (Some(pane), Some(keys)) = (pane, keys) else {
            return;
        };
        press_interrupt(&*self.seams.host, &*self.seams.time, &pane, keys).await;
    }

    /// A session's window closes with its task; its conversation stays, so a
    /// follow-up comes back on it. Says whether the window goes: one already
    /// closing had its kill, and one whose kill the pane host refused stays
    /// open, not closing, for the next close to try again.
    pub(crate) async fn retire(self: &Rc<Self>, record: &Rc<Record>) -> Result<bool, EngineError> {
        let pane = {
            let mut part = record.window.borrow_mut();
            if part.retiring {
                return Ok(true);
            }
            part.retiring = true;
            part.pane.clone()
        };
        let killed = match pane {
            Some(pane) => self.kill(record, &pane).await,
            None => false,
        };
        if !killed {
            record.window.borrow_mut().retiring = false;
        }
        self.changed();
        Ok(killed)
    }

    /// Kills a window: whether the pane host took the kill; the trace says
    /// why not.
    async fn kill(self: &Rc<Self>, record: &Rc<Record>, pane: &Pane) -> bool {
        let error = match self.seams.host.kill(pane).await {
            Ok(Killed::Killed) => return true,
            Ok(Killed::Refused { error }) => error,
            Err(error) => error.message,
        };
        self.trace_window(record, WindowEvent::KillFailed { error: Some(error) });
        false
    }

    /// A member's window closes once it holds no task, unless the human
    /// opened it or a message is still on its way in. One the human opened
    /// and has hidden since waits for its agent's turn to end first. Says
    /// whether it closed; one gone already has nothing to close.
    pub(crate) async fn close_if_free(
        self: &Rc<Self>,
        record: &Rc<Record>,
    ) -> Result<bool, EngineError> {
        let kept = {
            let part = record.window.borrow();
            part.pane.is_none() || part.pinned || (part.hidden && !part.settled)
        } || record.delivery.borrow().delivering.is_some()
            || self.seams.ledger.borrow().holds_work(record.id)?;
        if kept {
            return Ok(false);
        }
        self.retire(record).await?;
        Ok(true)
    }

    /// Closes a window whose exit is the engine's own (a switch, a Close, a
    /// chief that could not take its first message): the exit settles what
    /// the window was doing, as any exit does, but a chief's does not close
    /// its project. Says whether the window goes: one whose kill the pane
    /// host refused stays as it was, and no exit is made up for it.
    pub(crate) async fn close_own(
        self: &Rc<Self>,
        record: &Rc<Record>,
        pane: &Pane,
    ) -> Result<bool, EngineError> {
        record.window.borrow_mut().own_exit = true;
        let retiring = record.window.borrow().retiring;
        let went = if !retiring && !self.kill(record, pane).await {
            false
        } else {
            let open = record.window.borrow().pane.as_ref() == Some(pane);
            if open {
                if let Some(rest) = self.pane_exited(pane.clone()) {
                    rest.await;
                }
            }
            true
        };
        record.window.borrow_mut().own_exit = false;
        Ok(went)
    }
}

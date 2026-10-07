//! A window's lifecycle (`src/core/windows.js`): launch (prepare, token,
//! `pane.open`, conversation, `started`), look (`observe`, `drawn`), and close
//! (`retire`, `close_if_free`, `close_own`). It owns the record's window part,
//! the generation of each pane, and the chief's relaunch backoff. What a
//! window owes a stop of its task, and how it is interrupted, is `stops`.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use cf_harness::contract::{Interrupt, Launch, LaunchId, Observed, Pane, Window};
use cf_harness::seams::Time;
use cf_ledger::{Begun, MessageView, NewNote, ParticipantView, ProjectView, Stop};
use cf_proto::trace::WindowEvent;
use serde_json::{json, Value};

use crate::deliveries::Delivering;
use crate::delivery_text::{delivery_text, marker_of};
use crate::dispatcher::Dispatcher;
use crate::host::{EngineHost, Killed, OpenPane, Opened};
use crate::record::Record;
use crate::runtime::{begin, caught, returning};
use crate::seams::{EngineError, SavedAgent};
use crate::stops::{Interrupted, Unstopped};

/// The key that interrupts a harness's current turn, and the gap of a double press.
const ESCAPE: u8 = 27;
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
    /// The last stop this window process paid: its task and the sequence. It
    /// dies with the window, and a window opened later owes nothing for the
    /// stops asked before it opened.
    ///
    /// One pair is enough because a member session holds at most one task at
    /// a time (`require_free` in the ledger): the stops a window pays are its
    /// task's, and a task that follows (a follow-up, in a window the human
    /// keeps open) has its own, which a pair of the one before never stands
    /// in for. A window that held two and paid a stop for each in turn would
    /// forget the first's, and be interrupted for it again on going back to
    /// it: only a member's own lane, given tasks by name, can.
    pub(crate) stopped: Option<(i64, i64)>,
    /// The rounds the stop it owes has had.
    pub(crate) interrupted: Option<Interrupted>,
    /// The stop it ignored in every round, while it lasts.
    pub(crate) unstopped: Option<Unstopped>,
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
            stopped: None,
            interrupted: None,
            unstopped: None,
            activity: Activity::of(ActivityState::Closed),
        }
    }
}

/// What the windows keep across records: the last pane's generation.
#[derive(Debug, Default)]
pub(crate) struct WindowsState {
    generation: Cell<i64>,
}

/// A pane as the pane host's requests name it.
fn pane_body(pane: &Pane) -> Value {
    json!({ "id": pane.id, "generation": pane.generation })
}

/// A harness's interrupt as keys into its window (`pressInterrupt`): Escape
/// as many times in a row as it asks for, and, where those presses open a
/// dialog at a turn that ended just before them (Devin's rewind), one more
/// after a pause, which closes it and is nothing anywhere else. A key the
/// host did not take is not pressed again. Whether the host took any key at
/// all: one it refused, or never answered, is no press, whatever the harness
/// would have done with it.
pub(crate) async fn press_interrupt(
    host: &dyn EngineHost,
    time: &dyn Time,
    pane: &Pane,
    keys: Interrupt,
) -> bool {
    let escape = || async {
        let mut body = pane_body(pane);
        body["bytes"] = json!([ESCAPE]);
        let answer = host.request("pane.input", body).await;
        answer.is_ok_and(|answer| answer.get("ok") == Some(&Value::Bool(true)))
    };
    let mut taken = false;
    for press in 0..keys.presses {
        if press > 0 {
            time.sleep(DOUBLE_PRESS).await;
        }
        taken |= escape().await;
    }
    if let Some(after) = keys.close_after {
        time.sleep(after).await;
        taken |= escape().await;
    }
    taken
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
/// how its pane opens, and the stop its task had asked when the first message
/// was begun, which the new window owes nothing for.
struct Planned {
    window: Rc<dyn Window>,
    keys: Interrupt,
    argv: Vec<String>,
    env: Vec<(String, String)>,
    drop_env: Vec<String>,
    native_session: Option<String>,
    stop: Option<Stop>,
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
            window.stopped = None;
            window.interrupted = None;
            window.unstopped = None;
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

    /// What a window does now, told to the trace and the board when it
    /// changed. A trace that could not be told fails it, the window's
    /// activity changed all the same.
    pub(crate) fn set_activity(
        &self,
        record: &Record,
        activity: Activity,
    ) -> Result<(), EngineError> {
        {
            let mut window = record.window.borrow_mut();
            if window.activity == activity {
                return Ok(());
            }
            window.activity = activity.clone();
        }
        self.trace_window(
            record,
            WindowEvent::Activity {
                state: activity.state.as_str().to_owned(),
                reason: activity.reason,
            },
        )?;
        self.changed();
        Ok(())
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
        // A pause that came while the launch was prepared stopped the task this
        // message was to go in on: no window opens for it, and it waits for the
        // words that resume the task.
        if participant.role != "chief"
            && !self.launch_holds(participant, delivering.as_ref(), planned.stop)?
        {
            self.seams.launch_files.forget(&launch_id);
            if let Some(delivering) = delivering {
                self.give_back(delivering, "a pause came while its window was prepared")?;
            }
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
        // `.catch(...)` made a promise of its own to wait on, a turn after
        // the host's answer.
        let opened = returning(self.seams.host.open(OpenPane {
            pane: pane.clone(),
            cwd: project.directory.clone(),
            argv: planned.argv.clone(),
            env,
            drop_env: planned.drop_env.clone(),
        }))
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
                Box::pin(self.exited(&pane)).await?;
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
            part.stopped = planned.stop.map(|stop| (stop.task_id, stop.seq));
        }
        record.delivery.borrow_mut().delivering = delivering.clone();
        // A window that exited before its open was answered goes as any exit does.
        if exited {
            Box::pin(self.exited(&pane)).await?;
            return Ok(());
        }
        // The adapter's `started` is an `async` function, and `.catch(...)`
        // made a promise of its own to wait on.
        let started = caught(planned.window.started()).await;
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
        let begun = first
            .map(|first| self.seams.ledger.borrow_mut().begin_delivery(first.id))
            .transpose()?;
        // The stops this window owes are those asked before this look: a pause
        // that comes while it is prepared or opened is asked after it, and is
        // owed. Read in this turn, with the first message begun: the window
        // works on the task that message is about.
        let stop = self.seams.ledger.borrow().stop_of(participant.id)?;
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
        let text = begun
            .as_ref()
            .map(|begun| self.launch_text(project, participant, begun, resume))
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
        // An `async` function: its answer, or its failure, reaches this
        // step a turn after it was made, though Pi's never waits.
        let prepared = returning(adapter.prepare(&launch))
            .await
            .map_err(|cause| EngineError::said("launch-failed", cause))?;
        Ok(Some(Planned {
            window: prepared.window,
            keys: adapter.interrupt(),
            argv: prepared.argv,
            env: prepared.env,
            drop_env: prepared.drop_env,
            native_session: prepared.native_session,
            stop,
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

    /// Whether a launch prepared for its first message still holds: the
    /// message is still on its way, and no stop was asked of its window since
    /// the launch began, which is a pause that came in between.
    fn launch_holds(
        &self,
        participant: &ParticipantView,
        delivering: Option<&Delivering>,
        captured: Option<Stop>,
    ) -> Result<bool, EngineError> {
        let Some(delivering) = delivering else {
            return Ok(true);
        };
        let ledger = self.seams.ledger.borrow();
        let on_its_way = ledger
            .message(delivering.message)?
            .is_some_and(|message| message.state == "delivering");
        Ok(on_its_way && ledger.stop_of(participant.id)? == captured)
    }

    /// What a window's first message reads, which `begin_delivery` told: a
    /// member's fresh session starts from nothing, so when its first message is
    /// not the task's own brief (a reopening, an answer) the brief goes in
    /// first, as it was received with what its paste carried; a brief that
    /// was never received, held for the human's approval or not, is not the
    /// window's to read before it is. A resumed conversation has it already.
    fn launch_text(
        &self,
        project: &ProjectView,
        participant: &ParticipantView,
        first: &Begun,
        resume: Option<&str>,
    ) -> Result<String, EngineError> {
        let text = delivery_text(&first.message, &first.carried);
        let Some(number) = first
            .message
            .task_number
            .filter(|_| resume.is_none() && participant.role != "chief")
        else {
            return Ok(text);
        };
        let ledger = self.seams.ledger.borrow();
        let Some(thread) = ledger.task(project.id, number)? else {
            return Ok(text);
        };
        // Only a brief that arrived is found, so it is neither the message
        // being sent now nor one that rides in it.
        Ok(
            match ledger.first_received(participant.id, thread.task.id)? {
                Some(brief) => format!(
                    "{}\n\n{text}",
                    delivery_text(&brief.message, &brief.carried)
                ),
                None => text,
            },
        )
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
        let observed = returning(window.observe()).await?;
        record.window.borrow_mut().settled = observed.settled;
        Ok(observed)
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
            // `.catch(() => null)` made a promise of its own to wait on.
            Some(pane) => returning(self.seams.host.request("pane.snapshot", pane_body(&pane)))
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
            Some(pane) => self.kill(record, &pane).await?,
            None => false,
        };
        if !killed {
            record.window.borrow_mut().retiring = false;
        }
        self.changed();
        Ok(killed)
    }

    /// Kills a window: whether the pane host took the kill; the trace says
    /// why not, and a trace that cannot be told fails the kill.
    async fn kill(self: &Rc<Self>, record: &Rc<Record>, pane: &Pane) -> Result<bool, EngineError> {
        // `.catch(...)` made a promise of its own to wait on, a turn after
        // the host's answer.
        let killed = returning(self.seams.host.kill(pane)).await;
        let error = match killed {
            Ok(Killed::Killed) => return Ok(true),
            Ok(Killed::Refused { error }) => error,
            Err(error) => error.message,
        };
        self.trace_window(record, WindowEvent::KillFailed { error: Some(error) })?;
        Ok(false)
    }

    /// A member's window closes once it has no task in hand, unless the human
    /// opened it or a message is still on its way in. A follow-up that waits
    /// on the board for what it needs is not in hand: the session is its
    /// already, but its window opens again, on its conversation, when the
    /// follow-up goes, and stays closed meanwhile. One the human opened and
    /// has hidden since waits for its agent's turn to end first. Says whether
    /// it closed; one gone already has nothing to close.
    pub(crate) async fn close_if_free(
        self: &Rc<Self>,
        record: &Rc<Record>,
    ) -> Result<bool, EngineError> {
        let kept = {
            let part = record.window.borrow();
            part.pane.is_none() || part.pinned || (part.hidden && !part.settled)
        } || record.delivery.borrow().delivering.is_some()
            || self.seams.ledger.borrow().has_task_in_hand(record.id)?;
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
    /// host refused stays as it was, and no exit is made up for it. An exit
    /// that cannot be settled fails the close, which is the caller's to hear
    /// (the host's own exit event, with no caller, is told to the log).
    pub(crate) async fn close_own(
        self: &Rc<Self>,
        record: &Rc<Record>,
        pane: &Pane,
    ) -> Result<bool, EngineError> {
        record.window.borrow_mut().own_exit = true;
        let went = self.kill_and_settle(record, pane).await;
        record.window.borrow_mut().own_exit = false;
        went
    }

    async fn kill_and_settle(
        self: &Rc<Self>,
        record: &Rc<Record>,
        pane: &Pane,
    ) -> Result<bool, EngineError> {
        let retiring = record.window.borrow().retiring;
        if !retiring && !self.kill(record, pane).await? {
            return Ok(false);
        }
        let open = record.window.borrow().pane.as_ref() == Some(pane);
        if open {
            // An exit may close windows in turn: boxed, so the futures of
            // closing and exiting are not each the other's inside.
            Box::pin(self.exited(pane)).await?;
        }
        Ok(true)
    }
}

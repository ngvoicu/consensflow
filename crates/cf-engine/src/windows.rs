//! A window's lifecycle: launch (prepare, token, `pane.open`, conversation,
//! `started`), look (`observe`, `drawn`), and close (`retire`, `close_if_free`,
//! `close_own`). It owns the record's window part, the generation of each pane,
//! and the chief's relaunch backoff. What a window owes a stop of its task, and
//! how it is interrupted, is `stops`; the work its turn left running, which is
//! ended as its task's result is taken, is `background`.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use cf_harness::contract::{Interrupt, Launch, LaunchId, Observed, Pane, Window};
use cf_harness::seams::Time;
use cf_ledger::{
    Begun, ConversationView, MessageView, NewNote, ParticipantView, ProjectView, Stop,
};
use cf_proto::agents::Harness;
use cf_proto::panes::Servers;
use cf_proto::trace::WindowEvent;
use serde_json::{json, Value};

use crate::deliveries::Delivering;
use crate::delivery_text::{brief_first, delivery_text};
use crate::dispatcher::Dispatcher;
use crate::host::{EngineHost, Killed, OpenPane, Opened};
use crate::record::Record;
use crate::role_change::{Compared, Prelude};
use crate::runtime::{caught, returning};
use crate::seams::{EngineError, SavedAgent};
use crate::shown::Shown;
use crate::specs::{ChiefSpecs, RoleText};
use crate::stops::{Interrupted, Unstopped};
use crate::trust::Trusting;
use crate::unstarted::GivenUp;

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

/// A pane on its way open, and, if its exit came before the host's answer
/// (the host watches a window's process before it answers), what the host
/// said of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Opening {
    pub(crate) pane: Pane,
    pub(crate) exited: Option<Shown>,
}

/// The chief's window failing to start, tried again ever more slowly: how
/// often it failed, and when it may be tried next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Relaunch {
    pub(crate) failures: u32,
    pub(crate) at: i64,
    /// A window of the chief was given up for not coming up since one last came
    /// up: the next opens at its time whatever waits for it (`Dispatcher::step`).
    pub(crate) reopens: bool,
}

/// The record's window part.
pub(crate) struct WindowPart {
    /// The harness's window on its launch (the adapter's `launch` bag).
    pub(crate) window: Option<Rc<dyn Window>>,
    /// The keys that interrupt a turn in it (its adapter's `interrupt`).
    pub(crate) keys: Option<Interrupt>,
    /// How its harness keeps its own processes (its adapter's `servers`), and
    /// which harness it is, which the pane host is told where a turn begins.
    pub(crate) servers: Servers,
    pub(crate) harness: String,
    pub(crate) pane: Option<Pane>,
    pub(crate) opening: Option<Opening>,
    /// How many times a window was asked for it, counted as its window is asked for
    /// ([`Dispatcher::open_soon`]) and as each launch begins, before either waits on
    /// anything. The exit of an earlier window that read its record with no window
    /// open compares the count it began with: a window asked for meanwhile, up, on its
    /// way or given up, is no business of that exit's (`in_flight`).
    pub(crate) launches: u64,
    pub(crate) launch_id: Option<LaunchId>,
    /// The window's token, for its `cf`.
    pub(crate) token: Option<String>,
    /// A kill is on its way.
    pub(crate) retiring: bool,
    /// Its exit is the engine's own (`close_own`): a chief's closes no project.
    pub(crate) own_exit: bool,
    /// Why the engine closes it, where the pause note of the task it was working
    /// on is to say more than that it closed: with its project, when the chief's
    /// window closed by itself.
    pub(crate) closed_with: Option<String>,
    /// The human opened it (Show): it stays open.
    pub(crate) pinned: bool,
    /// The human hid it since: it closes once free and its turn settled.
    pub(crate) hidden: bool,
    /// Whether its last look said its turn settled.
    pub(crate) settled: bool,
    pub(crate) relaunch: Option<Relaunch>,
    /// How many of its windows in a row were given up for not coming up in the time
    /// they had: the next is given longer (`Launch::starts_given_up`). One that comes
    /// up starts the count again.
    pub(crate) starts_given_up: u32,
    /// It printed, then held still: its screen is drawn.
    pub(crate) drawn: bool,
    /// It named its first conversation.
    pub(crate) named: bool,
    /// Where it stands with its harness's question about trusting its folder.
    pub(crate) trust: Trusting,
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
    /// The pane of the last window that ended, and whether the host proved its
    /// program gone ([`Shown::gone`]): none until the host said, which it may
    /// do after the engine's own close has settled the window.
    pub(crate) last_exit: Option<(Pane, Option<bool>)>,
}

impl Default for WindowPart {
    fn default() -> Self {
        Self {
            window: None,
            keys: None,
            servers: Servers::Unknown,
            harness: String::new(),
            pane: None,
            opening: None,
            launches: 0,
            launch_id: None,
            token: None,
            retiring: false,
            own_exit: false,
            closed_with: None,
            pinned: false,
            hidden: false,
            settled: false,
            relaunch: None,
            starts_given_up: 0,
            drawn: false,
            named: false,
            trust: Trusting::Past,
            stopped: None,
            interrupted: None,
            unstopped: None,
            activity: Activity::of(ActivityState::Closed),
            last_exit: None,
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
/// how its harness keeps its own processes and which harness it is, how its pane
/// opens, the conversation it goes on with if there is one, and the stop its task
/// had asked when the first message was begun, which the new window owes nothing
/// for.
struct Planned {
    window: Rc<dyn Window>,
    keys: Interrupt,
    servers: Servers,
    harness: String,
    argv: Vec<String>,
    env: Vec<(String, String)>,
    drop_env: Vec<String>,
    native_session: Option<String>,
    conversation: Option<ConversationView>,
    stop: Option<Stop>,
    /// What goes into the window before anything else of its queue once it is up,
    /// in this order ([`Compared::prelude`]).
    prelude: Option<Prelude>,
    /// The text the window starts with, for a harness that keeps the first
    /// instructions of its conversations: what a conversation this launch starts
    /// is recorded as begun with.
    first_instructions: Option<String>,
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
            window.trust = Trusting::Past;
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
    /// there is one (a chief's is its handoff, most often). A conversation that
    /// is resumed and whose role changed, on a harness that keeps its first
    /// instructions, has no first message on its command line: the note that says
    /// the role goes in first once the window is up, and `message` after it
    /// ([`Dispatcher::compare_role`]); the launch owes its window that note until it
    /// is told, and a window that exits or is never ready before is a launch that
    /// did not come up ([`Prelude`]). Whether it is resumed is decided first: a
    /// conversation its harness holds nothing of opens afresh, and has no role to
    /// be told ([`Dispatcher::conversation_to_go_on_with`]). One that does not come
    /// up says why ([`Dispatcher::launch_failed`]); a participant forgotten while
    /// its launch waits gets nothing more under its ids. It is counted before
    /// anything ([`WindowPart::launches`]): its preparation waits on its harness, and
    /// an earlier window's exit that reads its record meanwhile is to see a window on
    /// its way (a review of 2026-10-16: the exit of a session's window paused the task
    /// the window the human opened again was being prepared for).
    pub(crate) async fn launch(
        self: &Rc<Self>,
        record: &Rc<Record>,
        project: &ProjectView,
        participant: &ParticipantView,
        message: Option<MessageView>,
    ) -> Result<(), EngineError> {
        record.window.borrow_mut().launches += 1;
        let harness = participant.harness.clone().unwrap_or_default();
        // A role the window took before it closed, though its arrival was never
        // seen, is what the conversation was told: the ledger learns it first.
        let settled = self.settle_replacements(participant).await?;
        if self.forgotten(record) {
            return Ok(());
        }
        let message = message.filter(|message| !settled.contains(&message.id));
        // A chief's first message is its handoff or not by the conversation it goes
        // on with, so that is looked into first, and then the role its window is
        // given is compared with the one that conversation was told: a conversation
        // its harness holds nothing of is none the window goes on with, has nothing
        // to be told, and gets no note. A member's does not wait for it, and its
        // conversation is looked into once the message is begun, after which its
        // role is compared ([`Dispatcher::plan`]).
        let mut conversation = None;
        let mut compared = Compared {
            first: message,
            prelude: None,
            role: None,
        };
        if participant.role == "chief" {
            conversation = self.conversation_to_go_on_with(participant).await?;
            let first =
                self.chief_first(project, participant, conversation.as_ref(), compared.first)?;
            let resumed = conversation
                .as_ref()
                .filter(|conversation| conversation.native_session.is_some());
            compared = self.compare_role(project, participant, resumed, first)?;
        }
        let Compared {
            first,
            prelude,
            role,
        } = compared;
        let launch_id = self.seams.launch_ids.draw();
        let generation = self.next_generation();
        let delivering = first.as_ref().map(|first| Delivering {
            message: first.id,
            since: self.now(),
            // A launch's first message is waited for by the launch's own time.
            rested_since: None,
            launch: true,
            chief: participant.role == "chief",
            queued: false,
            free_since: None,
            entered_again: false,
        });
        // What fails with a launch that does not come up: its first message, or
        // the one that would have been, which the role goes ahead of. A member's
        // is its first message in any case: where its role is to be said first,
        // that is found in the plan, and the message is given back to its queue.
        let failing = delivering
            .clone()
            .or_else(|| self.displaced(participant, prelude.as_ref()));
        // Boxed, so that the work of an operation that launches stays small
        // (`dispatcher::tests`): the plan asks the harness and prepares the window.
        let planned = match Box::pin(self.plan(
            record,
            project,
            participant,
            first.as_ref(),
            conversation,
            &launch_id,
            delivering.as_ref(),
            role,
            prelude,
        ))
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
                    failing,
                    &format!("the launch failed: {cause}"),
                );
            }
        };
        // A member's role was compared once its harness had said which conversation
        // its window goes on with, and where the role is to be said first the
        // message the launch was for went back to its queue: the window opens with
        // no first message, and that one goes after the note that says the role.
        let prelude = planned.prelude.clone();
        let delivering = delivering.filter(|_| {
            prelude
                .as_ref()
                .is_none_or(|prelude| prelude.displaced.is_none())
        });
        if self.launch_over(
            record,
            participant,
            delivering.as_ref(),
            planned.stop,
            "a pause came while its window was prepared",
        )? {
            self.seams.launch_files.forget(&launch_id);
            return Ok(());
        }
        let resume = planned
            .conversation
            .as_ref()
            .and_then(|conversation| conversation.native_session.clone());
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
            exited: None,
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
            .and_then(|opening| opening.exited);
        let pid = match opened {
            Ok(Opened::Open { pid }) => pid,
            Ok(Opened::Refused { error }) => {
                return self.not_opened(
                    record,
                    project,
                    participant,
                    failing,
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
                    failing,
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
            if let Some(shown) = exited {
                Box::pin(self.exited(&pane, shown)).await?;
            }
            return Ok(());
        }
        let resumed = resume.is_some() && planned.native_session == resume;
        let mut conversation_id = planned
            .conversation
            .as_ref()
            .map(|conversation| conversation.id);
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
            if let Some(instructions) = planned.first_instructions.as_deref() {
                self.seams
                    .ledger
                    .borrow_mut()
                    .set_conversation_role_text(started.id, instructions)?;
            }
        }
        {
            let mut part = record.window.borrow_mut();
            part.window = Some(Rc::clone(&planned.window));
            part.keys = Some(planned.keys);
            part.servers = planned.servers;
            part.harness.clone_from(&planned.harness);
            part.pane = Some(pane.clone());
            part.launch_id = Some(launch_id);
            part.token = Some(token);
            part.activity = Activity::of(ActivityState::Starting);
            part.drawn = false;
            part.named = false;
            part.stopped = planned.stop.map(|stop| (stop.task_id, stop.seq));
        }
        {
            let mut delivery = record.delivery.borrow_mut();
            delivery.delivering = delivering;
            delivery.prelude = prelude;
        }
        // A window that exited before its open was answered goes as any exit does.
        if let Some(shown) = exited {
            Box::pin(self.exited(&pane, shown)).await?;
            return Ok(());
        }
        // A harness that asks whether to trust the window's folder is answered, or
        // the human told, as its window shows the question (`trust`).
        self.watch_trust(record, project, participant, &pane).await;
        // The adapter's `started` is an `async` function, and `.catch(...)`
        // made a promise of its own to wait on.
        let started = match caught(planned.window.started()).await {
            // A window that never came up is given up, with its first message or
            // without one: a Codex chief opened again at rest after the app's restart
            // has none, and read "starting" for good (zeewin, 2026-10-19). What it
            // shows is asked first, for the screen goes with it, and its harness once
            // more after that: one that came up in that last moment is kept (Sol on
            // round 1 of the chief's restart).
            Err(error) => {
                let shown = self.look_at(&pane).await;
                match returning(planned.window.came_up()).await {
                    Some(native) => Ok(native),
                    None => {
                        let given_up = GivenUp {
                            pane,
                            failing,
                            error,
                            shown,
                        };
                        return self
                            .give_up_unstarted(record, project, participant, given_up)
                            .await;
                    }
                }
            }
            started => started,
        };
        record.window.borrow_mut().starts_given_up = 0;
        if self.forgotten(record) {
            return Ok(());
        }
        // A launch has not come up until its window is ready for work, and one that
        // is to say the role not until it has: a chief whose window was given up for
        // either, and is tried again, keeps its count of failures, so that its wait
        // grows and the human is told once. The human's Resume and switch clear it,
        // and so does the first readiness of a launch that has no role to say
        // ([`Dispatcher::came_up`]).
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
    /// it) or the launch is over before its window is made
    /// ([`Dispatcher::launch_over`]), or why the launch failed. `conversation` is
    /// the chief's, which was looked into before its first message was chosen.
    /// `role` is the text the window starts with where it was asked for already
    /// ([`Dispatcher::compare_role`]), and `prelude` what goes into the window
    /// first where it was compared: a chief's was, before its first message was
    /// chosen, and a member's is, here, once its harness has said which
    /// conversation its window goes on with.
    #[allow(clippy::too_many_arguments)]
    async fn plan(
        self: &Rc<Self>,
        record: &Rc<Record>,
        project: &ProjectView,
        participant: &ParticipantView,
        first: Option<&MessageView>,
        conversation: Option<ConversationView>,
        launch_id: &LaunchId,
        delivering: Option<&Delivering>,
        role: Option<RoleText>,
        prelude: Option<Prelude>,
    ) -> Result<Option<Planned>, EngineError> {
        let begun = first
            .map(|first| self.seams.ledger.borrow_mut().begin_delivery(first.id))
            .transpose()?;
        // The stops this window owes are those asked before this look: a pause
        // that comes while it is prepared or opened is asked after it, and is
        // owed. Read in this turn, with the first message begun: the window
        // works on the task that message is about.
        let stop = self.seams.ledger.borrow().stop_of(participant.id)?;
        // A member's conversation is looked into now, which asks its harness and
        // may wait: a pause that comes during it is asked after the stop was read,
        // as one that comes while the window is prepared is, and the launch is over.
        let conversation = if participant.role == "chief" {
            conversation
        } else {
            let found = self.conversation_to_go_on_with(participant).await?;
            if self.launch_over(
                record,
                participant,
                delivering,
                stop,
                "a pause came while its harness was asked about its conversation",
            )? {
                return Ok(None);
            }
            found
        };
        // A member's role is compared now that its conversation is known: one its
        // harness holds nothing of is not resumed, and has nothing to be told. Where
        // the role is to be said first, the message the launch was for is no first
        // message of its window: it goes back to its queue with its attempt, to go
        // in after the note that says the role. It is still what a launch that does
        // not come up fails ([`Dispatcher::launch_failed`]).
        let (begun, role, prelude) = if participant.role == "chief" {
            (begun, role, prelude)
        } else {
            let resumed = conversation
                .as_ref()
                .filter(|conversation| conversation.native_session.is_some());
            let compared = self.compare_role(project, participant, resumed, first.cloned())?;
            let begun = match self.displaced(participant, compared.prelude.as_ref()) {
                Some(displaced) => {
                    self.give_back(displaced, "its window is told its role first")?;
                    None
                }
                None => begun,
            };
            (begun, compared.role, compared.prelude)
        };
        let resume = conversation
            .as_ref()
            .and_then(|conversation| conversation.native_session.as_deref());
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
            .map(|begun| self.launch_text(begun, resume))
            .transpose()?;
        // A chief whose project has specs on is told where its Specmint skill
        // is, and its window is given the skill there.
        let RoleText {
            specs,
            instructions,
        } = match role {
            Some(role) => role,
            None => self.role_text(project, participant, &*adapter)?,
        };
        // Its last window's program was proven gone: nothing of it writes on.
        let previous_gone = record
            .window
            .borrow()
            .last_exit
            .as_ref()
            .is_some_and(|(_, gone)| *gone == Some(true));
        let launch = Launch {
            id: launch_id,
            project: project.id,
            handle: &participant.handle,
            role: &participant.role,
            directory: &project.directory,
            resume,
            previous_gone,
            message: text.as_deref(),
            agent: saved.as_ref().map(SavedAgent::agent),
            instructions: &instructions,
            specs: specs.as_ref().map(ChiefSpecs::launch),
            first_message_ms: self.seams.limits.launch_ms,
            starts_given_up: record.window.borrow().starts_given_up,
        };
        // An `async` function: its answer, or its failure, reaches this
        // step a turn after it was made, though Pi's never waits.
        let prepared = returning(adapter.prepare(&launch))
            .await
            .map_err(|cause| EngineError::said("launch-failed", cause))?;
        Ok(Some(Planned {
            window: prepared.window,
            keys: adapter.interrupt(),
            servers: adapter.servers(),
            // The name the harness's own programs go by (`codex-…`): the roster's kind
            // is `claude-code` for the harness whose binary is `claude`.
            harness: Harness::from_kind(harness)
                .map(Harness::as_str)
                .unwrap_or_default()
                .to_owned(),
            argv: prepared.argv,
            env: prepared.env,
            drop_env: prepared.drop_env,
            native_session: prepared.native_session,
            conversation,
            stop,
            prelude,
            first_instructions: adapter.keeps_first_instructions().then_some(instructions),
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

    /// Whether a launch that waited is over, with no window to open. A
    /// participant forgotten while its launch waits (it left, or its project was
    /// deleted) gets nothing more under its ids, whose rows may be gone. A member's
    /// launch holds only while its first message is still on its way and no stop
    /// was asked of its window since `captured` was read, which is a pause that
    /// came in between: the message goes back to wait for the words that resume
    /// the task, with `because`.
    fn launch_over(
        &self,
        record: &Rc<Record>,
        participant: &ParticipantView,
        delivering: Option<&Delivering>,
        captured: Option<Stop>,
        because: &str,
    ) -> Result<bool, EngineError> {
        if self.forgotten(record) {
            return Ok(true);
        }
        let Some(delivering) = delivering.filter(|_| participant.role != "chief") else {
            return Ok(false);
        };
        let holds = {
            let ledger = self.seams.ledger.borrow();
            let on_its_way = ledger
                .message(delivering.message)?
                .is_some_and(|message| message.state == "delivering");
            on_its_way && ledger.stop_of(participant.id)? == captured
        };
        if holds {
            return Ok(false);
        }
        self.give_back(delivering.clone(), because)?;
        Ok(true)
    }

    /// What a window's first message reads, which `begin_delivery` told: a
    /// member's fresh session starts from nothing, so the brief its task's
    /// window received goes in first ([`Dispatcher::brief_given_first`]). A
    /// resumed conversation has it already.
    fn launch_text(&self, first: &Begun, resume: Option<&str>) -> Result<String, EngineError> {
        let text = delivery_text(&first.message, &first.carried);
        Ok(match self.brief_given_first(&first.message)? {
            Some(brief) if resume.is_none() => format!("{brief}{text}"),
            _ => text,
        })
    }

    /// What a member's fresh window is given ahead of `message`, its first, when
    /// that is not its task's own brief (a reopening, an answer): the brief it
    /// received, as it was received with what its paste carried, and a blank line
    /// ([`brief_first`]). A brief that was never received, held for the human's
    /// approval or not, is not the window's to read before it is; only a brief
    /// that arrived is found, so it is neither `message` nor one that rides in it.
    /// None for the chief's, and where no brief of the task was received. The
    /// paste of `message` in its record is behind it (`is_paste_of`).
    pub(crate) fn brief_given_first(
        &self,
        message: &MessageView,
    ) -> Result<Option<String>, EngineError> {
        let Some(number) = message
            .task_number
            .filter(|_| message.recipient_role != "chief")
        else {
            return Ok(None);
        };
        let ledger = self.seams.ledger.borrow();
        let Some(thread) = ledger.task(message.project_id, number)? else {
            return Ok(None);
        };
        Ok(ledger
            .first_received(message.recipient_id, thread.task.id)?
            .map(|brief| brief_first(&brief.message, &brief.carried)))
    }

    /// A launch that did not come up. A member's first message fails with
    /// it, so its task fails and the requester hears why; a window the human
    /// opened with nothing to deliver tells them why it did not come. The
    /// chief's first message goes back to its queue with its attempt, and
    /// the chief is tried again, ever more slowly while it keeps failing;
    /// the human hears why once. Where a role was to be said first, a member's
    /// failing message is the one it went ahead of, and the note that says the
    /// role is withdrawn with it; a chief's stays in its queue (`role_change`).
    /// A participant forgotten while it launched hears nothing and settles
    /// nothing.
    pub(crate) fn launch_failed(
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
            // What it was to say of the role first was written for a window that
            // never came up, and no queue of a member's would give it to another.
            self.withdraw_replacements(participant)?;
            match delivering {
                Some(delivering) => {
                    // One the role went ahead of waits still: it fails now, or
                    // the launch would be tried again at every pass.
                    self.begin_displaced(&delivering)?;
                    self.settle_failure(delivering, because, false)?;
                }
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
        let before = record.window.borrow().relaunch;
        let failures = before.map_or(0, |relaunch| relaunch.failures) + 1;
        let wait = RELAUNCH_MS
            .saturating_mul(2_i64.saturating_pow(failures - 1))
            .min(RELAUNCH_MAX_MS);
        record.window.borrow_mut().relaunch = Some(Relaunch {
            failures,
            at: self.now() + wait,
            // Until a window comes up, as a launch that fails for another reason after it.
            reopens: before.is_some_and(|relaunch| relaunch.reopens),
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

    /// A window said it is ready for a delivery: its launch came up, and a chief
    /// that failed to start before has its count of failures cleared, the next
    /// failure told the human again. A launch that is to say the role first has not
    /// come up until it has, and keeps its count (`role_change`).
    pub(crate) fn came_up(&self, record: &Record) {
        if record.delivery.borrow().prelude.is_none() {
            record.window.borrow_mut().relaunch = None;
        }
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
            // closing and exiting are not each the other's inside. The engine
            // closed it, so it knows no more of what it showed than it did.
            Box::pin(self.exited(pane, Shown::default())).await?;
        }
        Ok(true)
    }
}

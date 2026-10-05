//! A window's lifecycle (`src/core/windows.js`): launch (prepare, token,
//! `pane.open`, conversation, `started`), look (`observe`, `drawn`),
//! interrupt (Escape rounds), and close (`retire`, `close_if_free`,
//! `close_own`). It owns the record's window part, the generation of each
//! pane, and the chief's relaunch backoff.
//!
//! Landing C freezes what the dispatcher asks of it; landing D ports it.

use std::cell::Cell;
use std::rc::Rc;

use cf_harness::contract::{LaunchId, Observed, Pane, Window};
use cf_ledger::{MessageView, ParticipantView, ProjectView};
use cf_proto::trace::WindowEvent;

use crate::dispatcher::Dispatcher;
use crate::record::Record;
use crate::seams::EngineError;

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
    /// It named its first conversation.
    pub(crate) named: bool,
    pub(crate) activity: Activity,
}

impl Default for WindowPart {
    fn default() -> Self {
        Self {
            window: None,
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
            named: false,
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

    /// The next pane's generation: past the last one, and no earlier than
    /// now, so a window opened after a restart takes no old one's.
    #[expect(dead_code, reason = "a window's launch draws it, in landing D")]
    pub(crate) fn next_generation(&self) -> i64 {
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

    /// Launches `participant`'s window, `message` its first if there is one.
    pub(crate) async fn launch(
        self: &Rc<Self>,
        _record: &Rc<Record>,
        _project: &ProjectView,
        _participant: &ParticipantView,
        _message: Option<MessageView>,
    ) -> Result<(), EngineError> {
        Err(not_ported("a window's launch"))
    }

    /// One look at a window: what its harness says of it now, or why that
    /// could not be told.
    pub(crate) async fn observe(
        self: &Rc<Self>,
        _participant: &ParticipantView,
        _record: &Rc<Record>,
    ) -> Result<Observed, String> {
        Err("a window's look is not ported yet".to_owned())
    }

    /// Whether a window's screen is drawn: its output quiet long enough.
    pub(crate) async fn drawn(self: &Rc<Self>, _record: &Rc<Record>) -> bool {
        false
    }

    /// A window whose task stopped (paused, or cancelled under it) has its
    /// agent interrupted.
    pub(crate) async fn interrupt_if_stopped(
        self: &Rc<Self>,
        _participant: &ParticipantView,
        _record: &Rc<Record>,
        _observed: &Observed,
    ) -> Result<(), EngineError> {
        Err(not_ported("a window's interrupt"))
    }

    /// A session's window closes: whether it goes (one already closing had
    /// its kill; one whose kill the host refused stays, not closing).
    pub(crate) async fn retire(self: &Rc<Self>, _record: &Rc<Record>) -> Result<bool, EngineError> {
        Err(not_ported("a window's close"))
    }

    /// A member's window closes once it holds no task, unless the human
    /// opened it or a message is still on its way in: whether it closed.
    pub(crate) async fn close_if_free(
        self: &Rc<Self>,
        _record: &Rc<Record>,
    ) -> Result<bool, EngineError> {
        Err(not_ported("a free window's close"))
    }

    /// The engine closes `pane`, its exit its own: whether it went.
    pub(crate) async fn close_own(
        self: &Rc<Self>,
        _record: &Rc<Record>,
        _pane: &Pane,
    ) -> Result<bool, EngineError> {
        Err(not_ported("the engine's own close"))
    }
}

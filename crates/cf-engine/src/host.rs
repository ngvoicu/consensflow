//! The pane host as the engine uses it: besides the requests an adapter makes
//! of a window's pane (`PaneHost`), the engine opens a window's pane, kills it,
//! and hears when it exits.
//!
//! A host's answer is told apart from a failure to get one: a pane the host
//! refused to open or kill says so in the host's own words (which the human
//! and the trace are told), where a request that never came back is a
//! [`HostError`]. Exits come in the order the host sent them, each told to
//! the engine where it is read (`Dispatcher::pane_exited`), before the next frame, so an exit
//! that came before its open's answer is known by the time the answer is.

use cf_harness::contract::{HostError, Pane, PaneHost, Work};
use cf_proto::panes::SilentPaste;
use serde_json::Value;

/// What a pane is opened with (`pane.open`): the window's pane, the folder its
/// program starts in, the program and its arguments, the environment the
/// engine gives it (the daemon's, the window's own, then its token), the
/// variables it drops, and how its harness reads a human's paste where it reads one
/// silently ([`Adapter::silent_paste`](cf_harness::contract::Adapter::silent_paste)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenPane {
    pub pane: Pane,
    pub cwd: String,
    pub argv: Vec<String>,
    pub env: Vec<(String, String)>,
    pub drop_env: Vec<String>,
    pub silent_paste: Option<SilentPaste>,
}

/// What the host answered an open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Opened {
    /// The pane is open, its program's process id when the host knows it.
    Open { pid: Option<u32> },
    /// The host would not open it, in its own words: nothing of it ran.
    Refused { error: String },
    /// The host did not answer by the open's deadline. It goes on with an open it was
    /// asked for, so the window's program may have started, or may start later
    /// (kluris, 2026-10-26): the engine ends it ([`EngineHost::kill`]).
    Unanswered,
}

/// What the host answered a kill.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Killed {
    /// The pane is on its way out; its exit comes as an exit does.
    Killed,
    /// The host would not kill it, in its own words: the window stays as it
    /// was, and no exit comes.
    Refused { error: String },
}

/// The pane host the engine works through.
pub trait EngineHost: PaneHost {
    /// Opens `open.pane`, running its program. The transport bounds the wait
    /// (60 s on the daemon's bridge).
    fn open(&self, open: OpenPane) -> Work<'_, Result<Opened, HostError>>;
    /// Kills `pane`'s program.
    fn kill<'a>(&'a self, pane: &'a Pane) -> Work<'a, Result<Killed, HostError>>;
    /// How long what `pane`'s screen shows has not changed, in milliseconds, as the host's
    /// `pane.snapshot` says it (`screenQuietMs`): none where its screen has shown nothing yet
    /// or the host does not say. The engine's look at whether a window's screen is still
    /// being drawn before it closes the window of a task that is done
    /// (`Dispatcher::close_if_free`), and at whether the screen of a window read at rest by
    /// the pacing's fallback is quiet, for its lane to say (`Dispatcher::fallback_lane`).
    /// It is a read of the screen and changes nothing: the Node traces, which hold every
    /// call of the host the engine makes, have none of it, so a fake writes none down.
    fn quiet<'a>(&'a self, pane: &'a Pane) -> Work<'a, Result<Option<f64>, HostError>>;
    /// What `pane`'s screen shows now: the host's `pane.snapshot` of its last
    /// [`cf_proto::panes::TAIL_LINES`] lines, as it answers it. The engine's look for a dialog
    /// of the window's harness that waits for a key (`Dispatcher::watch_dialog`, T-1183). A
    /// read of the screen that changes nothing, which the Node traces have none of, as
    /// [`EngineHost::quiet`].
    fn screen<'a>(&'a self, pane: &'a Pane) -> Work<'a, Result<Value, HostError>>;
    /// How many alerts of the system's notification centre are on the Mac's screen now (a
    /// macOS privacy dialog raised in the app's name is one): none said where the host does
    /// not say (a Windows host, an older one, a Mac with no window server). The engine's look
    /// at whether the dialog a window waits on is still up (`Dispatcher::watch_privacy`, T-1194).
    /// A read that changes nothing, which the Node traces have none of, as [`EngineHost::screen`].
    fn alerts(&self) -> Work<'_, Result<Option<usize>, HostError>>;
}

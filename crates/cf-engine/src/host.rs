//! The pane host as the engine uses it (`src/core/pane-host.js`): besides the
//! requests an adapter makes of a window's pane (`PaneHost`), the engine opens
//! a window's pane, kills it, and hears when it exits.
//!
//! A host's answer is told apart from a failure to get one: a pane the host
//! refused to open or kill says so in the host's own words (which the human
//! and the trace are told), where a request that never came back is a
//! [`HostError`]. Exits come in the order the host sent them, each told to
//! the engine where it is read (`Dispatcher::pane_exited`), before the next frame, so an exit
//! that came before its open's answer is known by the time the answer is.

use cf_harness::contract::{HostError, Pane, PaneHost, Work};

/// What a pane is opened with (`pane.open`): the window's pane, the folder its
/// program starts in, the program and its arguments, the environment the
/// engine gives it (the daemon's, the window's own, then its token), and the
/// variables it drops.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenPane {
    pub pane: Pane,
    pub cwd: String,
    pub argv: Vec<String>,
    pub env: Vec<(String, String)>,
    pub drop_env: Vec<String>,
}

/// What the host answered an open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Opened {
    /// The pane is open, its program's process id when the host knows it.
    Open { pid: Option<u32> },
    /// The host would not open it, in its own words.
    Refused { error: String },
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
}

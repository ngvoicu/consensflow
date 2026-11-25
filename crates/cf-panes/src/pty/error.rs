//! What can go wrong with a pane table's windows.

use std::fmt;
use std::io;
use std::path::PathBuf;

use super::PaneKey;

#[derive(Debug)]
pub enum PaneError {
    EmptyArgv,
    Updating,
    UpdateBlocked,
    ProgramNotAbsolute(PathBuf),
    AlreadyOpen(PaneKey),
    NotFound(PaneKey),
    /// The daemon closed the window before its open reached the table.
    Cancelled(PaneKey),
    /// The window is still being ended by a close before this one.
    Ending(PaneKey),
    /// The daemon that asked for the window has stopped: it is not opened, and no program
    /// is started for it.
    Fenced(PaneKey),
    Pty(String),
    /// The system would not start the window's program: its words, and, where the failure is an
    /// I/O error, the system's name for it from libuv's table (`ENOENT`), which the daemon reads
    /// back as the kind of failure a note says (T-1328).
    Spawn {
        message: String,
        code: Option<&'static str>,
    },
    Io(io::Error),
    /// The window did not open, and ending what had started of it failed too: why it did not
    /// open, and why it could not be ended (it may still be in the table, or its program
    /// still run).
    OpenNotUndone {
        cause: Box<PaneError>,
        cleanup: Box<PaneError>,
    },
    InvalidBacklog,
    InvalidEnvironmentVariableName(String),
    NoOutputStream(PaneKey),
    /// A pull of history from a page that has not subscribed to the output.
    NotSubscribed,
    /// A size a window's journal can take no more of: 4,096 held and output unacknowledged.
    SizesHeld(PaneKey),
    FutureAck {
        key: PaneKey,
        seq: u64,
        highest_issued: u64,
    },
    LockPoisoned,
}

impl PaneError {
    /// The system's name for the failure, where the window's program could not be started and
    /// the system named why.
    pub fn code(&self) -> Option<&'static str> {
        match self {
            Self::Spawn { code, .. } => *code,
            Self::OpenNotUndone { cause, .. } => cause.code(),
            _ => None,
        }
    }
}

impl fmt::Display for PaneError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyArgv => write!(formatter, "argv must contain an absolute program path"),
            Self::Updating => write!(
                formatter,
                "ConsensFlow is installing an update; new panes cannot start"
            ),
            Self::UpdateBlocked => write!(
                formatter,
                "Close or suspend every open session and wait for pane cleanup before installing"
            ),
            Self::ProgramNotAbsolute(path) => {
                write!(formatter, "argv[0] must be absolute: {}", path.display())
            }
            Self::AlreadyOpen(key) => write!(
                formatter,
                "pane {} generation {} is already open",
                key.id, key.generation
            ),
            Self::NotFound(key) => write!(
                formatter,
                "pane {} generation {} is not open",
                key.id, key.generation
            ),
            Self::Cancelled(key) => write!(
                formatter,
                "pane {} generation {} was closed before it opened",
                key.id, key.generation
            ),
            Self::Ending(key) => write!(
                formatter,
                "pane {} generation {} is still being ended",
                key.id, key.generation
            ),
            Self::Fenced(key) => write!(
                formatter,
                "pane {} generation {} was not opened: the daemon that asked for it has stopped",
                key.id, key.generation
            ),
            Self::Pty(message) | Self::Spawn { message, .. } => {
                write!(formatter, "PTY error: {message}")
            }
            Self::Io(error) => write!(formatter, "PTY I/O error: {error}"),
            Self::OpenNotUndone { cause, cleanup } => {
                write!(
                    formatter,
                    "{cause}; ending the window failed too: {cleanup}"
                )
            }
            Self::InvalidBacklog => write!(formatter, "backlogBytes must be greater than zero"),
            Self::InvalidEnvironmentVariableName(name) => write!(
                formatter,
                "invalid environment variable name in dropEnv: {name:?}"
            ),
            Self::NotSubscribed => write!(formatter, "pane-not-subscribed"),
            Self::SizesHeld(_) => write!(formatter, "pane-sizes-held"),
            Self::NoOutputStream(key) => write!(
                formatter,
                "pane {} generation {} has no output stream",
                key.id, key.generation
            ),
            Self::FutureAck {
                key,
                seq,
                highest_issued,
            } => write!(
                formatter,
                "pane {} generation {} ack {} exceeds highest issued seq {}",
                key.id, key.generation, seq, highest_issued
            ),
            Self::LockPoisoned => write!(formatter, "pane table lock is poisoned"),
        }
    }
}

impl std::error::Error for PaneError {}

impl From<io::Error> for PaneError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

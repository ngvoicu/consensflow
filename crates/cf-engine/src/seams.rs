//! What the engine is made with (the options of Node's dispatcher): the ledger,
//! the pane host, an adapter for each harness, the records, the clock and the
//! launch ids, who a window's token is for, the environment a pane starts with,
//! the saved agents, the role texts, the trace, the log and the launch files.
//! The daemon gives the real ones (3.6); the kit gives fakes that write down
//! what they were asked.

use std::cell::RefCell;
use std::rc::Rc;

use cf_base::env::Env;
use cf_base::refusal::Refusal;
use cf_harness::contract::{Adapter, Agent, LaunchId, Records};
use cf_harness::seams::Time;
use cf_ledger::{Ledger, LedgerError, ParticipantView, ProjectView};
use cf_proto::trace::TraceLine;

use crate::host::EngineHost;
use crate::runtime::Spawn;

/// Why one of the engine's operations did not happen: a refusal in the
/// engine's own words, or the ledger's, kept whole.
#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("{0}")]
    Refused(Refusal),
    #[error("{0}")]
    Ledger(#[from] LedgerError),
}

impl From<Refusal> for EngineError {
    fn from(refusal: Refusal) -> Self {
        EngineError::Refused(refusal)
    }
}

impl EngineError {
    /// A refusal of the engine's, said as `message` (JavaScript threw an
    /// `Error` with it).
    pub(crate) fn said(code: &'static str, message: impl Into<String>) -> Self {
        EngineError::Refused(Refusal::new(code, message))
    }

    /// As the API answers it: the refusal, or the ledger's file failing.
    pub fn refusal(&self) -> Refusal {
        match self {
            EngineError::Refused(refusal) | EngineError::Ledger(LedgerError::Refused(refusal)) => {
                refusal.clone()
            }
            EngineError::Ledger(other) => {
                Refusal::with_status("ledger-failed", other.to_string(), 500)
            }
        }
    }
}

/// A window's token, for the `cf` in it to act as its participant.
pub trait Credentials {
    /// A token for `participant`'s window in `project` (`credentials.issue`).
    fn issue(&self, project: i64, participant: i64) -> String;
    /// A window's token acts no longer (`credentials.revoke`).
    fn revoke(&self, token: &str);
}

/// The daemon's trace: a line for each change at a window and each project
/// deleted, and a deleted project's lines forgotten. Sync, and called inside
/// a ledger write too.
pub trait Trace {
    fn line(&self, line: TraceLine);
    fn forget(&self, project: i64);
}

/// What the engine writes to the daemon's log: what failed apart from
/// anything that waits for it (`log.error`, a launch or a delivery that nobody
/// awaits), and what a window that did not come up showed (`log.warn`).
pub trait Log {
    fn error(&self, message: &str, cause: &str);
    /// Something that went wrong and was gone past, said in one line.
    fn warn(&self, message: &str);
}

/// The files a launch wrote, forgotten once no window will read them
/// (`launchFiles.forget`).
pub trait LaunchFiles {
    fn forget(&self, launch: &LaunchId);
}

/// A saved agent, as a launch and the chief's checks read it: the model it
/// runs and the levels its harness reads, and whether it is an image agent.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SavedAgent {
    pub model: Option<String>,
    pub effort: Option<String>,
    pub thinking: Option<String>,
    pub designer: bool,
}

impl SavedAgent {
    /// The agent as a launch is given it.
    pub fn agent(&self) -> Agent<'_> {
        Agent {
            model: self.model.as_deref(),
            effort: self.effort.as_deref(),
            thinking: self.thinking.as_deref(),
            designer: self.designer,
        }
    }
}

/// The saved agents, read at each launch (`roster`): an agent, none for one
/// the human deleted, or why the agents file could not be read.
pub trait Roster {
    fn agent(&self, name: &str) -> Result<Option<SavedAgent>, Refusal>;
}

/// The text each role's window starts with (`roles`), or why it has none,
/// which fails its launch.
pub trait Roles {
    fn instructions(
        &self,
        participant: &ParticipantView,
        project: &ProjectView,
    ) -> Result<String, String>;
}

/// The environment a window's pane starts with before its harness's own
/// (`paneEnv`), in order.
pub trait PaneEnv {
    fn env(&self, participant: &ParticipantView, project: &ProjectView) -> Vec<(String, String)>;
}

/// The adapter for a harness's windows, by the word the ledger names the
/// harness with: none for one this build opens no windows of.
pub trait Adapters {
    fn adapter(&self, harness: &str) -> Option<Rc<dyn Adapter>>;
}

/// The launch ids, each new (`randomUUID`).
pub trait LaunchIds {
    fn draw(&self) -> LaunchId;
}

/// How long the engine waits on a window, and how often a delivery is tried.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// For a delivered message's header to show in the window's record.
    pub arrival_ms: i64,
    /// For a launch's first message to show.
    pub launch_ms: i64,
    /// Attempts at a delivery before it fails.
    pub max_attempts: u32,
}

impl Default for Limits {
    /// The daemon's (`arrivalTimeoutMs`, `launchTimeoutMs`, `maxAttempts`).
    fn default() -> Self {
        Self {
            arrival_ms: 60_000,
            launch_ms: 180_000,
            max_attempts: 3,
        }
    }
}

/// The variable that sets how long a window may take to show its first
/// message, in milliseconds. A test that waits for a window that never does
/// would otherwise wait three minutes; nothing in a person's environment sets it.
pub const LAUNCH_MS_VARIABLE: &str = "CONSENSFLOW_LAUNCH_TIMEOUT_MS";

impl Limits {
    /// The daemon's limits, with the launch's wait what `env` says it is
    /// ([`LAUNCH_MS_VARIABLE`]) where it names a number of milliseconds above
    /// zero; any other value is no value.
    pub fn of(env: &Env) -> Self {
        let launch = env
            .text(LAUNCH_MS_VARIABLE)
            .and_then(|text| text.parse::<i64>().ok())
            .filter(|ms| *ms > 0);
        Self {
            launch_ms: launch.unwrap_or(Self::default().launch_ms),
            ..Self::default()
        }
    }
}

/// Everything the engine is made with: each seam shared, so a second
/// engine can be made with the same ones (a restart's).
#[derive(Clone)]
pub struct Seams {
    /// Shared with whoever else reads and writes the board (the API, the
    /// page): borrowed for one call at a time, never across a wait.
    pub ledger: Rc<RefCell<Ledger>>,
    pub host: Rc<dyn EngineHost>,
    pub adapters: Rc<dyn Adapters>,
    pub records: Rc<dyn Records>,
    pub time: Rc<dyn Time>,
    pub launch_ids: Rc<dyn LaunchIds>,
    pub credentials: Rc<dyn Credentials>,
    pub pane_env: Rc<dyn PaneEnv>,
    pub roster: Rc<dyn Roster>,
    pub roles: Rc<dyn Roles>,
    pub trace: Rc<dyn Trace>,
    pub log: Rc<dyn Log>,
    pub launch_files: Rc<dyn LaunchFiles>,
    pub spawn: Rc<dyn Spawn>,
    pub limits: Limits,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_launch_wait_is_three_minutes_unless_the_environment_names_a_number_of_milliseconds() {
        let wait =
            |value: &str| Limits::of(&Env::from_vars([(LAUNCH_MS_VARIABLE, value)])).launch_ms;
        assert_eq!(Limits::of(&Env::default()), Limits::default());
        assert_eq!(wait("1500"), 1_500);
        for refused in ["", "0", "-5", "soon", "1.5", "2s"] {
            assert_eq!(wait(refused), 180_000, "{refused:?}");
        }
        let named = Limits::of(&Env::from_vars([(LAUNCH_MS_VARIABLE, "1500")]));
        assert_eq!(
            (named.arrival_ms, named.max_attempts),
            (60_000, 3),
            "the rest is as it was"
        );
    }
}

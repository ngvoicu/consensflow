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
use cf_harness::contract::{Adapter, Agent, LaunchId, Pane, Records, Work};
use cf_harness::seams::Time;
use cf_ledger::{Ledger, LedgerError, ParticipantView, ProjectView};
use cf_proto::panes::Servers;
use cf_proto::specs::AppSpecs;
use cf_proto::trace::TraceLine;

use crate::host::EngineHost;
use crate::roles::ProjectSpecs;
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

    /// Why the ready-made agent `name` went from the catalog (its harness no
    /// longer offers its model), when it is one that did: a member or a chief
    /// still on it is told so, and to pick another agent rather than define
    /// it again. None for any other name, and where nothing says.
    fn retired(&self, _name: &str) -> Option<String> {
        None
    }

    /// What the human says of specs for every project, as the agents file keeps
    /// it (the app's setting a project may say otherwise of): whether they are
    /// on, and the folder and the variant a project uses when it names none.
    /// Asked at the launch of a chief whose project does not say specs are off.
    /// Off, and naming neither, where the file says nothing or cannot be read:
    /// the launch asked for a saved agent from the same file already.
    fn app_specs(&self) -> AppSpecs {
        AppSpecs::default()
    }
}

/// The text each role's window starts with (`roles`), or why it has none,
/// which fails its launch. A chief whose project has specs on is given them
/// ([`ProjectSpecs`]), and with them the section its text has of them; every
/// other window is given none.
pub trait Roles {
    fn instructions(
        &self,
        participant: &ParticipantView,
        project: &ProjectView,
        specs: Option<&ProjectSpecs<'_>>,
    ) -> Result<String, String>;
}

/// The environment a window's pane starts with before its harness's own
/// (`paneEnv`), in order.
pub trait PaneEnv {
    fn env(&self, participant: &ParticipantView, project: &ProjectView) -> Vec<(String, String)>;
}

/// What ConsensFlow's own macOS permissions are: whether it may control the
/// computer (Accessibility) and see the screen (Screen & System Audio
/// Recording), which a program in its panes is given as its own (a pane's
/// Claude Code is attributed to ConsensFlow, as a terminal's is to the terminal).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenPermissions {
    pub accessibility: bool,
    pub screen_recording: bool,
}

impl ScreenPermissions {
    /// Whether both are given.
    pub fn all(self) -> bool {
        self.accessibility && self.screen_recording
    }
}

/// What the machine says of a harness's computer use for the project's
/// windows: ConsensFlow's permissions, whether Claude Code's computer use is on
/// for a folder, and whether Codex's is set up (each read from the harness's own
/// configuration, never written).
pub trait ComputerUse {
    /// ConsensFlow's permissions; none on a system where no harness's computer
    /// use runs (Claude Code has none outside macOS).
    fn permissions(&self) -> Option<ScreenPermissions>;
    /// Whether Claude Code's computer use is on for a window opened in
    /// `directory`, and the folder it is kept under.
    fn claude(&self, directory: &str) -> cf_harness::claude::computer_use::ComputerUse;
    /// Whether Codex's computer use is set up on the machine (the ChatGPT app's
    /// plugin, for every folder; its grants are its own app's).
    fn codex(&self) -> cf_harness::codex::computer_use::ComputerUse;
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
    /// For a launch's first message to show. Every adapter is told it
    /// (`Launch::first_message_ms`): what its window waits for before that
    /// message can show ends by it.
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

/// The variable that sets how long a delivered message's header may take to show
/// in its window's record, in milliseconds. A test of the daemon's reading of a
/// record that comes late has to outlast it, so with the minute the daemon gives
/// it, each such case takes minutes; nothing in a person's environment sets it.
pub const ARRIVAL_MS_VARIABLE: &str = "CONSENSFLOW_ARRIVAL_TIMEOUT_MS";

impl Limits {
    /// The daemon's limits, with the launch's wait and the arrival's what `env`
    /// says they are ([`LAUNCH_MS_VARIABLE`], [`ARRIVAL_MS_VARIABLE`]) where it
    /// names a number of milliseconds above zero; any other value is no value.
    pub fn of(env: &Env) -> Self {
        let named = |variable: &str| {
            env.text(variable)
                .and_then(|text| text.parse::<i64>().ok())
                .filter(|ms| *ms > 0)
        };
        let default = Self::default();
        Self {
            arrival_ms: named(ARRIVAL_MS_VARIABLE).unwrap_or(default.arrival_ms),
            launch_ms: named(LAUNCH_MS_VARIABLE).unwrap_or(default.launch_ms),
            ..default
        }
    }

    /// What the environment cut, in the words the daemon's log says it in at its
    /// start, so that a variable left in a shell is seen there; none where it cut
    /// nothing, the daemon of a person's.
    pub fn cut_by_the_environment(&self) -> Option<String> {
        let default = Self::default();
        let cuts: Vec<String> = [
            (
                "arrival wait",
                self.arrival_ms,
                default.arrival_ms,
                ARRIVAL_MS_VARIABLE,
            ),
            (
                "launch wait",
                self.launch_ms,
                default.launch_ms,
                LAUNCH_MS_VARIABLE,
            ),
        ]
        .into_iter()
        .filter(|(_, now, was, _)| now != was)
        .map(|(what, now, was, variable)| format!("{what} {now} ms, not {was} ({variable})"))
        .collect();
        (!cuts.is_empty()).then(|| format!("limits set by the environment: {}", cuts.join(", ")))
    }
}

/// What a worker's window started that outlives its turn, which only the pane
/// host can see: it keeps each pane's processes, and the daemon tells it where
/// a turn begins, so that what a window's close and its program's exit say it left
/// running is the turns' work and not the harness's. Nothing is asked to end: a
/// turn's end ends nothing (`turn_end`), and a close ends the window's tree. It
/// says nothing it cannot know: a host that does not answer has no background work
/// to tell of.
pub trait BackgroundWork {
    /// A turn is about to begin in `pane`: whatever its tree runs now came
    /// before it, and is no turn's work. `servers` says how the window's harness
    /// keeps its own processes (`Adapter::servers`) and `harness` which harness it
    /// is: what the host can tell is its own, and what that starts, is no turn's
    /// work either, however late it began.
    fn mark<'a>(&'a self, pane: &'a Pane, servers: Servers, harness: &'a str) -> Work<'a, ()>;

    /// How many commands of its harness `pane`'s tree runs now, less what ran at the rest
    /// `since` and what began before `after` (`pane.commands`), where the host can tell
    /// them ([`CommandsSeen`]).
    fn commands<'a>(
        &'a self,
        pane: &'a Pane,
        since: Option<u64>,
        after: Option<i64>,
    ) -> Work<'a, CommandsSeen>;

    /// `pane` is at rest: the host keeps what its tree runs now, under the number answered,
    /// for a look at its commands to leave out (`pane.rest`). None where it keeps none.
    /// The rests `keep` names, which monitors that run are left, are kept however many
    /// come after them.
    fn rest<'a>(&'a self, pane: &'a Pane, keep: &'a [u64]) -> Work<'a, Option<u64>>;
}

/// What a look at the commands of a window's harness found (`pane.commands`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandsSeen {
    /// So many run in the window.
    Running(usize),
    /// The host answered that it cannot tell them: a platform with no groups, or a harness
    /// that keeps its servers otherwise.
    Untold,
    /// The host answered that a process of the window cannot be read (another user's):
    /// a later look may read them all.
    Unreadable,
    /// The host answered that the window's processes kept changing while it read them:
    /// a later look may read them whole.
    Unsettled,
    /// The host did not answer, or answered what is no answer.
    Unanswered,
}

/// Everything the engine is made with: each seam shared, so a second
/// engine can be made with the same ones (a restart's).
#[derive(Clone)]
pub struct Seams {
    /// Shared with whoever else reads and writes the board (the API, the
    /// page): borrowed for one call at a time, never across a wait.
    pub ledger: Rc<RefCell<Ledger>>,
    pub host: Rc<dyn EngineHost>,
    pub background: Rc<dyn BackgroundWork>,
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
    pub computer_use: Rc<dyn ComputerUse>,
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

    #[test]
    fn the_arrival_wait_is_a_minute_unless_the_environment_names_a_number_of_milliseconds() {
        // The variable's name is written out, so that a rename shows here.
        let wait = |value: &str| {
            Limits::of(&Env::from_vars([("CONSENSFLOW_ARRIVAL_TIMEOUT_MS", value)])).arrival_ms
        };
        assert_eq!(ARRIVAL_MS_VARIABLE, "CONSENSFLOW_ARRIVAL_TIMEOUT_MS");
        assert_eq!(wait("15000"), 15_000);
        for refused in ["", "0", "-5", "soon", "1.5", "2s"] {
            assert_eq!(wait(refused), 60_000, "{refused:?}");
        }
        let named = Limits::of(&Env::from_vars([
            (ARRIVAL_MS_VARIABLE, "15000"),
            (LAUNCH_MS_VARIABLE, "1500"),
        ]));
        assert_eq!(
            named,
            Limits {
                arrival_ms: 15_000,
                launch_ms: 1_500,
                max_attempts: 3,
            },
            "each variable sets its own wait and nothing else"
        );
    }

    #[test]
    fn a_limit_the_environment_cut_is_told_for_the_log_and_one_it_left_is_not() {
        let told = |vars: &[(&str, &str)]| {
            Limits::of(&Env::from_vars(vars.iter().copied())).cut_by_the_environment()
        };
        assert_eq!(told(&[]), None);
        // A value that is no number cuts nothing, and says nothing.
        assert_eq!(told(&[(ARRIVAL_MS_VARIABLE, "soon")]), None);
        assert_eq!(
            told(&[(ARRIVAL_MS_VARIABLE, "15000")]).as_deref(),
            Some("limits set by the environment: arrival wait 15000 ms, not 60000 (CONSENSFLOW_ARRIVAL_TIMEOUT_MS)")
        );
        assert_eq!(
            told(&[(LAUNCH_MS_VARIABLE, "8000")]).as_deref(),
            Some("limits set by the environment: launch wait 8000 ms, not 180000 (CONSENSFLOW_LAUNCH_TIMEOUT_MS)")
        );
        assert_eq!(
            told(&[(ARRIVAL_MS_VARIABLE, "15000"), (LAUNCH_MS_VARIABLE, "8000")]).as_deref(),
            Some("limits set by the environment: arrival wait 15000 ms, not 60000 (CONSENSFLOW_ARRIVAL_TIMEOUT_MS), launch wait 8000 ms, not 180000 (CONSENSFLOW_LAUNCH_TIMEOUT_MS)")
        );
    }
}

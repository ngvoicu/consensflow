//! One look at a running project: everything the invariants are held to at a
//! moment, read from outside. The ledger's rows ([`Ledger`]), the windows as the
//! pane host and the daemon's trace show them ([`Windows`]), and each harness's
//! own record of a window's conversation ([`Record`]). The run takes a look every
//! few seconds (`stage`); a test makes one by hand, from a ledger file made for it
//! and records written in a harness's shape, with no harness running.

use std::collections::BTreeMap;

use super::ledger::{Item, Ledger, Participant};
use super::record::Record;

/// A process, by its number and the command line it was started with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Process {
    pub pid: u32,
    pub command: String,
}

/// A window the board shows open, as the pane host answers for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenWindow {
    /// The participant whose window it is.
    pub handle: String,
    pub pane: String,
    pub generation: i64,
    /// The process the pane host opened it on, where the run saw its answer.
    pub pid: Option<u32>,
    /// Whether that process is there.
    pub running: bool,
    /// Whether the pane host answers for the window at all.
    pub hosted: bool,
    /// What its tree runs that a turn of it started (`pane.background`): the first
    /// few the host names.
    pub background: Vec<Process>,
    /// How many its tree runs in all, which may be more than the host names: the
    /// watch follows only those it was named.
    pub background_total: usize,
    /// Why the pane host could not say what its tree runs, where it could not: what
    /// it runs is then not known to the watch, nor followed.
    pub tree_unread: Option<String>,
    /// The last lines of its screen, as the pane host draws it.
    pub tail: Vec<String>,
}

/// Who closed a window that the pane host said had exited.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClosedBy {
    /// The daemon asked the pane host to (`pane.kill`): a result taken, a cancel,
    /// a pause, a quit.
    Daemon,
    /// The run did, as a disruption.
    Run,
    /// Nobody asked: the program ended, or crashed.
    Itself,
}

/// A window that exited.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exit {
    pub handle: String,
    pub pane: String,
    pub generation: i64,
    /// When the host told it, in milliseconds.
    pub at: i64,
    pub code: Option<i64>,
    pub signal: Option<String>,
    pub closed_by: ClosedBy,
    /// The last lines its screen showed.
    pub tail: Vec<String>,
    /// What its close ended of what its turns left running, as the host listed it.
    pub ended: Vec<Process>,
}

/// A process of the run that no window has any more: what a window's tree ran,
/// still there a while after the window ended, the same process it was (the system
/// says it began when it said it did at the look that saw it). The run did not
/// start it, so it is said with what is known of it and left running: the run ends
/// nothing but what it started itself, through the handle it holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stranger {
    pub process: Process,
    /// The window it was seen in.
    pub pane: String,
    /// When that window ended, in milliseconds.
    pub since: i64,
    /// When the system says it began: what it is known by.
    pub began: String,
}

/// A process of the run the watch cannot judge: whether it still runs, or whether
/// the process under its number is the one it was, the system could not say. It is
/// left running, and said so.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unresolved {
    pub process: Process,
    pub pane: String,
    /// When its window ended, in milliseconds.
    pub since: i64,
    pub why: String,
}

/// A close of the app whose pane host's last word was not read to its end: its
/// output had not ended within the time the rig gives it, so an exit it told after,
/// and what that exit says its close ended, is none the watch has.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unread {
    /// When the app closed, in milliseconds.
    pub at: i64,
    pub why: String,
}

/// A change of a window's state, as the daemon's trace says it (`window.activity`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Activity {
    pub at: i64,
    pub handle: String,
    /// `working`, `idle`, `waiting`…
    pub state: String,
    pub reason: Option<String>,
}

/// The windows of the run as the look found them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Windows {
    pub open: Vec<OpenWindow>,
    /// Every window that exited since the app last started.
    pub exits: Vec<Exit>,
    pub strangers: Vec<Stranger>,
    pub unresolved: Vec<Unresolved>,
    /// Every close of the app whose host's last word was not read to its end.
    pub unread: Vec<Unread>,
    /// Every change of a window's state in the trace, oldest first.
    pub activity: Vec<Activity>,
}

/// One look at the project.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Look {
    /// When it was taken, in milliseconds.
    pub at: i64,
    pub ledger: Ledger,
    pub windows: Windows,
    /// Each harness's own record of a conversation, by the conversation's row,
    /// where the run could read one.
    pub records: BTreeMap<i64, Record>,
    /// Each conversation whose own record the run reads and could not read at this
    /// look, by its row, with why.
    pub records_unread: BTreeMap<i64, String>,
}

impl Look {
    /// The record of the participant `id`'s newest conversation that the look
    /// has, if it has one.
    pub fn record_of(&self, participant: i64) -> Option<&Record> {
        self.ledger
            .conversations_of(participant)
            .iter()
            .rev()
            .find_map(|conversation| self.records.get(&conversation.id))
    }

    /// The open window of the participant `handle`, if the board shows one.
    pub fn window_of(&self, handle: &str) -> Option<&OpenWindow> {
        self.windows
            .open
            .iter()
            .find(|window| window.handle == handle)
    }

    /// The last change of state the trace says of `handle`'s window by `at`.
    pub fn activity_of(&self, handle: &str, at: i64) -> Option<&Activity> {
        self.windows
            .activity
            .iter()
            .rev()
            .find(|change| change.handle == handle && change.at <= at)
    }

    /// Whether `who` is a harness Claude Code, whose record the watch reads whole.
    pub fn is_claude(who: &Participant) -> bool {
        who.harness.as_deref() == Some("claude-code")
    }

    /// The items of the copy of `participant`'s conversations said by `at`.
    pub fn items_by(&self, participant: i64, at: i64) -> Vec<&Item> {
        self.ledger
            .items_of(participant)
            .into_iter()
            .filter(|item| item.millis().is_none_or(|said| said <= at))
            .collect()
    }
}

//! What a window's program started and left running. An agent can start work
//! that outlives its turn (a job in the background, a monitor it armed) and
//! end the turn. The turn's end is the task's result all the same (the owner's
//! decision of 2026-10-13), and the daemon asks for what the turn left to be
//! ended once it has taken the result (`pane.end_background`), the window
//! staying open; closing the window ends all of it, on Unix once it was asked to
//! end and given time to release what it holds, on Windows at once with no chance
//! to (`pty::close` says why).
//!
//! The host looks at the window's **tree**: every process the window's program
//! started, wherever it went ([`Member`]). On macOS that is its children and what
//! descends from them, what is in the window's session (a job the agent detached
//! with `nohup … &` from a shell that then ended is left to the system's init,
//! and keeps the session it was started in), what the host saw the window's
//! program and its children start as they started it (a harness that gives each
//! command a shell in a session of its own leaves the job in that one, which the
//! window's does not hold), and whatever carries the pane's marker in its
//! environment (a process that left its session). On Windows it is
//! the pane's job object: a process joins it when it is started and leaves it only
//! by breaking away (`CREATE_BREAKAWAY_FROM_JOB`), which the job does not allow, so
//! a job whose shell has gone is in it still.
//!
//! The host cannot tell a program from the work it started by anything the
//! program is: an MCP server and a recording run are both children of the
//! harness. The daemon says a turn is about to begin (`pane.mark`), and the host
//! keeps the processes its tree has then: the harness, its servers, its language
//! servers. What the tree has later that is not among them began after the turn
//! did ([`Baseline::turn_work`] looks only at that). A process is known by its
//! number and by when it began, so a process that took the number of one that ended
//! is not mistaken for it; everything the host does to a process it does to the one
//! it chose, by that, never to the number a later look finds.
//!
//! Of what began after the mark the host calls only what it can prove the turn's
//! the turn's ([`work`] says how): a shell, what it started, and what has no parent
//! left. A server the harness starts late is none of these, and it is not told
//! apart from a job by when it began: the host leaves it alone, and says nothing of
//! it. And nothing the host can tell the harness's own is ever the turn's, by where
//! the harness keeps its servers or by the programs it is named for ([`harness`]
//! says how). So what is listed, ended and named is the turn's work and nothing of
//! the rest, and a process that cannot be told is left as it is.
//!
//! Only looking at the tree is the platform's: the macOS process tree and the
//! Windows job object each list their members, with the program each runs and the
//! path it runs from, the group each is in (macOS; a job object has none) and the
//! process that started it where the platform says (macOS, and Windows by the
//! parent each process names, which is the process that started it only while that
//! one is there); the rest is here.

mod harness;
mod work;

use std::collections::HashSet;

use cf_proto::panes::{Background, BackgroundProcess, Ending, BACKGROUND_PROCESSES, COMMAND_CHARS};

pub(crate) use harness::Kept;

/// A process by what tells it from a later one of the same number: the number,
/// and when it began, in whatever unit its platform keeps that in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct ProcessKey {
    pub pid: u32,
    pub began: u64,
}

/// Which process of the tree started a member, as far as the platform says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Parent {
    /// The platform did not say. Nothing is told of the member by it.
    Unknown,
    /// It names a process that is no member of the tree: the process that started
    /// it has gone (the system keeps naming a parent that went, which is no member
    /// either), or it is the window's program, which the tree starts at.
    Gone,
    /// The process of the tree that started it.
    Of(ProcessKey),
}

impl Parent {
    /// The process that started it, where it is one of the tree.
    pub fn of(self) -> Option<ProcessKey> {
        match self {
            Self::Of(parent) => Some(parent),
            Self::Unknown | Self::Gone => None,
        }
    }
}

/// A process of a pane's tree: the command line it was started with, and what the
/// platform could read of the rest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Member {
    pub key: ProcessKey,
    /// The command line, as far as the platform let it be read; the program alone
    /// where it could read no more.
    pub command: String,
    /// The name of the program it runs, as the system keeps it (`zsh`, `bash.exe`):
    /// empty where it could not be read.
    pub program: String,
    /// The whole path the program runs from, as the system keeps it, with no word of
    /// a command line in it: empty where it could not be read.
    pub exe: String,
    /// The process group it is in; none where the platform says none (a Windows
    /// job object has no groups).
    pub group: Option<i32>,
    pub parent: Parent,
}

/// The processes of a pane's tree when its turn began; none until the daemon
/// says one does, and a pane with none has no background work, since nothing
/// says what came before.
#[derive(Debug, Default, Clone)]
pub(crate) struct Baseline {
    /// The processes the tree had when the latest turn began.
    before: Option<HashSet<ProcessKey>>,
    /// The processes it had when the first turn began, and has for good: what the
    /// window opened with.
    launch: HashSet<ProcessKey>,
    /// How the window's harness keeps its servers, as the daemon said at the
    /// latest mark: none where it did not say, or the platform has no groups.
    kept: Option<Kept>,
    /// Which harness it is, as the daemon said at the latest mark: the name its own
    /// programs are told by.
    harness: Option<String>,
}

impl Baseline {
    /// A turn begins: every process of the tree now came before it, `kept` says how
    /// the window's harness keeps its servers ([`Kept::of`]) and `harness` which
    /// harness it is.
    pub fn mark(&mut self, tree: &[Member], kept: Option<Kept>, harness: Option<String>) {
        let now: HashSet<ProcessKey> = tree.iter().map(|member| member.key).collect();
        if self.before.is_none() {
            self.launch.clone_from(&now);
        }
        self.before = Some(now);
        self.kept = kept;
        self.harness = harness;
    }

    /// Whether a turn was ever said to begin: where it was not, the tree is
    /// not worth looking at.
    pub fn is_marked(&self) -> bool {
        self.before.is_some()
    }

    /// What `tree`, the pane's tree now, has that its turn started, as the daemon is
    /// told of it.
    pub fn left_running(&self, tree: &[Member]) -> Background {
        listed(self.turn_work(tree))
    }

    /// The members of `tree` that are the turn's work, all of them, oldest first:
    /// what began after the mark and is proved the turn's ([`work`]), less what is
    /// proved the harness's ([`harness`]). Whatever `tree` holds is looked at, and
    /// what the daemon is told of it is for [`listed`] to bound.
    pub fn turn_work(&self, tree: &[Member]) -> Vec<Member> {
        let Some(before) = &self.before else {
            return Vec::new();
        };
        let started: Vec<&Member> = tree
            .iter()
            .filter(|member| !before.contains(&member.key))
            .collect();
        if started.is_empty() {
            return Vec::new();
        }
        let mut protected = self
            .kept
            .map(|kept| harness::own(kept, &self.launch, tree))
            .unwrap_or_default();
        if let Some(harness) = &self.harness {
            protected.extend(harness::programs(harness, tree));
        }
        let turns = work::turns(&started, &protected);
        let mut work: Vec<Member> = started
            .into_iter()
            .filter(|member| turns.contains(&member.key))
            .cloned()
            .collect();
        work.sort_by_key(|member| (member.key.began, member.key.pid));
        work
    }
}

/// What an ending came to, once it was made: the processes it chose that are not
/// among `alive`, the processes of the tree now, were ended; those that are were
/// not, and are said so.
pub(crate) fn report(chosen: Vec<Member>, alive: &HashSet<ProcessKey>) -> Ending {
    let (left, ended): (Vec<Member>, Vec<Member>) = chosen
        .into_iter()
        .partition(|member| alive.contains(&member.key));
    Ending {
        ended: listed(ended),
        left: listed(left),
    }
}

/// `chosen`, the first of it as the daemon is told of it: how many, and the first
/// [`BACKGROUND_PROCESSES`] by when they began.
pub(crate) fn listed(mut chosen: Vec<Member>) -> Background {
    chosen.sort_by_key(|member| (member.key.began, member.key.pid));
    Background {
        total: chosen.len(),
        processes: chosen
            .iter()
            .take(BACKGROUND_PROCESSES)
            .map(|member| BackgroundProcess {
                pid: member.key.pid,
                command: bounded(&member.command),
            })
            .collect(),
    }
}

/// A command line held to what a frame carries: its blanks made one, cut at
/// [`COMMAND_CHARS`] characters with a mark that says it was.
fn bounded(command: &str) -> String {
    let words = command.split_whitespace().collect::<Vec<_>>().join(" ");
    if words.chars().count() <= COMMAND_CHARS {
        return words;
    }
    let kept: String = words.chars().take(COMMAND_CHARS - 1).collect();
    format!("{kept}…")
}

#[cfg(test)]
mod tests;

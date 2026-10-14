//! What a window's program started and left running. An agent can start work
//! that outlives its turn (a job in the background, a monitor it armed) and
//! end the turn. The turn's end is the task's result all the same (the owner's
//! decision of 2026-10-13), and the daemon asks for what the turn left to be
//! ended once it has taken the result (`pane.end_background`), the window
//! staying open; closing the window ends the rest of what is the window's, on Unix
//! once it was asked to end and given time to release what it holds, on Windows at
//! once with no chance to (`pty::close` says why).
//!
//! The host looks at the window's **tree**: every process the window's program
//! started that the platform still names ([`Member`]). On macOS that is its children
//! and what descends from them, what is in the window's session and whatever carries
//! the pane's marker in its environment; a close ends of these only what is under the
//! program by the process that started each, or is in the session the program leads
//! while it runs, or was seen so at a look, whatever the selection says (`process_tree`'s
//! bound), and leaves the rest running. On Windows
//! it is the pane's job object: a process joins it when it is started and leaves it
//! only by breaking away (`CREATE_BREAKAWAY_FROM_JOB`), which the job does not allow,
//! and it is the job that is ended, and nothing outside it can be.
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
//! **When in doubt the host ends less.** Of what began after the mark it calls the
//! turn's only what a chain of processes that run proves it: a shell the harness
//! started, and what descends from it ([`work`]). A process whose chain is broken (a
//! job an agent detached with `nohup … &` or `Start-Process` from a shell that then
//! ended) has no chain to look at, and its being a shell, or having no parent, is no
//! proof of whose it is: it is left running, not ended and not named, until its window
//! closes. Nor is anything the host can tell the harness's own ever the turn's, by
//! where the harness keeps its servers or by the programs it is named for
//! ([`harness`] says how); and what it has seen to be the harness's it keeps for its
//! own as long as it runs ([`Baseline::turn_work`] remembers it), whatever becomes of
//! the process that started it. So what is listed, ended and named is the turn's work
//! and nothing of the rest, and a process that cannot be told is left as it is.
//!
//! An ending is bound to the turn it ends. What it ends is chosen once, when it
//! begins, and is those processes and what they start while they end
//! (`process_tree::Lineage`; `job_object`'s `end`): never what a later look of the
//! whole tree takes for the turn's, which a new turn's work is, started in the window
//! by a human or by a message that went in meanwhile.
//!
//! Only looking at the tree is the platform's: the macOS process tree and the
//! Windows job object each list their members, with the program each runs and the
//! path it runs from, the group each is in (macOS; a job object has none) and the
//! process that started it where the platform says (macOS, and Windows by the
//! parent each process names, which is the process that started it only while that
//! one is there); the rest is here.

mod harness;
mod work;

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, PoisonError};

use cf_proto::panes::{Background, BackgroundProcess, Ending, BACKGROUND_PROCESSES, COMMAND_CHARS};

pub(crate) use harness::Kept;

/// A process by what tells it from a later one of the same number: the number,
/// and when it began, in whatever unit its platform keeps that in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct ProcessKey {
    pub pid: u32,
    pub began: u64,
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
    /// The process of the tree that started it, where there is one: none where the
    /// platform does not say which started it, and none where that process is no
    /// member (it has gone, or it is the window's program, which the tree starts at).
    pub parent: Option<ProcessKey>,
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
    /// What was seen to descend from a process of the harness's own at the latest
    /// look, and runs still: kept for the day the process that started it has gone. It
    /// is shared by the copies a look is made with: whatever one sees, the next knows.
    own: Arc<Mutex<HashSet<ProcessKey>>>,
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
        // What the harness has now is its own, whatever becomes of what started it.
        self.protected(tree);
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
    /// proved the harness's. Whatever `tree` holds is looked at, and what the daemon
    /// is told of it is for [`listed`] to bound.
    pub fn turn_work(&self, tree: &[Member]) -> Vec<Member> {
        let Some(before) = &self.before else {
            return Vec::new();
        };
        let protected = self.protected(tree);
        let started: Vec<&Member> = tree
            .iter()
            .filter(|member| !before.contains(&member.key))
            .collect();
        let turns = work::turns(&started, &protected);
        let mut work: Vec<Member> = started
            .into_iter()
            .filter(|member| turns.contains(&member.key))
            .cloned()
            .collect();
        work.sort_by_key(|member| (member.key.began, member.key.pid));
        work
    }

    /// The members of `tree` that are the harness's own and what descends from them:
    /// what [`harness`] proves of them now (where it keeps its servers, the programs
    /// it is named for), and what a look before saw descend from one and runs still,
    /// with what descends from that, though the process that started it may have gone
    /// since. What descends from a process of the harness's is kept for the next look,
    /// and what ended is let go of. The processes that are the harness's by where they
    /// are or by what they are named for are not kept: the next look tells them again,
    /// and one may leave what told it (a child begins in its parent's group, and moves
    /// out of it a moment after).
    fn protected(&self, tree: &[Member]) -> HashSet<ProcessKey> {
        let mut protected = self
            .kept
            .map(|kept| harness::own(kept, &self.launch, tree))
            .unwrap_or_default();
        if let Some(harness) = &self.harness {
            protected.extend(harness::programs(harness, tree));
        }
        let mut own = self.own.lock().unwrap_or_else(PoisonError::into_inner);
        protected.extend(descending_from(&own, tree));
        let kept: HashSet<ProcessKey> = tree
            .iter()
            .filter(|member| {
                protected.contains(&member.key)
                    && (own.contains(&member.key)
                        || member
                            .parent
                            .is_some_and(|parent| protected.contains(&parent)))
            })
            .map(|member| member.key)
            .collect();
        *own = kept;
        protected
    }
}

/// What `tree` has that is in `roots` or descends from a process in it, by the
/// parent each was started by. Nothing where `roots` has none.
pub(crate) fn descending_from(roots: &HashSet<ProcessKey>, tree: &[Member]) -> HashSet<ProcessKey> {
    if roots.is_empty() {
        return HashSet::new();
    }
    let by_key: HashMap<ProcessKey, &Member> =
        tree.iter().map(|member| (member.key, member)).collect();
    tree.iter()
        .filter(|member| {
            let mut at = *member;
            // A tree names no process twice among a process's ancestors: the bound is
            // only for a list that is no tree.
            for _ in 0..=by_key.len() {
                if roots.contains(&at.key) {
                    return true;
                }
                match at.parent.and_then(|parent| by_key.get(&parent)) {
                    Some(parent) => at = parent,
                    None => return false,
                }
            }
            false
        })
        .map(|member| member.key)
        .collect()
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

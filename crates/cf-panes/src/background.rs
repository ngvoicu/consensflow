//! What a window's program started and left running. An agent can start work
//! that outlives its turn (a job in the background, a monitor it armed) and
//! end the turn. A turn ends nothing it left running (turn-end, the chief's decision of
//! 2026-10-21): the daemon asks what it left (`pane.background`, and which of the
//! harness's commands run, `pane.commands`), and closing the window ends what is the
//! window's, on Unix once it was asked to end and given time to release what it holds,
//! on Windows at once with no chance to (`pty::close` says why).
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
//! and it is the job that a close ends, and nothing outside it can be.
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
//! **When in doubt the host names less.** Of what began after the mark it calls the
//! turn's only what a chain of processes that run proves it: a shell the harness
//! started, and what descends from it ([`work`]). A process whose chain is broken (a
//! job an agent detached with `nohup … &` or `Start-Process` from a shell that then
//! ended) has no chain to look at, and its being a shell, or having no parent, is no
//! proof of whose it is: it is left running, not ended and not named, until its window
//! closes, which ends it where the tree finds it the window's (always on Windows; on
//! the Mac not a command left by a shell that leads a session of its own, Pi's, which
//! runs on after the close too). Nor is anything the host can tell the harness's own
//! ever the turn's, by where the harness keeps its servers or by the programs it is
//! named for ([`harness`] says how); and what it has seen to be the harness's it keeps
//! for its own as long as it runs ([`Baseline::turn_work`] remembers it), whatever
//! becomes of the process that started it. So what is listed and named is the turn's
//! work and nothing of the rest, and a process that cannot be told is not named.
//!
//! **A moment later.** A harness runs short commands of its own at any time, and the
//! host cannot tell one from the agent's by anything it is. Claude Code runs its status
//! line (the user's script: a `bash` that runs `git … branch --show-current` and ends a
//! moment later) in a group of its own, as it runs the commands its agent asks for, so a
//! look that comes while one runs finds a shell the harness started after the mark with
//! a job under it, which is all the chain asks of the turn's work (seen in 3.0.3: a
//! close named two runs of one, and the `git`, as work the window left running). So
//! whatever the host names, at a window's close and at the exit of its program, it looks
//! at again [`SETTLE`] after it first found it, and names only what both looks find: a
//! process that is still there ([`steady`]), by its
//! number and when it began. A run of a status line that ended meanwhile is none of it,
//! nor is the next run, which is another process. Where the first look finds nothing
//! there is nothing to wait for, and no second look is made. The look the daemon asks
//! for while a turn runs (`pane.background`) is one look, and says what it finds then;
//! the closes of a quit make none, with no one left to tell. What lasts longer than the
//! moment and is in no group of the window's is named all the same, as the agent's.
//!
//! Only looking at the tree is the platform's: the macOS process tree and the
//! Windows job object each list their members, with the program each runs and the
//! path it runs from, the group each is in (macOS; a job object has none) and the
//! process that started it where the platform says (macOS, and Windows by the
//! parent each process names, which is the process that started it only while that
//! one is there); the rest is here.

#[cfg(any(target_os = "macos", test))]
mod clock;
mod harness;
mod work;

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use cf_proto::panes::{
    Background, BackgroundProcess, Servers, BACKGROUND_PROCESSES, COMMAND_CHARS,
};

#[cfg(test)]
pub(crate) use clock::Reading;
#[cfg(any(target_os = "macos", test))]
pub(crate) use clock::{began_before, PaneClock};
#[cfg(target_os = "macos")]
pub(crate) use clock::{Read, Watch};
pub(crate) use harness::Kept;

/// How many of a window's rests are kept ([`Baseline::rest`]): a rest asked of in a look is
/// the latest a few turns before it, and one no longer kept leaves nothing out.
const RESTS: usize = 16;

/// How long the host waits before it looks again at what it found the turn's work, where
/// it names what a window left running (see above): longer than a run of a harness's
/// status line takes, and short enough for a close, and the end of a turn, to wait for.
pub(crate) const SETTLE: Duration = Duration::from_secs(2);

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
    /// The window's processes at the latest of its rests the daemon asked to be kept
    /// ([`Baseline::rest`]), each by the number the host gave it, oldest first: a few.
    rests: VecDeque<(u64, HashSet<ProcessKey>)>,
    /// The number the next rest is given.
    next_rest: u64,
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

    /// Whether the host can tell the commands of the window's harness, as the daemon said at
    /// a mark how it keeps its servers: in the window's group, on a platform with groups,
    /// by their groups; in no group, its commands each in a shell of its own
    /// ([`Servers::ByShell`]), by their chains ([`Baseline::commands`]). Asked before the
    /// tree is walked (Calliope on 472b1d96).
    pub fn tells_commands(&self) -> bool {
        self.commands_kept().is_some()
    }

    fn commands_kept(&self) -> Option<Kept> {
        // A window in the window's group where there are no groups claims what cannot be.
        self.kept
            .filter(|kept| kept.servers == Servers::InGroup)
            .filter(|kept| kept.group.is_some())
            .or(self.kept.filter(|kept| kept.servers == Servers::ByShell))
    }

    /// The window is at rest, its tree `tree`: kept, under the number answered, for a
    /// look at its commands to leave out ([`Baseline::commands`]). The last [`RESTS`] are
    /// kept, and those `keep` names however many came after them: the daemon names those a
    /// monitor that runs is left (Astraeus on 136a7158). None where the host cannot tell
    /// the commands.
    pub fn rest(&mut self, tree: &[Member], keep: &[u64]) -> Option<u64> {
        self.commands_kept()?;
        let number = self.next_rest;
        self.next_rest += 1;
        self.rests
            .push_back((number, tree.iter().map(|member| member.key).collect()));
        let latest = self.rests.len().saturating_sub(RESTS);
        let mut at = 0;
        self.rests.retain(|(number, _)| {
            at += 1;
            at > latest || keep.contains(number)
        });
        Some(number)
    }

    /// The commands of the window's harness that run in `tree` now ([`harness::commands`]),
    /// less what ran at the rest `since` where it is one kept, and what `before` proves began
    /// before the monitors were called (the pane's clock: [`clock`]): none where the host
    /// cannot tell them (`cf_proto::panes::Commands`). A rest no longer kept, or never kept,
    /// leaves nothing out.
    ///
    /// **Where the platform has no groups** (a Windows job object, T-906) no process is a
    /// command by its group: a harness that runs each command in a shell of its own
    /// ([`Servers::ByShell`]) has them told by their chain instead ([`Baseline::chained`]).
    pub fn commands(
        &self,
        tree: &[Member],
        since: Option<u64>,
        before: &dyn Fn(&Member) -> bool,
    ) -> Option<Background> {
        let kept = self.commands_kept()?;
        let Some(group) = kept.group.filter(|_| kept.servers == Servers::InGroup) else {
            return Some(listed(self.chained(tree, since)));
        };
        let empty = HashSet::new();
        let rest = since
            .and_then(|since| self.rests.iter().find(|(number, _)| *number == since))
            .map_or(&empty, |(_, rest)| rest);
        Some(listed(harness::commands(group, tree, rest, before)))
    }

    /// The commands of a window whose harness runs each in a shell of its own, on a platform
    /// with no groups (a Windows job object, [`Servers::ByShell`]): what
    /// began after the rest `since`, where it is one kept, else after the window's launch,
    /// that is the work a turn's is proved by ([`work`]: a shell the harness started, and a
    /// chain of processes that run down from it while it runs); never what is the harness's
    /// own ([`Baseline::protected`]). Claude Code runs every command so (Git Bash's
    /// `bash.exe` for a Bash call's, a Monitor's and a background job's), and its servers as
    /// the programs they are (`node`, `uvx`, or behind a `cmd.exe /c`, which is no shell
    /// here): the processes it started for its own are none of them, whenever they began. No
    /// start is read against a monitor's call: where no rest proves them, the window's
    /// launch, before any command of it, is what is left out, and nothing later is.
    ///
    /// **The shell is the command** (T-906's live `tell` on zeewin2). Claude Code 2.1.312
    /// ends a command on Windows by its shell's number alone: its tree kill asks `ps` for the
    /// children and signals the group (`process.kill(-pid)`), neither of which native
    /// Windows has, and falls back to the shell. A Monitor's `exec sleep 611` in Git Bash is
    /// a `sleep.exe` of its own, under the shell, which runs on with no parent once the shell
    /// is ended; so does what a Bash call the human interrupted was running. What runs on so
    /// is left running, as a job a command detached is ([`work`]), and the window's close
    /// ends it; it is no command, or a monitor whose notice was Claude Code's would be held
    /// for as long as it runs. A command that printed such a notice runs still, its shell
    /// with it (Git Bash keeps the shell while what it `exec`s runs).
    ///
    /// Limits: a server the user wraps in `bash` or `powershell` and the harness starts after
    /// the rest, and a shell a server started for its own while it runs, are counted, which
    /// holds a monitor until a later look or Finish; a command that printed a notice and then
    /// ended its shell, leaving what it started to run on, is not.
    fn chained(&self, tree: &[Member], since: Option<u64>) -> Vec<Member> {
        let rest = since.and_then(|number| self.rests.iter().find(|(kept, _)| *kept == number));
        let left_out = rest.map_or(&self.launch, |(_, rest)| rest);
        let protected = self.protected(tree);
        let started: Vec<&Member> = tree
            .iter()
            .filter(|member| !left_out.contains(&member.key))
            .collect();
        let shells = work::turns(&started, &protected);
        started
            .into_iter()
            .filter(|member| shells.contains(&member.key))
            .cloned()
            .collect()
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

/// The members of `later`, the turn's work as a look [`SETTLE`] after another found it,
/// that `earlier`, the turn's work as the other found it, has too: what is still there,
/// each process told by its number and when it began, so that a process that took the
/// number of one that ended is not mistaken for it. What `earlier` has that has ended
/// since is none of it, and so is what `later` has that began since: a window's own
/// short run is another process at every look.
pub(crate) fn steady(earlier: &[Member], later: Vec<Member>) -> Vec<Member> {
    let named: HashSet<ProcessKey> = earlier.iter().map(|member| member.key).collect();
    later
        .into_iter()
        .filter(|member| named.contains(&member.key))
        .collect()
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
        possibly_left: Vec::new(),
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

//! Which of the processes of a window's tree are its harness's own, and so never
//! the turn's work, whatever shell started them or however alone they are. The host
//! cannot tell them from the agent's work by anything the processes are, and by
//! when they began ([`super::Baseline`]) it is wrong for a server that begins late
//! and for what a server starts. It goes by what it can prove of each process, in
//! two ways, and a process the second way cannot prove the harness's it does not
//! take for the turn's either ([`super::work`]).
//!
//! **Where it keeps its servers** (`pane.mark`'s `servers`), in one of two shapes
//! ([`Servers`]), both going by ancestry: [`Member::parent`] says which process of
//! the tree started each. The window's program leads the window's group. It is the
//! harness, or the supervisor that opens it, and the agent's commands are started by
//! the harness's own process: the window's program (Claude Code, Pi) or one beside
//! it in the window's group (Codex's own process, under the supervisor). So a walk
//! up from a process, through the parents that are processes of the tree, stops at
//! the window's program: it is in the window's group, and it would make everything
//! of the window the harness's. The process is the harness's if it, or a process the
//! walk meets, is a server:
//!
//! - [`Servers::InGroup`]: a server is a process in the window's group. What a
//!   server starts is the harness's whatever group it starts it in, which a
//!   browser a server launches is: a tool's command is not, for its shell is in a
//!   group of its own and its parent is the window's program.
//! - [`Servers::AtLaunch`]: a server is a process the window had when its first
//!   turn began and that is outside the window's group. The harness's own process
//!   is there from the launch too, but in the window's group: what it starts is
//!   not a server's. A process that began after the first turn is no server of the
//!   launch, whatever the mark after it took for the turn's past.
//!
//! **What it is named for** (`pane.mark`'s `harness`): a program named for the
//! harness (`codex-code-mode-host`, which Codex starts the first time its agent runs
//! a command and keeps for the session) that runs from where the harness's own
//! binary does is the harness's, and so is whatever descends from it
//! ([`programs`]). It goes by the program's path, which the system keeps whole
//! (spaces in it are no matter), not by a word of a command line; and a program that
//! is named for the harness and is not beside its binary is anybody's.
//!
//! A process whose parent has gone has no ancestry to look at: it is the harness's
//! only where it is in the window's group (the first shape), is a program of its own
//! (the second), or was seen the harness's before, while its parent lived
//! ([`super::Baseline`] keeps what it has seen). It is never the turn's for being left
//! without one ([`super::work`]): a browser that outlives the server that started it
//! and left the group is left alone, as it is not proved the agent's.

use std::collections::{HashMap, HashSet};

use cf_proto::panes::Servers;

use super::{descending_from, Member, ProcessKey};

/// What the daemon said of the window's harness at a mark, with the process
/// group of the window's program, which leads it: the window's group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Kept {
    /// The window's group.
    pub group: i32,
    /// How its harness keeps its servers, which is known: a window whose harness
    /// is not known to keep them in a way has no `Kept`.
    pub servers: Servers,
}

impl Kept {
    /// What `servers` says of a window whose program leads `group`: none where it
    /// says it is not known, and where the platform has no groups.
    pub fn of(servers: Servers, group: Option<i32>) -> Option<Self> {
        let group = group?;
        (!servers.is_unknown()).then_some(Self { group, servers })
    }
}

/// The processes of `tree` that are the harness's own servers, and what they
/// started. `launch` is the processes the window had when its first turn began. The
/// window's program is none of them, though it is in its own group: what it starts is
/// the agent's, its commands among it.
pub(super) fn own(
    kept: Kept,
    launch: &HashSet<ProcessKey>,
    tree: &[Member],
) -> HashSet<ProcessKey> {
    let by_key: HashMap<ProcessKey, &Member> =
        tree.iter().map(|member| (member.key, member)).collect();
    tree.iter()
        .filter(|member| {
            !leads(kept, member) && descends_from_a_server(kept, launch, &by_key, member)
        })
        .map(|member| member.key)
        .collect()
}

/// Whether `member` is a server, or a process one started, by the walk up from it.
fn descends_from_a_server(
    kept: Kept,
    launch: &HashSet<ProcessKey>,
    by_key: &HashMap<ProcessKey, &Member>,
    member: &Member,
) -> bool {
    let mut at = member;
    // A tree names no process twice among a process's ancestors: the bound is
    // only for a list that is no tree.
    for _ in 0..=by_key.len() {
        if is_server(kept, launch, at) {
            return true;
        }
        match at.parent.and_then(|parent| by_key.get(&parent)) {
            Some(parent) if !leads(kept, parent) => at = parent,
            _ => return false,
        }
    }
    false
}

/// The commands of a window whose harness keeps its servers in the window's group
/// ([`Servers::InGroup`], Claude Code): the processes of `tree` the window's program
/// started that lead a process group of their own, and what is outside the window's
/// group and was left with no parent in the tree that leads one. Claude Code runs every
/// command so (a Bash call's shell, a Monitor's, a background job's, `/bin/zsh -c … eval
/// '…'`, as a session of its own), and what a command `exec`s takes its place in its
/// group; it runs its hooks and its status line so too, for the moment each runs. Its MCP
/// servers and its own helpers (`caffeinate`) it starts in the window's group, and what a
/// command starts stays in the command's group (T-203's probes of 2.1.307 in a pane's
/// shape: a Monitor of `exec sleep 300`, one of a loop, a background `sleep 50`, a Python
/// MCP server, a `*` PostToolUse hook, a Stop hook and a status line). A process whose
/// group could not be read is not known to be outside the window's, nor in it: one the
/// program started is counted.
///
/// What ran when the window was last at rest before the monitors asked of began
/// (`rest`, [`super::Baseline::rest`]) is none of them: a server or a helper that left
/// the window's group (`setsid`) and runs for good is no command (Astraeus on 472b1d96),
/// and no command a monitor asked of runs began before it. Hooks and the status line
/// begin and end in a moment, and need none of it.
///
/// Limits. A process another left when it ended, which keeps the group of the process
/// that started it and does not lead one (a job a command's shell started and left), is
/// none of them: the command it was started by has ended, which ended its monitor by its
/// status. And a server or helper that left the window's group before the window's first
/// rest, which is before its first message (Claude Code starts its servers as it takes
/// that), is counted for as long as it runs: a monitor armed in the window's first turn
/// waits for it, the safe way.
pub(super) fn commands(kept: Kept, tree: &[Member], rest: &HashSet<ProcessKey>) -> Vec<Member> {
    tree.iter()
        .filter(|member| !rest.contains(&member.key))
        .filter(|member| member.group != Some(kept.group))
        .filter(|member| match member.parent {
            Some(parent) => i32::try_from(parent.pid) == Ok(kept.group),
            None => member.group == i32::try_from(member.key.pid).ok(),
        })
        .cloned()
        .collect()
}

/// Whether `member` is a server in the way the harness keeps them.
fn is_server(kept: Kept, launch: &HashSet<ProcessKey>, member: &Member) -> bool {
    match kept.servers {
        Servers::InGroup => member.group == Some(kept.group),
        // A process whose group could not be read is not known to be outside the window's.
        Servers::AtLaunch => {
            launch.contains(&member.key) && member.group.is_some_and(|group| group != kept.group)
        }
        Servers::Unknown => false,
    }
}

/// Whether `member` is the window's program: it leads the window's group, so the
/// group's number is its own.
fn leads(kept: Kept, member: &Member) -> bool {
    i32::try_from(member.key.pid) == Ok(kept.group)
}

/// The processes of `tree` that are programs of `harness`'s own, and what descends
/// from them: a program named `<harness>-…` (`codex-code-mode-host`) that runs from
/// the directory a program of the harness's own binary does (`codex`), by the path
/// each runs from. The harness's binary itself is none of them: what it starts is
/// the agent's, its commands among it.
pub(super) fn programs(harness: &str, tree: &[Member]) -> HashSet<ProcessKey> {
    let harness = harness.to_ascii_lowercase();
    // Where a program of the harness's own binary runs from: named for it, or run
    // under its name (a launcher that links the program to another name).
    let homes: Vec<&str> = tree
        .iter()
        .filter(|member| name_of(&member.program) == harness || stem_of(&member.exe) == harness)
        .filter_map(|member| split(&member.exe).map(|(dir, _)| dir))
        .collect();
    let named = format!("{harness}-");
    let roots: HashSet<ProcessKey> = tree
        .iter()
        .filter(|member| {
            split(&member.exe).is_some_and(|(dir, file)| {
                lower(strip_exe(file)).starts_with(&named)
                    && homes.iter().any(|home| same_directory(home, dir))
            })
        })
        .map(|member| member.key)
        .collect();
    descending_from(&roots, tree)
}

/// The directory and the file of a program's path, split at its last separator,
/// whichever of the two a platform has.
fn split(exe: &str) -> Option<(&str, &str)> {
    let at = exe.rfind(['/', '\\'])?;
    let (dir, file) = (&exe[..at], &exe[at + 1..]);
    (!file.is_empty()).then_some((dir, file))
}

/// `file` without a `.exe`, which Windows' programs have.
fn strip_exe(file: &str) -> &str {
    let lower = file.to_ascii_lowercase();
    if lower.ends_with(".exe") {
        &file[..file.len() - ".exe".len()]
    } else {
        file
    }
}

fn lower(text: &str) -> String {
    text.to_ascii_lowercase()
}

/// The name a program is known by, as the system keeps it: without a `.exe`, in
/// lower case.
fn name_of(program: &str) -> String {
    lower(strip_exe(program))
}

/// The name of the file a path names, as [`name_of`] says it; none for no path.
fn stem_of(exe: &str) -> String {
    split(exe).map_or_else(String::new, |(_, file)| name_of(file))
}

/// Whether two directories are the same one: Windows' have no case.
fn same_directory(a: &str, b: &str) -> bool {
    if cfg!(windows) {
        a.eq_ignore_ascii_case(b)
    } else {
        a == b
    }
}

#[cfg(test)]
mod tests;

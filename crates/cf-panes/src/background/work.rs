//! Which of the processes a window's tree began after a turn's mark are the work of
//! that turn, which the host can prove and so ends and names. It proves it one way,
//! the same on every platform and for every harness, and tells nothing else for the
//! turn's:
//!
//! **A shell the harness started, and a chain of processes that run down from it.**
//! The agent's commands are run by a shell the harness starts for each of them (`bash
//! -c …`, `zsh -c …`, `powershell -Command …`), and what a command starts is the
//! shell's to the tree. A server is the program its configuration names (`npx`,
//! `node`, `uvx`, `docker`), started with no shell of the user's to run it. So a
//! process is the turn's when the chain of processes that run, up from it through
//! those that began after the mark, ends in a shell that a process of the harness's
//! (one that began before the mark) started: each link is a process that runs, which
//! started the next.
//!
//! What has no such chain is not the turn's, whatever it is. A job the agent detaches
//! (`nohup sleep 600 &` from a shell that then ends, `Start-Process`) loses the shell
//! it was started by, and its having lost it is no proof that the agent started it: a
//! harness's own helper does as well, and a process in the window's session, or in a
//! group the host does not know, may be a server's child that outlived the server. A
//! shell that has lost its parent, or that a late server started, is not proved the
//! harness's own. The host leaves them all running, does not end them and does not
//! name them (the window's close ends them with everything else the window started).
//! What is the harness's by where it is, by its program or by having been seen the
//! harness's ([`super::harness`]), is never the turn's either, whatever shell it seems
//! to descend from.
//!
//! Limits, each of them a process that is left running, never one that is ended by
//! mistake, but for the last:
//!
//! - A command whose shell has ended, whichever way the agent ended it (`nohup … &`,
//!   `setsid`, `disown`, `Start-Process`, a double fork), is left running. What the host
//!   ends is what a shell that still runs started: a command the shell tool of the
//!   harness is still waiting for when the turn ends, or one the harness runs in the
//!   background for its agent and keeps the shell of.
//! - `cmd.exe` is no shell here. npm installs every server it runs on Windows
//!   behind a `cmd.exe /c npx …`, which the harness starts as it does the agent's
//!   command, and what the agent runs through `cmd /c` is left running with it.
//! - A process the agent started from a shell the harness started before the mark
//!   (a shell that outlives its turn) is no shell's to the host, which stops its walk
//!   at the mark, and is left.
//! - A server the user wraps in `bash` or `powershell` and the harness starts after
//!   the mark is taken for the agent's command, and ended, where the harness is not
//!   known to keep its servers in a way ([`cf_proto::panes::Servers`]), unless it was
//!   seen the harness's while it ran. It is the one case of this rule that is wrong
//!   the other way: the servers the harnesses document are started as programs, and
//!   none of them was seen started by a shell.

use std::collections::{HashMap, HashSet};

use super::{Member, ProcessKey};

/// The shells an agent's commands run in, by the name of the program, without a
/// `.exe`: the Bourne family, the others the harnesses start on Unix, and the two
/// PowerShells.
const SHELLS: [&str; 10] = [
    "sh",
    "bash",
    "zsh",
    "dash",
    "ksh",
    "fish",
    "csh",
    "tcsh",
    "powershell",
    "pwsh",
];

/// Whether `program` is the name of one of [`SHELLS`]; Windows' names have no case.
fn is_shell(program: &str) -> bool {
    let name = program.to_ascii_lowercase();
    let name = name.strip_suffix(".exe").unwrap_or(&name);
    SHELLS.contains(&name)
}

/// Whether `member` runs a shell: by the file its path names where the system says
/// it (a script a shell runs is the shell's, and a process that has just run another
/// program may still have the name it had), else by the name of its program.
fn runs_a_shell(member: &Member) -> bool {
    match member.exe.rsplit(['/', '\\']).next() {
        Some(file) if !file.is_empty() => is_shell(file),
        _ => is_shell(&member.program),
    }
}

/// The processes of `started`, which began after the turn's mark, that are the turn's
/// work: each that is not in `protected`, and is, or descends from, a shell that a
/// process of the harness's started. `protected` is what is never the turn's, with
/// whatever descends from it.
pub(super) fn turns(started: &[&Member], protected: &HashSet<ProcessKey>) -> HashSet<ProcessKey> {
    let by_key: HashMap<ProcessKey, &Member> =
        started.iter().map(|member| (member.key, *member)).collect();
    started
        .iter()
        .filter(|member| !protected.contains(&member.key) && descends_from_a_shell(member, &by_key))
        .map(|member| member.key)
        .collect()
}

/// Whether `member` is, or descends from, a shell that the harness started: the walk up
/// from it, through the parents that began after the mark, ends at a process that is a
/// shell and has a parent that is none of those, which is a process of the harness's,
/// from before the mark. It ends at none that has no parent (it has gone: no chain),
/// and at none that is no shell (a server a shell was started by is not proved the
/// harness's own).
fn descends_from_a_shell(member: &Member, started: &HashMap<ProcessKey, &Member>) -> bool {
    let mut at = member;
    // A tree names no process twice among a process's ancestors: the bound is
    // only for a list that is no tree.
    for _ in 0..=started.len() {
        match at.parent.and_then(|parent| started.get(&parent)) {
            Some(parent) => at = parent,
            None => return at.parent.is_some() && runs_a_shell(at),
        }
    }
    false
}

#[cfg(test)]
mod tests;

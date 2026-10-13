//! Which of the processes a window's tree began after a turn's mark are the work of
//! that turn, which the host can prove and so ends and names. It proves it two ways,
//! the same on every platform and for every harness, and tells nothing else for the
//! turn's:
//!
//! - **A shell.** The agent's commands are run by a shell the harness starts for
//!   each of them (`bash -c …`, `zsh -c …`, `powershell -Command …`), and what a
//!   command starts is the shell's to the tree. A server is the program its
//!   configuration names (`npx`, `node`, `uvx`, `docker`), started with no shell of
//!   the user's to run it. So a process is the turn's when it, or a process it
//!   descends from that began after the mark, is a shell.
//! - **An orphan.** A job the agent detaches (`nohup sleep 600 &` from a shell that
//!   then ends) loses the shell it was started by, and is left to the system's init.
//!   The tree finds it all the same, by the window's session or by what it saw the
//!   window's program start on macOS, and by the pane's job object on Windows, and
//!   says no parent of it is left ([`Parent::Gone`]). What the window's tree holds is
//!   what the window's program started, and a process of it that began after the mark
//!   and has no parent left is a job someone detached: the turn's, with what
//!   descends from it.
//!
//! What is neither is not the turn's, whatever it is: a server a harness starts late
//! (Claude Code's MCP servers begin as it takes its first message, after the mark;
//! Codex's begin when a tool first needs one), a helper the harness keeps. The host
//! cannot tell it from a job by anything it is, and leaves it alone, and says
//! nothing of it. What is the harness's by where it is, or by its program
//! ([`super::harness`]), is never the turn's, whatever shell or orphan it seems to be.
//!
//! Limits, each of them a process that is left running, never one that is ended by
//! mistake, but for the last:
//!
//! - `cmd.exe` is no shell here. npm installs every server it runs on Windows
//!   behind a `cmd.exe /c npx …`, which the harness starts as it does the agent's
//!   command, and what the agent runs through `cmd /c` is left running with it.
//! - A process that left the window's session and the pane's marker (a job the
//!   agent started with `setsid` from a program that shows no environment, whose
//!   parent had gone) is in the tree only if the host saw the shells that started
//!   it start while they ran: one that began and ended before the host looked (on a
//!   machine with every core busy, a shell that ends at once) is not in it at all.
//! - A process the agent started from a shell the harness started before the mark
//!   (a shell that outlives its turn) is no shell's to the host, which stops its walk
//!   at the mark, and is left.
//! - A server the user wraps in `bash` or `powershell` and the harness starts after
//!   the mark is taken for the agent's command, and ended, where the harness is not
//!   known to keep its servers in a way ([`cf_proto::panes::Servers`]); and so is a
//!   process of the harness's own that has lost the process that started it and is
//!   no program named for the harness beside its binary. These are the cases of this
//!   rule that are wrong the other way: the servers the harnesses document are
//!   started as programs, and none of them was seen started by a shell.

use std::collections::{HashMap, HashSet};

use super::{Member, Parent, ProcessKey};

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
/// work: each that is not in `protected`, and is, or descends from, a shell or a
/// process whose parent is gone. `protected` is what is never the turn's, with
/// whatever descends from it.
pub(super) fn turns(started: &[&Member], protected: &HashSet<ProcessKey>) -> HashSet<ProcessKey> {
    let by_key: HashMap<ProcessKey, &Member> =
        started.iter().map(|member| (member.key, *member)).collect();
    started
        .iter()
        .filter(|member| !protected.contains(&member.key) && in_a_line_of_work(member, &by_key))
        .map(|member| member.key)
        .collect()
}

/// Whether `member`, or a process it descends from that began after the mark, is a
/// shell or has no parent left. The walk up stops at a process that began before the
/// mark, which the window's program is (a shell, in a test): it is no shell of the
/// agent's.
fn in_a_line_of_work(member: &Member, started: &HashMap<ProcessKey, &Member>) -> bool {
    let mut at = member;
    // A tree names no process twice among a process's ancestors: the bound is
    // only for a list that is no tree.
    for _ in 0..=started.len() {
        if runs_a_shell(at) || at.parent == Parent::Gone {
            return true;
        }
        match at.parent.of().and_then(|parent| started.get(&parent)) {
            Some(parent) => at = parent,
            None => return false,
        }
    }
    false
}

#[cfg(test)]
mod tests;

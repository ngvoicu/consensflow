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
//! What has no such chain is not the turn's **by a chain**, whatever it is. A job the agent
//! detaches (`nohup sleep 600 &` from a shell that then ends, `Start-Process`) loses the
//! shell it was started by, and its having lost it is no proof that the agent started it: a
//! harness's own helper does as well, and a process in the window's session, or in a group
//! the host does not know, may be a server's child that outlived the server. The host does not
//! call such a process the turn's work, and does not end it. It is not nothing, though: a
//! window's close ends it where the tree finds it the window's (all of them on Windows, where
//! the job holds them; on the Mac those in the session the window's program leads, or under
//! it), so a pause that kept a window for the work it would end, and the note of a close, said
//! nothing of it, and a close ended it unnamed (T-1145: Muse on Windows detached a `ping` with
//! `Start-Process`, and Pi and Devin on the Mac detach with `&`). So the host names it
//! as **detached work** ([`detached`]), beside the turn's: a process of the tree that began after
//! the mark, that is none of the harness's own, and whose starter has gone (no parent), with
//! whatever descends from it, **where the platform can prove it the agent's**. The chain is still the
//! only proof of whose it is; this says that the window would end it, and that nothing but a
//! detach explains it. On the Mac it is in the window's session or under its program, which a close
//! ends: a daemon (Gradle's, an `adb` server) makes a session of its own, which is what keeps it
//! out. On Windows the pane's job holds a build's daemon as it holds a `Start-Process` job, and
//! the owner does not want the chief told of the daemon (2026-10-15), so it is proved by what
//! started it, read when it began: a shell ([`super::births`]). It is listed so that a pause keeps
//! its window for it, the chief is told it runs and a close names what it ended; it is never
//! ended by anything but the close, and never taken for a command the harness waits on
//! ([`super::Baseline::commands`] goes by the chain alone).
//!
//! Limits, each of them a process that is left running, never one that is ended by
//! mistake, but for the last:
//!
//! - A command whose shell has ended, whichever way the agent ended it (`nohup … &`,
//!   `setsid`, `disown`, `Start-Process`, a double fork), is not the turn's by a chain: what
//!   the host ends *as the turn's* is what a shell that still runs started (a command the
//!   shell tool of the harness is still waiting for when the turn ends, or one the harness
//!   runs in the background for its agent and keeps the shell of). What it left is
//!   *detached work* ([`detached`]) where the tree holds it. **Where the tree does not hold
//!   it, nothing names it**: on the Mac a job that left the window's session (`setsid`, and
//!   the tool shells of Claude Code, Pi and Devin, which lead sessions of their own: their
//!   `nohup … &` jobs are in a session of their own once the shell has ended, T-1068's logs
//!   show a terminal of `??`) with its shell gone,
//!   whose program is a system binary (`sleep`, `ping`: no environment to read a marker
//!   from) and which no look saw under the window's program, has no tie to the window left
//!   (its session leader and its parent have gone, and the system keeps no record of who
//!   started it that holds: `process_tree::native`). `kqueue`'s `NOTE_TRACK` would follow it
//!   from its birth, and macOS answers `EOPNOTSUPP` to it (measured on Darwin 27.0, T-1145);
//!   the Endpoint Security framework needs an entitlement a ConsensFlow build does not
//!   have. Such a job survives the window's close too, so a pause that closed the window
//!   ended nothing of it, and a marker that can be read names it as possibly the window's
//!   (`possibly_left`). **This stays the written limit** (T-1100, answer A, 2026-11-03): the
//!   kernel's "responsible process" of a process survives `setsid`, a double fork and a
//!   system binary, and a program spawned with `responsibility_spawnattrs_setdisclaim` is
//!   its own, so the job would read as the window's program's; that costs the TCC
//!   attribution of the harness (a prompt in a window nobody watches) and a hand-rolled
//!   `posix_spawn` for all five harnesses in place of portable-pty's fork path, too much
//!   for 3.0.7. On Windows no such job exists: the pane's job holds what a
//!   process starts wherever it goes. What it cannot prove there is a job whose starter was
//!   not read as a shell: `cmd.exe /c start …` (`cmd.exe` is no shell here), one started
//!   by a service (`schtasks`, a WMI call), and one whose birth the job's port did not tell.
//! - On Windows a program of Git Bash's own kind that a script started (`sleep`, `grep`) names
//!   a parent that has gone, a forked copy of the shell, so the walk up from it stops there:
//!   the reader of the job's members gives it the parent `ps -W` of Git Bash says
//!   ([`crate::cygwin`]). Where the script itself has ended before the look (a harness ended it
//!   by its number), its command's shell has ended, and it is left as above.
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
pub(super) fn is_shell(program: &str) -> bool {
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

/// The processes of `started`, which began after the turn's mark or were detached work at an
/// earlier turn's ([`super::Baseline::mark`] carries them), that are **detached work**: a job whose starter has gone, and what descends from it. Each is not in
/// `protected` (what is never the turn's, with whatever descends from it), and the walk up
/// from it, through the parents that began after the mark, ends at a process that has no
/// parent at all: the one that started it is no process of the tree any longer
/// (`nohup sleep 600 &` from a shell that ended, `Start-Process`).
///
/// A walk that ends at a parent that began before the mark ends at none: that parent is the
/// window's program, a server or the harness's own helper, and what it started is the
/// harness's to run (a program Codex starts without a shell is one: T-1137 reads Codex's own
/// count of them). Nor is anything the chain proves the turn's here ([`turns`]): the two
/// never meet, for a chain ends at a shell with a parent and this at a process with none.
///
/// **What this takes for detached work and is not**: a process of the harness's own that
/// began after the mark, outlived its parent and is told harness's by none of the ways above
/// (where it keeps its servers, the programs it is named for, having been seen so). The
/// second look [`super::steady`] makes drops what ends within a moment (a status line's
/// `git`), and nothing else is told apart: a long-lived helper that a harness starts in the
/// turn and orphans would hold a paused window open until its work ended or the human
/// deleted the session, and would be named in the note of a close. That is a window kept
/// for something the close would end, the cost this rule chooses over a window closed on
/// work it ended unnamed.
pub(super) fn detached(
    started: &[&Member],
    protected: &HashSet<ProcessKey>,
) -> HashSet<ProcessKey> {
    let by_key: HashMap<ProcessKey, &Member> =
        started.iter().map(|member| (member.key, *member)).collect();
    started
        .iter()
        .filter(|member| lost_its_starter(member, &by_key, protected))
        .map(|member| member.key)
        .collect()
}

/// Whether the walk up from `member`, through the parents that began after the mark, ends at
/// a process with no parent, and none of the processes met is `protected`.
fn lost_its_starter(
    member: &Member,
    started: &HashMap<ProcessKey, &Member>,
    protected: &HashSet<ProcessKey>,
) -> bool {
    let mut at = member;
    // A tree names no process twice among a process's ancestors: the bound is
    // only for a list that is no tree.
    for _ in 0..=started.len() {
        if protected.contains(&at.key) {
            return false;
        }
        match at.parent {
            None => return true,
            Some(parent) => match started.get(&parent) {
                Some(parent) => at = parent,
                // A parent from before the mark: the harness's, not a job that lost its starter.
                None => return false,
            },
        }
    }
    false
}

#[cfg(test)]
mod tests;

//! Which of the processes a window's tree began after a turn's mark are the agent's
//! commands, where the daemon did not say how its harness keeps its servers: a
//! harness not known to keep them in a way (OpenCode and Devin were not probed)
//! and every harness on Windows, where a job object has no process groups to tell
//! them by. What the host has to go by there is when each process began and which
//! process began it ([`Member::parent`]), and neither tells a server from a job:
//! Claude Code starts its servers as it takes its first message, after the mark,
//! as a child of its own process, and starts each command its agent runs as a
//! child of its own process too. What differs is what each runs. The agent's
//! command is run by a shell the harness starts for it (`bash -c …`,
//! `powershell -Command …`), and what the command starts is the shell's to the
//! tree; a server is the program its configuration names (`npx`, `node`, `uvx`,
//! `docker`), started with no shell of the user's to run it. So a process is the
//! turn's when it, or a process it descends from that began after the mark, is a
//! shell, and it is the harness's, or no one can say, otherwise.
//!
//! Limits, each of them a process that is left running and said to be, never one
//! that is ended by mistake:
//!
//! - `cmd.exe` is no shell here. npm installs every server it runs on Windows
//!   behind a `cmd.exe /c npx …`, which the harness starts as it does the agent's
//!   command, and what the agent runs through `cmd /c` is left running with it.
//! - A process whose parent has gone (a job the agent's shell left with `&` or
//!   `Start-Process` and ended, a browser that outlived its launcher) has no shell
//!   to be found under: it is told by its own place alone, and there is none.
//! - A server the user wraps in `bash` or `powershell` and the harness starts after
//!   the mark is taken for the agent's command, and ended. It is the one case of
//!   this rule that is wrong the other way: the servers the harnesses document are
//!   started as programs, and none of them was seen started by a shell.

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

/// The processes of `started`, which began after the turn's mark, that are the
/// turn's: each one that is a shell, or descends from one of them.
pub(super) fn turns(started: &[Member]) -> HashSet<ProcessKey> {
    let by_key: HashMap<ProcessKey, &Member> =
        started.iter().map(|member| (member.key, member)).collect();
    started
        .iter()
        .filter(|member| under_a_shell(member, &by_key))
        .map(|member| member.key)
        .collect()
}

/// Whether `member`, or a process it descends from that began after the mark, is a
/// shell. The walk up stops at a process that began before the mark, which the
/// window's program is (a shell, in a test): it is no shell of the agent's.
fn under_a_shell(member: &Member, started: &HashMap<ProcessKey, &Member>) -> bool {
    let mut at = member;
    // A tree names no process twice among a process's ancestors: the bound is
    // only for a list that is no tree.
    for _ in 0..=started.len() {
        if is_shell(&at.program) {
            return true;
        }
        match at.parent.and_then(|parent| started.get(&parent)) {
            Some(parent) => at = parent,
            None => return false,
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::background::Baseline;

    /// A process of `program` started by the process `parent`, if any.
    fn process(pid: u32, program: &str, parent: Option<u32>) -> Member {
        Member {
            key: key(pid),
            command: format!("{program} {pid}"),
            program: program.to_owned(),
            group: None,
            parent: parent.map(key),
        }
    }

    fn key(pid: u32) -> ProcessKey {
        ProcessKey {
            pid,
            began: u64::from(pid),
        }
    }

    fn pids(members: &[Member]) -> Vec<u32> {
        members.iter().map(|member| member.key.pid).collect()
    }

    #[test]
    fn a_shell_is_known_by_the_name_of_its_program_with_or_without_the_extension_and_the_case_of_windows(
    ) {
        for shell in [
            "sh",
            "bash",
            "zsh",
            "bash.exe",
            "BASH.EXE",
            "PowerShell.exe",
            "pwsh.exe",
            "dash",
        ] {
            assert!(is_shell(shell), "{shell}");
        }
        for other in [
            "cmd.exe",
            "node.exe",
            "npx",
            "python3",
            "msedge.exe",
            "ssh",
            "bashful",
            "",
            ".exe",
        ] {
            assert!(!is_shell(other), "{other}");
        }
    }

    #[test]
    fn a_shell_the_turn_started_and_what_it_started_are_the_turns_and_nothing_else_is() {
        let started = [
            // The agent's command: a shell, the browser it runs, and what the
            // browser starts.
            process(40, "bash.exe", Some(10)),
            process(41, "msedge.exe", Some(40)),
            process(42, "msedge.exe", Some(41)),
            // A server, begun late: no shell, and what it starts is no shell's.
            process(50, "node.exe", Some(10)),
            process(51, "chrome.exe", Some(50)),
            // What a `.cmd` shim starts.
            process(60, "cmd.exe", Some(10)),
            process(61, "node.exe", Some(60)),
        ];
        let turns = turns(&started);
        let mut turned: Vec<u32> = turns.iter().map(|key| key.pid).collect();
        turned.sort_unstable();
        assert_eq!(turned, [40, 41, 42]);
    }

    #[test]
    fn a_process_whose_parent_is_not_among_what_the_turn_started_has_no_shell_to_be_found_under() {
        // The shell the job was started by has gone, or began before the turn: the
        // window's program, a shell in a test, is not the agent's.
        let started = [
            process(70, "sleep", None),
            process(71, "sleep", Some(10)),
            process(72, "sleep", Some(99)),
        ];
        assert!(turns(&started).is_empty());
    }

    #[test]
    fn a_list_that_names_its_processes_as_each_others_parents_is_no_tree_and_ends() {
        let started = [
            process(80, "sleep", Some(81)),
            process(81, "sleep", Some(80)),
        ];
        assert!(turns(&started).is_empty());
    }

    /// The window's program (a shell: a test's, a supervisor's) and its tree before
    /// the turn: a server it started.
    fn before() -> Vec<Member> {
        vec![
            process(10, "bash", None),
            process(20, "node", Some(10)),
            process(21, "chromium", Some(20)),
        ]
    }

    fn marked() -> Baseline {
        let mut baseline = Baseline::default();
        baseline.mark(&before(), None);
        baseline
    }

    #[test]
    fn where_the_harness_is_not_known_to_keep_its_servers_an_ending_ends_what_a_shell_started_and_leaves_the_rest(
    ) {
        let mut tree = before();
        tree.extend([
            process(40, "zsh", Some(10)),
            process(41, "sleep", Some(40)),
            process(42, "tail", Some(40)),
            // A server that began after the mark, and its browser.
            process(50, "node", Some(10)),
            process(51, "chromium", Some(50)),
            // What a server the window had before the turn started, since.
            process(22, "chromium", Some(20)),
        ]);
        let plan = marked().ending(&tree, &[40, 41, 42, 50, 51, 22]);
        assert_eq!(pids(&plan.end), [40, 41, 42]);
        // The oldest first, as the host lists them.
        assert_eq!(pids(&plan.left), [22, 50, 51]);
    }

    #[test]
    fn what_descends_from_a_process_that_is_ended_is_ended_with_it_though_it_was_not_asked_for() {
        let mut tree = before();
        tree.extend([
            process(40, "bash", Some(10)),
            process(41, "msedge", Some(40)),
            process(42, "msedge", Some(41)),
            process(43, "msedge", Some(42)),
        ]);
        // The daemon names the first of them only, as the host lists no more.
        let plan = marked().ending(&tree, &[40]);
        assert_eq!(pids(&plan.end), [40, 41, 42, 43]);
        assert!(plan.left.is_empty());
    }

    #[test]
    fn only_what_is_asked_for_is_ended_and_what_is_gone_or_never_the_turns_is_nobodys_business() {
        let mut tree = before();
        tree.extend([
            process(40, "bash", Some(10)),
            process(41, "sleep", Some(40)),
            process(44, "bash", Some(10)),
            process(45, "sleep", Some(44)),
        ]);
        // The second job is not asked for; 20 is the server's, 99 is gone.
        let plan = marked().ending(&tree, &[40, 20, 99]);
        assert_eq!(pids(&plan.end), [40, 41]);
        assert!(plan.left.is_empty());
    }
}

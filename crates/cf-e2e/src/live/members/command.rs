//! A long command a member is asked to run, and the processes it leaves: found
//! and ended by a mark that is in its command line and nowhere else of the
//! machine's. The checks that cut a window in the middle of its turn (a stop, a
//! quit) need a window that is inside a command, and need to know when the
//! command is gone, which a window that was cut does not take with it.

use std::sync::OnceLock;

use regex::Regex;

use super::super::claude::{processes_with, Process};
use crate::process::{signal, Signal};
use crate::{pattern, Result};

/// A command that runs for five minutes, which a check finds and ends by its mark.
#[derive(Debug, Clone, Copy)]
pub struct LongCommand {
    mark: &'static str,
}

impl LongCommand {
    /// The command whose command line ends with `mark`.
    pub const fn new(mark: &'static str) -> Self {
        Self { mark }
    }

    /// The command, as a member is told to run it: Node waits for five minutes, and
    /// the mark is a word it is given and does nothing with.
    pub fn text(&self) -> String {
        format!("node -e \"setTimeout(() => {{}}, 300000)\" {}", self.mark)
    }

    /// Its own processes: the `node` it runs, not the window whose launch text (the
    /// task, as an argument of Claude) names it too.
    pub fn running(&self) -> Result<Vec<Process>> {
        Ok(of_node(processes_with(self.mark)?))
    }

    /// Every process of the machine that names the mark in its command line: a
    /// window whose members were given the command in a message names it in none,
    /// so for them it is the command's own, and its shell's.
    pub fn named(&self) -> Result<Vec<Process>> {
        processes_with(self.mark)
    }

    /// Ends every process of the machine that has the mark in its command line,
    /// the window's among them: what a window that was cut leaves is not left.
    pub fn end(&self) {
        for process in processes_with(self.mark).unwrap_or_default() {
            let _ = signal(process.pid, Signal::Kill);
        }
    }
}

/// The processes of `found` that are the `node -e` of the command: the program
/// Node (by its name, with or without the folder it is in), then `-e`.
fn of_node(found: Vec<Process>) -> Vec<Process> {
    static NODE: OnceLock<Regex> = OnceLock::new();
    let node = pattern::once(&NODE, r"^(\S*/)?node\s+-e\s");
    found
        .into_iter()
        .filter(|process| node.is_match(&process.command))
        .collect()
}

/// How many characters of a command a line of processes shows.
const LINE: usize = 110;

/// Processes as `pid (parent) command`, each command cut to 110 characters, the
/// folder of its program given up first where that is what makes it long (see
/// [`in_a_line`]).
pub fn shown(found: &[Process]) -> Vec<String> {
    found
        .iter()
        .map(|process| {
            format!(
                "{} ({}) {}",
                process.pid,
                process.ppid,
                in_a_line(&process.command)
            )
        })
        .collect()
}

/// `command` as a line of [`LINE`] characters at most. A program that is named by a
/// path (the checkout's, which is as long as the folder a person keeps it in) is told by
/// its file name behind `…/` when the command is longer than a line, so that what ran
/// is in the line wherever the checkout is, and not the folders it is in.
fn in_a_line(command: &str) -> String {
    let (program, rest) = command.split_once(' ').unwrap_or((command, ""));
    let named = match program.rfind(['/', '\\']) {
        Some(at) if command.chars().count() > LINE => {
            let joined = if rest.is_empty() { "" } else { " " };
            format!("…/{}{joined}{rest}", &program[at + 1..])
        }
        _ => command.to_owned(),
    };
    named.chars().take(LINE).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn process(pid: u32, ppid: u32, command: &str) -> Process {
        Process {
            pid,
            ppid,
            command: command.to_owned(),
        }
    }

    #[test]
    fn the_command_is_node_waiting_five_minutes_with_the_mark_as_a_word_it_is_given() {
        let command = LongCommand::new("cf-stops-live-long-command");
        assert_eq!(
            command.text(),
            "node -e \"setTimeout(() => {}, 300000)\" cf-stops-live-long-command"
        );
    }

    #[test]
    fn only_the_node_that_runs_the_command_is_the_command_not_the_window_that_names_it_too() {
        let found = vec![
            process(10, 1, "node -e \"setTimeout(() => {}, 300000)\" cf-x"),
            process(
                11,
                10,
                "/opt/homebrew/bin/node -e \"setTimeout(() => {}, 300000)\" cf-x",
            ),
            // The window's launch text holds the task, and so the command.
            process(
                12,
                1,
                "claude --session-id s Run exactly this: node -e \"x\" cf-x",
            ),
            process(13, 1, "/bin/sh -c node -e \"x\" cf-x"),
            process(14, 1, "nodemon -e js cf-x"),
            process(15, 1, "node script.js cf-x"),
        ];
        let node: Vec<u32> = of_node(found).iter().map(|process| process.pid).collect();
        assert_eq!(node, [10, 11]);
    }

    #[test]
    fn processes_are_shown_with_their_parent_and_a_command_cut_to_a_line() {
        let long = "x".repeat(300);
        let shown = shown(&[process(7, 1, "node -e a"), process(8, 7, &long)]);
        assert_eq!(shown[0], "7 (1) node -e a");
        assert_eq!(shown[1].len(), "8 (7) ".len() + 110);
    }

    #[test]
    fn a_program_named_by_a_long_path_is_told_by_its_file_name_so_the_line_holds_what_ran() {
        // The checkout's folder is as long as a person made it: the program was cut off
        // behind it, and the test that looked for its flags failed in a long-named worktree.
        let folder = "/Users/someone/Projects/work/trees/a-long-name-for-a-worktree/app/src-tauri/target/debug";
        let long = format!("{folder}/fake-agent --background-job 120 /a/job/file");
        assert!(long.chars().count() > LINE);
        let shown = shown(&[process(9, 1, &long)]);
        assert_eq!(
            shown[0],
            "9 (1) …/fake-agent --background-job 120 /a/job/file"
        );
        // Only a command longer than a line gives its folder up; the others are as they are.
        let short = process(4, 1, "/bin/sh -c sleep");
        assert_eq!(shown_line(&short), "4 (1) /bin/sh -c sleep");
        // A program alone, and a Windows path, are told the same way; one that is no path is cut.
        let alone = format!("{folder}/{folder}/consensflow-bridge");
        assert_eq!(
            shown_line(&process(5, 2, &alone)),
            "5 (2) …/consensflow-bridge"
        );
        let windows = format!(
            "C:\\Users\\x\\{}\\fake-agent.exe --background-job 5",
            "y".repeat(100)
        );
        assert_eq!(
            shown_line(&process(6, 1, &windows)),
            "6 (1) …/fake-agent.exe --background-job 5"
        );
        let no_path = format!("node {}", "z".repeat(200));
        assert_eq!(
            shown_line(&process(8, 1, &no_path)).len(),
            "8 (1) ".len() + LINE
        );
    }

    /// The one line `shown` makes of one process.
    fn shown_line(found: &Process) -> String {
        shown(std::slice::from_ref(found)).remove(0)
    }

    #[cfg(unix)]
    #[test]
    fn a_command_that_runs_is_found_by_its_mark_and_ended_with_it() {
        use std::time::Duration;

        // A shell stands in for the node: only the name `node -e` makes a process the command's
        // own, so none is here; what ends the process that names the mark is `end`. (Two commands
        // in the shell, so that it stays the process that holds the mark.)
        let mark = "cf-live-members-test-mark-4171";
        let command = LongCommand::new(mark);
        let mut shell = crate::process::Run::new("/bin/sh")
            .args(["-c", &format!("sleep 8; : {mark}")])
            .inheriting_env()
            .spawn()
            .unwrap();
        let named = crate::live::until(Duration::from_secs(10), Duration::from_millis(50), || {
            let found = processes_with(mark).ok()?;
            (!found.is_empty()).then_some(found)
        });
        assert!(named.is_some(), "the shell that holds the mark is listed");
        assert!(command.running().unwrap().is_empty(), "no node runs it");
        command.end();
        let status = shell.wait(Duration::from_secs(10)).unwrap();
        assert!(status.is_some_and(|status| !status.success()), "{status:?}");
        assert!(processes_with(mark).unwrap().is_empty());
    }
}

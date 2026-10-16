//! A long command a member is asked to run, and the processes it leaves: found
//! and ended by a mark that is in its command line and nowhere else of the
//! machine's. The checks that cut a window in the middle of its turn (a stop, a
//! quit) need a window that is inside a command, and need to know when the
//! command is gone, which a window that was cut does not take with it. A check
//! that only needs a window to be inside a command for a while, on any machine,
//! asks for one that ends by itself ([`quiet_command`]): it has nothing to find
//! and nothing to end.

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

/// A command that says nothing and ends by itself after `seconds` seconds, as a
/// member of a machine that is `windows` (or is not) is told to run it: `sleep`
/// where there is one, and a ping of the machine's own address on Windows, which
/// has none that every shell knows (a reply a second, the first at once, so one
/// more than the seconds).
pub fn quiet_command(seconds: u64, windows: bool) -> String {
    if windows {
        format!("ping -n {} 127.0.0.1", seconds + 1)
    } else {
        format!("sleep {seconds}")
    }
}

/// Whether the command line `line`, as a process list or the host's tree gives it, ends
/// by running `command` (a program, then its words): its last words are the program, as
/// told or named by a path, quoted or not, with or without `.exe` and in any case, then
/// exactly the command's words and nothing after them. A shell's line that ends with the
/// command runs it too (`/bin/sh -c "$0" "$@" sleep 119`). Windows' PowerShell starts a
/// program it finds on the `PATH` by its path, and the line of a `ping -n 120 127.0.0.1`
/// it ran is `"C:\WINDOWS\System32\PING.EXE" -n 120 127.0.0.1` (zeewin, 2026-10-16).
pub fn runs_command(line: &str, command: &str) -> bool {
    let line = line.trim_end();
    match command.split_once(' ') {
        Some((program, words)) => line
            .strip_suffix(words)
            .and_then(|before| before.strip_suffix(' '))
            .is_some_and(|before| ends_with_program(before, program)),
        None => ends_with_program(line, command),
    }
}

/// Whether `before`, a command line up to a word, ends with the program `program` as a
/// word of its own: named bare or by a path, quoted or not, with or without `.exe`, in
/// any case.
fn ends_with_program(before: &str, program: &str) -> bool {
    let named = match before.strip_suffix('"') {
        // A quoted path is a word of its own where its opening quote begins one.
        Some(quoted) => match quoted.rsplit_once('"') {
            Some((ahead, named)) if ahead.is_empty() || ahead.ends_with(' ') => named,
            _ => return false,
        },
        None => before.rsplit(' ').next().unwrap_or(before),
    };
    let file = named.rsplit(['/', '\\']).next().unwrap_or(named);
    let stem = match file.len().checked_sub(".exe".len()) {
        Some(at) if file.is_char_boundary(at) && file[at..].eq_ignore_ascii_case(".exe") => {
            &file[..at]
        }
        _ => file,
    };
    stem.eq_ignore_ascii_case(program)
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
    fn a_quiet_command_is_a_sleep_and_on_windows_a_ping_of_the_machines_own_address_a_reply_more_than_the_seconds(
    ) {
        assert_eq!(quiet_command(45, false), "sleep 45");
        assert_eq!(quiet_command(45, true), "ping -n 46 127.0.0.1");
    }

    #[test]
    fn a_line_runs_the_command_as_it_was_told_or_by_its_programs_path_as_powershell_starts_it() {
        let ping = "ping -n 120 127.0.0.1";
        // As it was told, alone or in its shell's line.
        assert!(runs_command("ping -n 120 127.0.0.1", ping));
        assert!(runs_command("bash -c ping -n 120 127.0.0.1", ping));
        assert!(runs_command(
            "/bin/sh -c \"$0\" \"$@\" sleep 119",
            "sleep 119"
        ));
        // As PowerShell started it on zeewin: its path quoted, the name in capitals, `.exe`.
        assert!(runs_command(
            r#""C:\WINDOWS\System32\PING.EXE" -n 120 127.0.0.1"#,
            ping
        ));
        // And by a path unquoted, a folder with a space in quotes, a Unix path.
        assert!(runs_command(
            r"C:\Windows\system32\ping.exe -n 120 127.0.0.1",
            ping
        ));
        assert!(runs_command(
            r#""C:\Program Files\Tools\ping.exe" -n 120 127.0.0.1"#,
            ping
        ));
        assert!(runs_command("/sbin/ping -n 120 127.0.0.1", ping));
        // Another program with the words, other words, a name that only ends in it.
        assert!(!runs_command(
            r#""C:\WINDOWS\System32\tracert.exe" -n 120 127.0.0.1"#,
            ping
        ));
        assert!(!runs_command(
            r#""C:\WINDOWS\System32\PING.EXE" -n 61 127.0.0.1"#,
            ping
        ));
        assert!(!runs_command(r"C:\tools\noping.exe -n 120 127.0.0.1", ping));
        assert!(!runs_command("-n 120 127.0.0.1", ping));
    }

    #[test]
    fn a_line_runs_the_command_only_in_whole_words_and_only_to_its_end() {
        let (sleep, ping) = ("sleep 119", "ping -n 120 127.0.0.1");
        assert!(runs_command("sleep 119", sleep));
        assert!(runs_command("sleep 119  ", sleep), "spaces after the line");
        // A program whose name only ends in the command's, as told or by a path.
        assert!(!runs_command("nosleep 119", sleep));
        assert!(!runs_command("/usr/bin/nosleep 119", sleep));
        assert!(!runs_command(
            r#""C:\tools\noping.exe" -n 120 127.0.0.1"#,
            ping
        ));
        // A quoted path whose quote does not begin a word of its own.
        assert!(!runs_command(
            r#"x"C:\WINDOWS\System32\PING.EXE" -n 120 127.0.0.1"#,
            ping
        ));
        // A word that only begins with the command's last one.
        assert!(!runs_command("sleep 1190", sleep));
        assert!(!runs_command("ping -n 120 127.0.0.10", ping));
        assert!(!runs_command(
            r#""C:\WINDOWS\System32\PING.EXE" -n 120 127.0.0.10"#,
            ping
        ));
        // More words after the command's.
        assert!(!runs_command("sleep 119 30", sleep));
        assert!(!runs_command("ping -n 120 127.0.0.1 -4", ping));
        // A file name that only begins with the program's, or with `.exe` and more.
        assert!(!runs_command("sleepy 119", sleep));
        assert!(!runs_command(r"C:\x\ping.exe.bak -n 120 127.0.0.1", ping));
        // The words apart from the program by more than the one space it was told with.
        assert!(!runs_command("sleep  119", sleep));
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

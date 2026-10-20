//! A long command a member is asked to run, and the processes it leaves, by a mark
//! that is in its command line. The checks that cut a window in the middle of its
//! turn (a stop, a quit) need a window that is inside a command, and need to know
//! when the command is gone. A check that only needs a window to be inside a command
//! for a while, on any machine, asks for one that ends by itself ([`quiet_command`]):
//! it has nothing to find and nothing to end.
//!
//! A command is found and ended in the window's own tree alone, what descends from
//! the window's process ([`LongCommand::in_window`], [`LongCommand::end_in`], and
//! [`LongCommand::chosen_in`] with [`end_chosen`] for what a window that is cut
//! leaves), never by the mark across the machine: a check that ended every process
//! whose command line held its mark ended the worker that ran it, whose brief quoted
//! the mark (2026-10-16). What is read across the machine by the mark
//! ([`LongCommand::running`]) is read, and nothing of it is ended.

use std::sync::OnceLock;

use regex::Regex;

use super::super::claude::{processes_with, Process};
use super::descent::{
    descending_from, end_chosen, holding, Ended, Identity, Machine, Running, ThisMachine,
};
use crate::{pattern, Result};

/// A command that runs for a while, which a check finds by its mark.
#[derive(Debug, Clone, Copy)]
pub struct LongCommand {
    mark: &'static str,
    seconds: u64,
}

impl LongCommand {
    /// The command whose command line ends with `mark`, which runs for five minutes.
    pub const fn new(mark: &'static str) -> Self {
        Self::lasting(mark, 300)
    }

    /// The command whose command line ends with `mark`, which runs for `seconds` and
    /// ends by itself: for a check that lets a window that runs it again finish it.
    pub const fn lasting(mark: &'static str, seconds: u64) -> Self {
        Self { mark, seconds }
    }

    /// How long it runs.
    pub const fn seconds(&self) -> u64 {
        self.seconds
    }

    /// The command, as a member is told to run it: Node waits for its seconds, and the
    /// mark is a word it is given and does nothing with.
    pub fn text(&self) -> String {
        format!(
            "node -e \"setTimeout(() => {{}}, {})\" {}",
            self.seconds * 1_000,
            self.mark
        )
    }

    /// Its own processes, read across the machine and never ended: the `node` it runs,
    /// not the window whose launch text (the task, as an argument of Claude) names it too.
    pub fn running(&self) -> Result<Vec<Process>> {
        Ok(of_node(processes_with(self.mark)?))
    }

    /// Its processes in the window whose own process is `window` (found when the window
    /// opened): what descends from it that holds the mark, as `machine` lists the
    /// machine's ([`descending_from`]). A process outside the window that holds the mark
    /// is none of it.
    pub fn in_window(&self, machine: &dyn Machine, window: Identity) -> Result<Vec<Running>> {
        Ok(descending_from(&machine.processes()?, window)
            .into_iter()
            .filter(|process| process.command.contains(self.mark))
            .collect())
    }

    /// Its processes in the window whose own process is `window`, and what they started:
    /// what a check ends of it, now ([`LongCommand::end_in`]), or once the window is cut,
    /// which leaves what it ran with no window to descend from ([`end_chosen`]).
    pub fn chosen_in(&self, machine: &dyn Machine, window: Identity) -> Result<Vec<Running>> {
        Ok(holding(&machine.processes()?, window, self.mark))
    }

    /// Ends its processes in the window whose own process is `window`, and what they
    /// started, on this machine ([`LongCommand::chosen_in`], [`end_chosen`]): what came of
    /// each, nothing where the window runs no such command. Nothing outside the window is
    /// ended, whatever its command line says.
    pub fn end_in(&self, window: Identity) -> Result<Ended> {
        Ok(end_chosen(&self.chosen_in(&ThisMachine, window)?))
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

    /// A process `pid`, begun at `started`, started by `ppid`.
    fn running(pid: u32, started: u64, ppid: u32, command: &str) -> Running {
        Running {
            identity: Identity { pid, started },
            ppid,
            command: command.to_owned(),
        }
    }

    #[test]
    fn a_process_outside_the_window_that_holds_the_mark_runs_on_and_the_windows_own_command_is_ended(
    ) {
        use std::time::Duration;

        use super::super::descent::end_chosen_by;
        use super::super::descent::testing::Listed;

        // A check that ended every process whose command line held its mark ended the
        // worker that ran it, whose brief quoted the mark (2026-10-16). What is found and
        // ended is what descends from the window's own process and holds the mark.
        let mark = "cf-live-members-outsider-mark-5113";
        let command = LongCommand::new(mark);
        let outsider = if cfg!(windows) {
            crate::process::Run::new("cmd")
                .args(["/c", &format!("ping -n 30 127.0.0.1 > nul & rem {mark}")])
        } else {
            crate::process::Run::new("/bin/sh").args(["-c", &format!("sleep 30; : {mark}")])
        };
        let mut outsider = outsider.inheriting_env().spawn().unwrap();
        // The window's program (100), its agent (101, Devin's runs its commands), the
        // command the agent runs (102) and what that started (103); and outside the window
        // the process that says the mark too, and one that says nothing of it.
        let window = Identity {
            pid: 100,
            started: 10,
        };
        let machine = Listed::new(vec![
            running(100, 10, 1, "devin --config /c/config.json"),
            running(101, 11, 100, "devin --config /c/config.json"),
            running(102, 12, 101, &format!("node -e \"x\" {mark}")),
            running(103, 13, 102, "node worker"),
            running(
                outsider.id(),
                5,
                std::process::id(),
                &format!("sh -c sleep 30; : {mark}"),
            ),
            running(300, 3, 1, "editor"),
        ]);
        let pids =
            |found: &[Running]| -> Vec<u32> { found.iter().map(|one| one.identity.pid).collect() };
        assert_eq!(pids(&command.in_window(&machine, window).unwrap()), [102]);
        let chosen = command.chosen_in(&machine, window).unwrap();
        assert_eq!(pids(&end_chosen_by(&machine, &chosen).ended), [102, 103]);
        assert_eq!(machine.ended(), [102, 103]);
        // A window that runs no such command has nothing found or ended.
        let editor = Identity {
            pid: 300,
            started: 3,
        };
        assert!(command.in_window(&machine, editor).unwrap().is_empty());
        assert!(command.chosen_in(&machine, editor).unwrap().is_empty());
        assert_eq!(machine.ended().len(), 2);

        // And the process outside the window that says the same words runs on.
        assert!(
            outsider.wait(Duration::from_millis(500)).unwrap().is_none(),
            "the process outside the window was ended"
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
    fn what_a_cut_window_left_of_the_command_is_ended_by_the_processes_chosen_while_it_was_the_windows(
    ) {
        use std::time::Duration;

        use crate::live::until;
        use crate::process::Run;

        let process_of = |pid: u32| ThisMachine.identity(pid).ok().flatten();

        // A window that is cut leaves what it ran with no window to descend from: what the
        // window ran of the command is chosen while it is the window's, and ended after the
        // cut by which process it is. A process outside the window that says the mark runs
        // on. A shell stands in for the window, and one it runs for the command (two
        // commands in it, so that it stays the process that holds the mark).
        let mark = "cf-live-members-cut-mark-4171";
        let command = LongCommand::new(mark);
        let shell = |line: String| {
            Run::new("/bin/sh")
                .args(["-c", &line])
                .inheriting_env()
                .spawn()
                .unwrap()
        };
        let mut window = shell(format!("/bin/sh -c 'sleep 30; : {mark}' & wait"));
        let mut outsider = shell(format!("sleep 30; : {mark}"));
        let own = process_of(window.id()).expect("the window's process is read");
        let chosen = until(Duration::from_secs(10), Duration::from_millis(50), || {
            let found = command.chosen_in(&ThisMachine, own).ok()?;
            (found.len() == 2).then_some(found)
        })
        .expect("the window's command and its sleep are chosen");
        assert!(chosen
            .iter()
            .all(|one| one.identity.pid != outsider.id() && one.identity != own));

        window.kill();
        assert!(
            command.in_window(&ThisMachine, own).unwrap().is_empty(),
            "a window that is gone has no tree to find the command in"
        );
        assert_eq!(end_chosen(&chosen).ended, chosen);
        let gone = until(Duration::from_secs(10), Duration::from_millis(50), || {
            chosen
                .iter()
                .all(|one| process_of(one.identity.pid) != Some(one.identity))
                .then_some(())
        });
        assert!(gone.is_some(), "what the cut window left was ended");
        assert!(
            outsider.wait(Duration::from_millis(500)).unwrap().is_none(),
            "the process outside the window was ended"
        );
    }
}

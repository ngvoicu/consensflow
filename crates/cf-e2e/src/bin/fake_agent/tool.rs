//! The command a turn runs, as an agent's shell tool runs one. A message with a line
//! `RUN <program> <arguments…>` is a turn that starts the program, by a shell as a
//! harness starts each command its agent runs (the shell and the program are processes
//! of the window's tree, with the command line they are given, which the pane host
//! lists as work the turn started) and works until it ends, then answers `ran: <the
//! line>`. An Escape stops the turn, as it stops Claude's, and the program ends with
//! it: a harness that kills the tool it cancels. With `--survive` after the word the
//! program is left running: a harness that cancels its turn and lets go of the tool.

use std::ffi::OsString;
use std::path::Path;
use std::thread;
use std::time::Duration;

use cf_e2e::process::Spawned;

use crate::background::{in_a_shell, THINKING};
use crate::reply::{Reply, Window};

/// How often the program is looked at for its end while the turn works.
const LOOK: Duration = Duration::from_millis(200);

/// What the program of a turn is asked to be: its line, and whether it outlives the
/// stop of the turn.
#[derive(Debug, PartialEq, Eq)]
struct Command<'a> {
    program: &'a str,
    arguments: Vec<&'a str>,
    survives: bool,
}

impl<'a> Command<'a> {
    /// Reads the line `RUN [--survive] <program> <arguments…>` of `text`: none for a
    /// message with no such line, or whose line names no program.
    fn parse(text: &'a str) -> Option<Self> {
        let line = text.lines().find_map(|line| line.strip_prefix("RUN "))?;
        let (survives, line) = match line.strip_prefix("--survive ") {
            Some(line) => (true, line),
            None => (false, line),
        };
        let mut words = line.split_whitespace();
        let program = words.next().filter(|program| !program.starts_with("--"))?;
        Some(Self {
            program,
            arguments: words.collect(),
            survives,
        })
    }
}

/// The reply to `text` if it is a `RUN`: the turn that runs the program. None for
/// another message.
pub fn reply(text: &str, window: &Window) -> Option<Result<Reply, String>> {
    Command::parse(text).map(|command| run(&command, window))
}

/// The turn: the program is started once the agent has thought a moment (the daemon
/// marks where a launch's turn begins when its message shows in the record, up to a
/// pass after the agent began, and what the tree has before that is not the turn's),
/// and the turn works until the program ends or the turn is stopped.
fn run(command: &Command, window: &Window) -> Result<Reply, String> {
    thread::sleep(THINKING);
    // By a shell in a group of its own, as Claude Code runs a command: the window's
    // own group is its harness's and its servers'.
    let words = command.arguments.iter().map(OsString::from).collect();
    let mut program = in_a_shell(Path::new(command.program), words, false)?
        .inheriting_env()
        .closed_input()
        .spawn()
        .map_err(|failed| failed.to_string())?;
    while !program.has_exited() {
        if window.stop.work(LOOK) {
            if command.survives {
                leave_running(program);
            } else {
                end(&mut program);
            }
            return Ok(Reply::Stopped);
        }
    }
    Ok(Reply::Text(format!(
        "ran: {} {}",
        command.program,
        command.arguments.join(" ")
    )))
}

/// Ends the shell and the program it runs, as a harness that kills the tool it cancels
/// does: on Unix the group the shell leads, which holds both; on Windows the shell's
/// tree.
fn end(shell: &mut Spawned) {
    #[cfg(unix)]
    {
        use nix::sys::signal::{killpg, Signal};
        use nix::unistd::Pid;

        if let Ok(leader) = i32::try_from(shell.id()) {
            let _ = killpg(Pid::from_raw(leader), Signal::SIGKILL);
        }
    }
    #[cfg(windows)]
    {
        use cf_e2e::process::{own_var, Run};

        let root = own_var("SystemRoot").unwrap_or_else(|| r"C:\Windows".to_owned());
        let _ = Run::new(format!(r"{root}\System32\taskkill.exe"))
            .args(["/T", "/F", "/PID"])
            .arg(shell.id().to_string())
            .run();
    }
    shell.kill();
}

/// Lets the program run on past the turn: the handle ends the program it is dropped
/// with, so it is held on a thread of its own until the program is over.
fn leave_running(mut program: Spawned) {
    thread::spawn(move || {
        while !program.has_exited() {
            thread::sleep(LOOK);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_run_is_a_program_and_its_arguments_and_may_outlive_the_stop_of_its_turn() {
        assert_eq!(
            Command::parse("RUN sleep 119"),
            Some(Command {
                program: "sleep",
                arguments: vec!["119"],
                survives: false
            })
        );
        // The line is one of the message, which the daemon heads with its own.
        assert_eq!(
            Command::parse(
                "[ConsensFlow m-1 · T-1 · task from @chief]\nRUN --survive ping -n 61 127.0.0.1"
            ),
            Some(Command {
                program: "ping",
                arguments: vec!["-n", "61", "127.0.0.1"],
                survives: true
            })
        );
    }

    #[test]
    fn a_message_that_is_no_run_or_names_no_program_is_none() {
        for text in [
            "",
            "Reply with exactly: RUN",
            "RUN",
            "RUN ",
            "RUN --survive ",
            "RUN --loud sleep 1",
            "run sleep 1",
        ] {
            assert_eq!(Command::parse(text), None, "{text:?}");
        }
    }
}

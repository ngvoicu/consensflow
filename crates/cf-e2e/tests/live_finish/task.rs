//! What the worker is given to do: one short shell command, run with its own shell
//! tool, and a reply with a line that holds what the command printed. The command is
//! the machine's `hostname`:
//!
//! - It is a program, so that the worker's shell starts a process and the turn has a
//!   tool call to leave something behind it, which is what the check is about; and it
//!   is a short one, a few milliseconds, with no network, no repository and no file
//!   to read, so that nothing but the worker's own shell can make a turn long.
//! - It is found in the folders every window has on its `PATH` on both machines
//!   (`/bin` on macOS, `System32` on Windows). `git`, which both machines have, is on
//!   it on macOS (`/usr/bin`) and not on Windows: the `PATH` the rig gives a window
//!   there is the stand-in's folder, the harness's and Node's, and `System32`
//!   (`rig/install.rs`), and Git for Windows' folder is none of them, so `git
//!   --version` would be "not recognized" by a worker's shell on zeewin, and the check
//!   would fail for a reason that is no finding.
//! - It prints the machine's own name, which only a run of it tells (a worker that
//!   never ran the command cannot say it, and a pass of the check would prove nothing
//!   of a turn that left no process because it ran none). The check learns it by
//!   running the command itself, so that no text differs between the macOS machine
//!   and the Windows one.

use std::io;

use cf_e2e::live::command;
use cf_e2e::process::Run;
use cf_e2e::{Error, Result};

/// The command every worker is asked to run.
const COMMAND: &str = "hostname";

/// What the chief gives the worker, and what the worker's result must hold if it did it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    /// The brief of the task.
    pub brief: String,
    /// What the result must hold: the command's output, or for the stand-in the line it
    /// says of the command it ran.
    pub output: String,
}

impl Task {
    /// The task of a worker that is a real harness: run [`COMMAND`] with its own shell
    /// tool, and reply with one line that holds what it printed. The output it must
    /// hold is what this machine's `hostname` prints, the first line of it.
    pub fn real() -> Result<Self> {
        let program = command(COMMAND)?;
        let ran = Run::new(&program).inheriting_env().run()?;
        let name = ran
            .stdout
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty());
        match (ran.code, name) {
            (Some(0), Some(name)) => Ok(Self {
                brief: format!(
                    "Run the shell command \"{COMMAND}\" with your shell tool, and no other. When it has ended, reply with one line that holds what it printed, and nothing more."
                ),
                output: name.to_owned(),
            }),
            _ => Err(Error::Program {
                action: "run",
                program: COMMAND.to_owned(),
                source: io::Error::other(format!("it printed no name: {ran}")),
            }),
        }
    }

    /// The task of the rig's stand-in agent, which has no shell tool: `RUN` starts the
    /// program as a process of its window, as a harness's tool does, and the reply says
    /// the line it ran.
    pub fn stand_in() -> Self {
        Self {
            brief: format!("RUN {COMMAND}"),
            output: format!("ran: {COMMAND}"),
        }
    }

    /// The task of a stand-in that leaves work behind it, as a worker does that starts a
    /// job in the background and ends its turn: a job of `seconds` seconds that runs on
    /// past the turn, and the reply that says it was left.
    pub fn stand_in_leaving(seconds: u64) -> Self {
        Self {
            brief: format!("BACKGROUND {seconds} left a job running"),
            output: "left a job running".to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_real_worker_is_told_to_run_the_command_with_its_shell_tool_and_say_what_it_printed() {
        // (A machine with no `hostname` to run has no task to give.)
        let Ok(task) = Task::real() else {
            return;
        };
        assert!(
            task.brief
                .starts_with("Run the shell command \"hostname\" with your shell tool"),
            "{}",
            task.brief
        );
        assert!(task
            .brief
            .contains("reply with one line that holds what it printed"));
        assert!(!task.brief.contains('\n'), "a brief is one line");
        // What the result must hold is what the machine's own `hostname` printed.
        assert!(!task.output.is_empty() && !task.output.contains('\n'));
        let again = Run::new(command(COMMAND).unwrap())
            .inheriting_env()
            .run()
            .unwrap();
        assert_eq!(again.stdout.trim(), task.output);
    }

    #[test]
    fn the_stand_in_is_told_to_run_the_program_and_says_which_it_ran() {
        let task = Task::stand_in();
        assert_eq!(task.brief, "RUN hostname");
        assert_eq!(task.output, "ran: hostname");
    }

    #[test]
    fn the_stand_in_that_leaves_work_is_told_how_long_the_job_runs_and_says_it_left_it() {
        let task = Task::stand_in_leaving(9);
        assert_eq!(task.brief, "BACKGROUND 9 left a job running");
        assert_eq!(task.output, "left a job running");
    }
}

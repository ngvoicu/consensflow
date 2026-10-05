//! The one place a ConsensFlow program starts another process: how a
//! program starts here (Windows scripts and npm's shims included) and how a
//! window's program does, where a command is on PATH, running one in this
//! process's place, to its end (`execFile`) or beside this one a line at a
//! time (`spawn`), ending one, and whether one is alive.

#![deny(unsafe_code)]
#![deny(clippy::undocumented_unsafe_blocks)]

mod alive;
mod child;
mod execute;
mod runnable;
mod search;
mod terminate;

pub use alive::alive;
pub use child::{spawn, Child, Streams};
pub use execute::{execute, Failed, Limits};
pub use runnable::{pane_argv, runnable, Run};
pub use search::{find_in, on_path};
pub use terminate::{terminate, Ending};

use std::ffi::{OsStr, OsString};
use std::io;
use std::process::Command;

/// Runs `program` with `args` in this process's place, its standard
/// streams, working folder and environment inherited: on Unix this process
/// becomes it, so its exit code and the signals it gets are its own; Windows
/// has no such call, so there it runs as a child whose exit code this
/// process then exits with. It returns only when the program could not be
/// started, with the reason.
#[allow(clippy::disallowed_methods)] // The one place a process starts.
pub fn run_in_place(program: &OsStr, args: &[OsString]) -> io::Error {
    let mut command = Command::new(program);
    command.args(args);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.exec()
    }
    #[cfg(not(unix))]
    match command.status() {
        Ok(status) => std::process::exit(status.code().unwrap_or(1)),
        Err(cause) => cause,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_program_that_is_not_there_is_an_error_not_an_exit() {
        let missing = OsStr::new("/nonexistent/consensflow-test-program");
        assert_eq!(run_in_place(missing, &[]).kind(), io::ErrorKind::NotFound);
    }
}

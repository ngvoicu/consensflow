//! `cargo xtask windows` and `cargo xtask windows-matrix`: the Windows machine
//! this checkout is tried on (`zeewin`, over ssh), and the matrix of the chief
//! evals run there.
//!
//! `windows` runs a command on a Windows machine, over SSH, on this working tree
//! as it is now: every tracked file, and new ones not ignored, go over as one
//! tarball and are unpacked into a build folder there, each with the time of the
//! unpack (every other file there goes, wherever it came from, but for the build's
//! outputs, `node_modules` and the Rust build among them: `script::KEPT`), so that
//! cargo builds what a switch of
//! the checkout changed, whatever time the Mac kept of it. The command then runs
//! in that folder with Node and Cargo on PATH, its output streamed back; its exit
//! status is this one's, as the machine says it in the last line of the output
//! (below). One run at a time: a second is refused
//! while the first goes on there (the machine says "A run is still going on this
//! machine (process N): one at a time." and ends with 75, which is the answer),
//! and a run stopped here (Ctrl-C, or a `kill`) is ended there too, which
//! Windows' SSH server does not do (the status is then 130).
//!
//! The status is said by the script, as the last line it writes (`== exit N`),
//! and not taken from ssh, because what ssh answers is the machine's shell's.
//! Measured on zeewin, whose SSH server's default shell is PowerShell
//! (`HKLM\SOFTWARE\OpenSSH\DefaultShell` is
//! `C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe`):
//!
//! - `ssh zeewin "cmd /c exit 7"` ends with 1, with no runner involved, and so
//!   does `ssh zeewin "powershell -NoProfile -NonInteractive -Command exit 75"`;
//! - a script sent as this one is, which wrote `last=7` and ended with `exit
//!   $LASTEXITCODE`, ended ssh with 1, and the lock's own `exit 75` did too;
//! - `ssh zeewin 'powershell -NoProfile -NonInteractive -Command "exit 7"; exit
//!   $LASTEXITCODE'` ended with 7: the outer PowerShell answers 1 for any
//!   command whose last program failed, and keeps the status only if it is
//!   told to.
//!
//! A machine whose SSH server hands the command to `cmd.exe` would pass the
//! status through; the line works wherever. So the run answers the status of the
//! line, whatever ssh answered. Where the output does not end with that line (a
//! connection lost, a machine gone) there is only ssh's own status, which is the
//! answer if it is a failure and never 0 (a run that has not said it ended well
//! did not end well), or 1 if ssh has none (a signal ended it). A stop is 130.
//! A status that no program ends with (above 255, or below 0 as Windows' are:
//! -1073741819) is answered as 1, for a system that keeps a byte of one would
//! make 256 a success; the line in the output says what it was. Each answer
//! that is not a plain one said by the machine is said on the error stream.
//!
//! ```text
//! cargo xtask windows --host <ssh host> [--build] -- cargo xtask live paste
//! cargo xtask windows --host <ssh host> -- cargo xtask eval --scenario round-trip --chief devin --staff devin
//! cargo xtask windows --host <ssh host> --in app/src-tauri -- cargo test
//! ```
//!
//! `--build` installs the packages and builds the page, a pane's `cf.exe` and the
//! pane host first: on a fresh machine, and after any change to the Rust. `--in`
//! runs the command in a folder of the copy. `--env NAME=VALUE` gives the command
//! that variable, as many as are named and no others: this machine's environment
//! does not go, and the machine's own is the command's as it is. The output says
//! each (`== env NAME=VALUE`) before the command's own. The black-box suites of
//! `crates/cf-e2e` build the `cf` and the pane host they run, and need none:
//!
//! ```text
//! cargo xtask windows --host <ssh host> -- cargo xtask test integration
//! cargo xtask windows --host <ssh host> --env CONSENSFLOW_TEST_DAEMON=native -- cargo xtask eval ...
//! ```
//!
//! The machine needs an OpenSSH server, Node, Rust, git and Windows' own
//! `tar.exe`; the folder is `%USERPROFILE%\consensflow-build` unless `--dir`
//! names another under it. Run one at a time: a live run's windows are the
//! machine's.
//!
//! `windows-matrix` is the evals there: each `--scenario` for each `--pair
//! chief:staff`, one run after another through the same run, with `--env` given
//! to each, the output of each kept in `evals/reports/windows/` and one line per
//! run at the end.
//!
//! The modules: [`options`] (the words), [`script`] (the PowerShell that goes, as
//! text, and the line it ends by), [`tree`] (the tarball), [`session`] (ssh, as it
//! goes, and the answer), [`tail`] (the last line of what it said), [`interrupt`]
//! (a stop), [`run`] (one run) and [`matrix`].

mod interrupt;
mod matrix;
mod options;
mod run;
mod script;
mod session;
mod tail;
#[cfg(test)]
mod testing;
mod tree;

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

use cf_base::time::SystemClock;

use crate::context::Context;
use crate::dispatch::{Command, Console, Failure, Run};
use crate::process::{self, Invocation};
use interrupt::Interrupt;
use matrix::Machine;
use options::{MatrixOptions, Options};
use run::Outcome;

pub const COMMANDS: &[Command] = &[
    Command {
        words: &["windows"],
        about: "Run a command on the Windows machine this checkout is tried on, over ssh",
        usage: "--host HOST [--build] [--dir DIR] [--in DIR] [--env NAME=VALUE ...] -- <command ...>",
        run: Run::Native(windows),
    },
    Command {
        words: &["windows-matrix"],
        about: "Run the chief evals on the Windows machine, a pair of chief and staff at a time",
        usage: "--host HOST [--build] [--env NAME=VALUE ...] --scenario NAME ... --pair CHIEF:STAFF ... [--claude-model M] [--pi-model M]",
        run: Run::Native(windows_matrix),
    },
];

/// The status of a run that was stopped, as a program ends that Ctrl-C stops.
const STOPPED: i32 = 130;

/// Why a run on the machine did not happen: what, in words, as the command says it.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// What was asked cannot be sent.
    #[error(transparent)]
    Refused(#[from] script::Refusal),
    /// A program could not be started.
    #[error(transparent)]
    Process(#[from] process::Failure),
    /// A step of the run that must end with 0 did not.
    #[error("`{step}` ended with status {status}{said}")]
    Step {
        step: String,
        status: i32,
        said: String,
    },
    #[error("could not {what} {}: {cause}", path.display())]
    Files {
        what: &'static str,
        path: PathBuf,
        cause: io::Error,
    },
    #[error("could not make a folder to pack the tree in: {0}")]
    Scratch(io::Error),
    /// A file of the tree has a name that cannot be sent as text.
    #[error("a file of the tree has a name that is not text, and none is sent: {0}")]
    Name(String),
    /// What the run says could not be written (the run was ended on the machine).
    #[error("could not write what the run says: {0}")]
    Output(io::Error),
    #[error("could not wait for ssh: {0}")]
    Wait(io::Error),
}

impl Error {
    /// The step `invocation` ended with `status` and said `stderr`.
    fn step(invocation: &Invocation, status: i32, stderr: &str) -> Self {
        let said = stderr.trim();
        Self::Step {
            step: invocation.display(),
            status,
            said: if said.is_empty() {
                String::new()
            } else {
                format!(": {said}")
            },
        }
    }

    /// How a failed operation on `path` is told: what was being done.
    fn files(what: &'static str, path: &Path) -> impl FnOnce(io::Error) -> Self {
        let path = path.to_path_buf();
        move |cause| Self::Files { what, path, cause }
    }
}

fn windows(context: &Context, args: &[OsString], console: &mut Console) -> Result<i32, Failure> {
    let options = Options::parse(args).map_err(Failure::Usage)?;
    run::check(&options).map_err(|refusal| Failure::Usage(refusal.to_string()))?;
    let interrupt = Interrupt::listen()?;
    match run::run(context, &options, console, &interrupt) {
        Ok(Outcome::Exited(status)) => Ok(status),
        Ok(Outcome::Stopped) => Ok(STOPPED),
        Err(error) => {
            writeln!(console.err, "windows: {error}")?;
            Ok(1)
        }
    }
}

fn windows_matrix(
    context: &Context,
    args: &[OsString],
    console: &mut Console,
) -> Result<i32, Failure> {
    let options = MatrixOptions::parse(args).map_err(Failure::Usage)?;
    let interrupt = Interrupt::listen()?;
    let mut machine = Machine {
        context,
        interrupt: &interrupt,
    };
    match matrix::run(context, &options, &mut machine, &mut SystemClock, console) {
        Ok(status) => Ok(status),
        Err(error) => {
            writeln!(console.err, "windows-matrix: {error}")?;
            Ok(1)
        }
    }
}

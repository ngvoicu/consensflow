//! What the app's bundle carries (landing S1): the native `cf` (crates/cf)
//! built and put in `bin/`, that `cf` staged as the bundle's `cli/bin/cf` with,
//! on Windows, Microsoft's console host beside it, and the console host fetched
//! on its own.
//!
//! - `build-cf` ([`build_cf`]) builds `cf` and replaces the copy in `bin/`,
//!   which is the checkout's, the test suites' and, through `stage`, the
//!   bundle's.
//! - `stage` ([`stage`]) does that, then puts the copy and the console host
//!   where Tauri bundles them from, `app/src-tauri/resources`.
//! - `conpty` ([`conpty`]) fetches the console host's package, checks it against
//!   the SHA-256 it is pinned to and unzips its two files.
//!
//! What a step reaches outside the files it works on, it reaches through
//! [`System`]: the programs it runs, which system this is, the time of day, and
//! the deleting of a file. [`Machine`] is this machine. The tests' is a stand-in
//! that answers as a test says, so that what only Windows does (refuse to delete
//! a `cf.exe` that runs), or only macOS (signing), or only the network (a
//! package fetched), runs on any system and reaches none.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use cf_base::env::Env;
use cf_base::time::{Clock, SystemClock};

use crate::dispatch::{Command, Console, Failure, Run};
use crate::process::{self, Invocation};

mod build_cf;
mod conpty;
mod stage;
#[cfg(test)]
mod testing;

pub const COMMANDS: &[Command] = &[
    Command {
        words: &["build-cf"],
        about: "Build the native cf and put it in bin/ (signed ad hoc on macOS)",
        usage: "[--offline]",
        run: Run::Native(build_cf::run),
    },
    Command {
        words: &["stage"],
        about:
            "Build cf and stage it, and on Windows the console host, as the app bundle's resources",
        usage: "",
        run: Run::Native(stage::run),
    },
    Command {
        words: &["conpty"],
        about: "Fetch Microsoft's console host, check its SHA-256, and put its two files in DIR",
        usage: "--into DIR",
        run: Run::Native(conpty::run),
    },
];

/// The system the steps run on, as far as they differ by it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Platform {
    Windows,
    MacOs,
    Other,
}

impl Platform {
    /// The system this xtask was built for.
    const HOST: Self = if cfg!(windows) {
        Self::Windows
    } else if cfg!(target_os = "macos") {
        Self::MacOs
    } else {
        Self::Other
    };

    /// The file the built `cf` is: a program has `.exe` on Windows.
    fn cf(self) -> &'static str {
        match self {
            Self::Windows => "cf.exe",
            Self::MacOs | Self::Other => "cf",
        }
    }
}

/// What a step reaches outside the files it works on; see the module.
pub(crate) trait System {
    fn platform(&self) -> Platform;
    /// Runs `invocation` to its end, with the terminal as its input and output:
    /// the code it exited with.
    fn run(&mut self, invocation: &Invocation) -> Result<i32, process::Failure>;
    /// The time of day, in milliseconds since the epoch.
    fn now_ms(&mut self) -> i64;
    /// Deletes the file at `path`. Windows refuses when a program runs from it.
    fn remove_file(&mut self, path: &Path) -> io::Result<()>;
}

/// This machine, with the environment xtask was started with.
struct Machine<'a> {
    env: &'a Env,
}

impl System for Machine<'_> {
    fn platform(&self) -> Platform {
        Platform::HOST
    }

    fn run(&mut self, invocation: &Invocation) -> Result<i32, process::Failure> {
        process::run(invocation, self.env)
    }

    fn now_ms(&mut self) -> i64 {
        SystemClock.now_ms()
    }

    fn remove_file(&mut self, path: &Path) -> io::Result<()> {
        fs::remove_file(path)
    }
}

/// Why a step could not be done.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A program it had to run could not be started.
    #[error(transparent)]
    Process(#[from] process::Failure),
    /// A program ended with a status other than 0, and said why itself.
    #[error("{command} ended with status {status}")]
    Ended { command: String, status: i32 },
    /// A file or a folder could not be made, read, moved, copied or removed.
    #[error("could not {what} {}: {cause}", path.display())]
    Files {
        what: &'static str,
        path: PathBuf,
        cause: io::Error,
    },
    /// What it had to say could not be written.
    #[error(transparent)]
    Said(#[from] io::Error),
    /// The build said it was done and left no `cf` to put anywhere.
    #[error("the build left no cf at {}", path.display())]
    NotBuilt { path: PathBuf },
    /// The package is not the one Microsoft published, whose SHA-256 is pinned.
    #[error(
        "{} is not the package Microsoft published (SHA-256 {actual}, not {pinned}): deleted it; run again",
        archive.display()
    )]
    NotThePackage {
        archive: PathBuf,
        actual: String,
        pinned: String,
    },
    /// The package is not a zip, or a file in it cannot be read.
    #[error("could not read {} as a zip: {cause}", archive.display())]
    Unzip { archive: PathBuf, cause: String },
    /// The package has no file at the place the app takes one from.
    #[error("{} has no {inside}", archive.display())]
    NotInThePackage {
        archive: PathBuf,
        inside: &'static str,
    },
}

/// How a failed file operation is told: what was being done, and to which path.
fn files(what: &'static str, path: &Path) -> impl FnOnce(io::Error) -> Error {
    let path = path.to_path_buf();
    move |cause| Error::Files { what, path, cause }
}

/// Runs `invocation` and refuses a status other than 0.
fn ran(system: &mut dyn System, invocation: &Invocation) -> Result<(), Error> {
    match system.run(invocation)? {
        0 => Ok(()),
        status => Err(Error::Ended {
            command: invocation.display(),
            status,
        }),
    }
}

/// What a command answers for how its work came out: 0, or the status of the
/// program that failed after it said so. Any other failure is for the caller to
/// say, with the status 1; what could not be said is its input/output failure,
/// which is how the caller knows a closed pipe (`cargo xtask build-cf | head -0`).
fn finish(result: Result<(), Error>, console: &mut Console) -> Result<i32, Failure> {
    match result {
        Ok(()) => Ok(0),
        Err(error @ Error::Ended { status, .. }) => {
            writeln!(console.err, "xtask: {error}")?;
            Ok(status)
        }
        Err(Error::Said(cause)) => Err(Failure::Io(cause)),
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use cf_base::env::Env;

    use crate::sidecar::testing::{checkout, Fake};

    #[test]
    fn the_cf_is_a_program_of_the_system_it_is_built_on() {
        assert_eq!(Platform::Windows.cf(), "cf.exe");
        assert_eq!(Platform::MacOs.cf(), "cf");
        assert_eq!(Platform::Other.cf(), "cf");
    }

    #[test]
    fn the_host_is_the_system_xtask_was_built_for() {
        let expected = if cfg!(windows) {
            Platform::Windows
        } else if cfg!(target_os = "macos") {
            Platform::MacOs
        } else {
            Platform::Other
        };
        assert_eq!(
            Machine {
                env: &Env::default()
            }
            .platform(),
            expected
        );
    }

    fn console<'a>(out: &'a mut Vec<u8>, err: &'a mut Vec<u8>) -> Console<'a> {
        Console { out, err }
    }

    #[test]
    fn a_step_that_is_done_answers_0_and_says_nothing() {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert_eq!(finish(Ok(()), &mut console(&mut out, &mut err)).unwrap(), 0);
        assert!(out.is_empty() && err.is_empty());
    }

    #[test]
    fn a_program_that_failed_has_its_status_answered_and_the_command_named() {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let ended = Error::Ended {
            command: "cargo build".into(),
            status: 101,
        };
        let status = finish(Err(ended), &mut console(&mut out, &mut err)).unwrap();
        assert_eq!(status, 101);
        assert_eq!(
            String::from_utf8(err).unwrap(),
            "xtask: cargo build ended with status 101\n"
        );
        assert!(out.is_empty());
    }

    #[test]
    fn any_other_failure_is_left_to_the_caller_to_say() {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let refused = Error::NotBuilt {
            path: PathBuf::from("target/cf"),
        };
        let failure = finish(Err(refused), &mut console(&mut out, &mut err)).unwrap_err();
        assert_eq!(failure.to_string(), "the build left no cf at target/cf");
        assert!(matches!(failure, Failure::Sidecar(_)));
        assert!(out.is_empty() && err.is_empty());
    }

    #[test]
    fn what_could_not_be_said_is_an_input_output_failure_so_a_closed_pipe_ends_quietly() {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let closed = Error::Said(io::Error::from(io::ErrorKind::BrokenPipe));

        let failure = finish(Err(closed), &mut console(&mut out, &mut err)).unwrap_err();

        // The one `dispatch::run` ends quietly, as it ends `--help | head -0`.
        assert!(
            matches!(&failure, Failure::Io(cause) if cause.kind() == io::ErrorKind::BrokenPipe),
            "{failure:?}"
        );
        assert!(out.is_empty() && err.is_empty());
    }

    #[test]
    fn a_program_run_is_one_that_ended_with_0() {
        let (_dir, context) = checkout();
        let invocation = Invocation::new("cargo", &context.root).arg("build");
        let mut system = Fake::on(Platform::Other);
        ran(&mut system, &invocation).unwrap();

        system.cargo_status = 101;
        let refused = ran(&mut system, &invocation).unwrap_err();
        assert_eq!(refused.to_string(), "cargo build ended with status 101");

        system.cargo_status = 0;
        system.missing = vec!["cargo".into()];
        let refused = ran(&mut system, &invocation).unwrap_err();
        assert_eq!(
            refused.to_string(),
            "`cargo` was not found: is it installed, and on the PATH?"
        );
    }

    #[test]
    fn a_failed_file_operation_says_what_was_done_and_to_which_path() {
        let cause = io::Error::from(io::ErrorKind::PermissionDenied);
        let said = files("copy the cf to", Path::new("bin/cf"))(cause).to_string();
        assert_eq!(said, "could not copy the cf to bin/cf: permission denied");
    }
}

//! The machine as `sign-mac` meets it, and what the steps meet it through.
//!
//! [`Runner`] is everything a run takes from outside: the programs it runs, the
//! time it waits and the bytes nobody can guess. The release hands it
//! [`System`]; a test hands it a script in place of Apple's tools, so that the
//! order of the calls, what is cleaned up when one fails and what is kept
//! secret are all tested without them.
//!
//! [`Tools`] is what the steps hold: a runner, the secrets of the run and the
//! console. It is the one way a step runs a program and the one way it speaks,
//! and a line spoken here has the secrets blanked out of it.

use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::{self, Read};
use std::thread;
use std::time::Duration;

use cf_base::env::Env;

use super::secrets::Secrets;
use crate::cli::{Console, Failure};
use crate::process::{self, Output, Unstarted};

/// `["a", path, "b"]` as the arguments a program is run with.
macro_rules! args {
    ($($arg:expr),* $(,)?) => {
        vec![$(::std::ffi::OsString::from($arg)),*]
    };
}
pub(super) use args;

/// What a run takes from the machine it is on.
pub(super) trait Runner {
    /// Runs `program` with `args` to its end and answers what it left.
    fn capture(&self, program: &str, args: &[OsString]) -> Result<Output, Unstarted>;

    /// Lets `time` pass.
    fn wait(&self, time: Duration);

    /// Fills `bytes` with ones nobody can guess.
    fn random(&self, bytes: &mut [u8]) -> io::Result<()>;
}

/// The machine itself: programs run with the environment `main` read, and the
/// system's own source of bytes (a Mac's, which is what signs).
pub(super) struct System<'a> {
    pub env: &'a Env,
}

impl Runner for System<'_> {
    fn capture(&self, program: &str, args: &[OsString]) -> Result<Output, Unstarted> {
        process::capture(OsStr::new(program), args, self.env)
    }

    fn wait(&self, time: Duration) {
        thread::sleep(time);
    }

    fn random(&self, bytes: &mut [u8]) -> io::Result<()> {
        File::open("/dev/urandom")?.read_exact(bytes)
    }
}

/// A step that could not finish, in `message`'s words.
pub(super) fn fail(message: impl Into<String>) -> Failure {
    Failure::Failed(message.into())
}

/// What the steps of a run run their programs through, and speak by.
pub(super) struct Tools<'a, 'c> {
    runner: &'a dyn Runner,
    secrets: &'a Secrets,
    console: &'a mut Console<'c>,
}

impl<'a, 'c> Tools<'a, 'c> {
    pub fn new(runner: &'a dyn Runner, secrets: &'a Secrets, console: &'a mut Console<'c>) -> Self {
        Self {
            runner,
            secrets,
            console,
        }
    }

    /// Runs `program` and answers what it wrote. One that ends with a code is a
    /// failure that names it and its first argument, a subcommand, and says what
    /// it said: never the other arguments, one of which may be a password.
    pub fn run(&self, program: &str, args: impl AsRef<[OsString]>) -> Result<String, Failure> {
        let args = args.as_ref();
        let output = self.capture(program, args)?;
        if output.code == 0 {
            return Ok(output.stdout);
        }
        Err(fail(refusal(program, args, &output)))
    }

    /// Runs `program` and keeps what it left, whatever its code: only one that
    /// cannot be started is a failure here.
    pub fn capture(&self, program: &str, args: impl AsRef<[OsString]>) -> Result<Output, Failure> {
        self.runner
            .capture(program, args.as_ref())
            .map_err(|cause| fail(cause.to_string()))
    }

    /// Runs `program` as a last step that must be tried and cannot be helped when
    /// it fails: a failure is told, never raised, so that it does not hide the one
    /// that brought the run here.
    pub fn attempt(&mut self, program: &str, args: impl AsRef<[OsString]>) {
        let args = args.as_ref();
        let told = match self.capture(program, args) {
            Ok(output) if output.code == 0 => return,
            Ok(output) => refusal(program, args, &output),
            Err(failure) => failure.to_string(),
        };
        self.say(&told);
    }

    /// Says a line on the error stream, as progress is. A closed pipe does not
    /// stop a run that has signed half of what it signs.
    pub fn say(&mut self, line: &str) {
        let _ = writeln!(self.console.err, "sign-mac: {}", self.secrets.redact(line));
    }

    /// Says what a program wrote, as it wrote it, secrets blanked out.
    pub fn echo(&mut self, text: &str) {
        let _ = writeln!(self.console.err, "{}", self.secrets.redact(text));
    }

    /// Lets `time` pass.
    pub fn wait(&self, time: Duration) {
        self.runner.wait(time);
    }
}

/// How a program that ended with a code is told: by its name and its first
/// argument and what it said, or its code when it said nothing.
fn refusal(program: &str, args: &[OsString], output: &Output) -> String {
    let first = args
        .first()
        .map(|word| format!(" {}", word.to_string_lossy()))
        .unwrap_or_default();
    let (stderr, stdout) = (output.stderr.trim(), output.stdout.trim());
    let said = [stderr, stdout]
        .into_iter()
        .find(|text| !text.is_empty())
        .map_or_else(|| format!("exited with {}", output.code), str::to_string);
    format!("{program}{first} failed: {said}")
}

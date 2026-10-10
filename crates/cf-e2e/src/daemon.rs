//! The daemon as a process: `cf ui --json --no-open`, as the app starts it,
//! on a home of a case's own. It says a handle line first (where it listens
//! and the token that opens its screens), then speaks the bridge's frames on
//! its standard streams until its input ends, and writes its log, `daemon.log`,
//! in its home.
//!
//! [`Daemon`] is the program and what is done to it; [`Home`] is the place it
//! runs in, with the turn it takes to run there ([`crate::serial`]).

use std::ffi::OsStr;
use std::io::BufReader;
use std::ops::Deref;
use std::process::ChildStdout;
use std::process::ExitStatus;
use std::sync::MutexGuard;
use std::time::Duration;

use serde_json::Value;

use crate::process::{Run, Signal, Sink, Spawned};
use crate::wire::{self, First};
use crate::{cf, checkout, files, serial, Error, Result, ScratchHome};

/// How long a daemon has to say it is ready.
pub const READY: Duration = Duration::from_secs(10);

/// How long a daemon is given to end when its input ends, before it is ended.
pub const STOP: Duration = Duration::from_secs(5);

/// A home for a daemon to run on: the folders of a [`ScratchHome`] and the
/// turn to run in. It goes, with everything in it, when it is dropped.
#[derive(Debug)]
pub struct Home {
    scratch: ScratchHome,
    _turn: MutexGuard<'static, ()>,
}

impl Home {
    /// A home, once it is its case's turn to run a daemon.
    pub fn new() -> Result<Self> {
        let turn = serial::turn();
        Ok(Self {
            scratch: ScratchHome::new()?,
            _turn: turn,
        })
    }

    /// The folder a project's work is in.
    pub fn workspace(&self) -> std::path::PathBuf {
        self.scratch.root().join("workspace")
    }

    /// The daemon's log.
    pub fn log(&self) -> String {
        files::read_string(&self.scratch.consensflow().join("daemon.log")).unwrap_or_default()
    }

    /// The environment a daemon runs in: the home's, and on Windows the user
    /// profile, which is where a program there looks for its home.
    pub fn vars(&self) -> Vec<(&'static str, std::path::PathBuf)> {
        let mut vars = self.scratch.vars();
        if cfg!(windows) {
            vars.push(("USERPROFILE", self.scratch.root().join("home")));
        }
        vars
    }

    /// The daemon on this home.
    pub fn daemon(&self) -> Result<Daemon> {
        Daemon::start(self.vars())
    }
}

impl Deref for Home {
    type Target = ScratchHome;

    fn deref(&self) -> &ScratchHome {
        &self.scratch
    }
}

/// A daemon that is running.
#[derive(Debug)]
pub struct Daemon {
    process: Spawned,
    output: Option<BufReader<ChildStdout>>,
}

impl Daemon {
    /// Starts `cf ui --json --no-open` with the variables `vars` and no others
    /// (on Windows the few the system needs to start a program, and to find
    /// one), in the checkout. Its handle line is not read yet.
    pub fn start<I, K, V>(vars: I) -> Result<Self>
    where
        I: IntoIterator<Item = (K, V)>,
        K: AsRef<OsStr>,
        V: AsRef<OsStr>,
    {
        Self::run(cf::binary()?, ["ui", "--json", "--no-open"], vars)
    }

    /// Starts `program` with `args` where the daemon is, in the same way: for
    /// the `cf` under test ([`Daemon::start`]), and for the stand-in that a case
    /// holds the rig's refusal of a daemon that is not the native one to.
    pub fn run<I, K, V, A, S>(program: impl AsRef<OsStr>, args: A, vars: I) -> Result<Self>
    where
        I: IntoIterator<Item = (K, V)>,
        K: AsRef<OsStr>,
        V: AsRef<OsStr>,
        A: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut process = Run::new(program)
            .args(args)
            .vars(vars)
            .finding_programs()
            .cwd(checkout::root())
            .spawn()?;
        let output = process.take_output();
        Ok(Self { process, output })
    }

    /// Its process id.
    pub fn id(&self) -> u32 {
        self.process.id()
    }

    /// Everything it has said on its error output so far.
    pub fn errors(&self) -> String {
        self.process.errors()
    }

    /// The handle line it says when it is ready, read as JSON: `url`, where it
    /// listens, and `token`. It is waited for up to `within`; a daemon that
    /// ends first, or says none in time, is an error that says what it wrote
    /// to its error output.
    pub fn handle(&mut self, within: Duration) -> Result<Value> {
        let Some(output) = self.output.take() else {
            return Err(Error::Daemon("the handle line is read once".to_owned()));
        };
        let (first, rest) = wire::first_line(output, within);
        self.output = rest;
        match first {
            First::Line(line) => serde_json::from_str(&line).map_err(|source| {
                Error::Daemon(format!(
                    "the daemon's first line is no handle ({source}): {line}"
                ))
            }),
            First::Ended => Err(Error::Daemon(format!(
                "the daemon ended before it said it was ready: {}",
                self.errors()
            ))),
            First::TimedOut => Err(Error::Daemon(format!(
                "the daemon never said it was ready: {}",
                self.errors()
            ))),
        }
    }

    /// Calls `on_line` with each line the daemon goes on to say, on a thread of
    /// its own, and `done` once its output is over, or `on_line` has said to
    /// leave (by answering false), which closes the daemon's output: its next
    /// write finds nobody there. The lines after the handle are frames; they are
    /// read from here on or not at all.
    pub fn read_each(
        &mut self,
        on_line: impl FnMut(String) -> bool + Send + 'static,
        done: impl FnOnce() + Send + 'static,
    ) {
        if let Some(output) = self.output.take() {
            wire::read_each(output, on_line, done);
        }
    }

    /// Where its input is written from.
    pub fn sink(&self) -> Option<Sink> {
        self.process.sink()
    }

    /// Queues `line` (which ends with its newline) for its input.
    pub fn send(&self, line: &str) {
        self.process.send(line);
    }

    /// Ends its input once what is queued is written: the app's way to ask it
    /// to stop.
    pub fn end_input(&self) {
        self.process.end_input();
    }

    /// Sends it SIGTERM, with its input left open. Windows has none to send.
    pub fn terminate(&self) {
        self.process.signal(Signal::Terminate);
    }

    /// Sends it SIGKILL.
    pub fn kill(&mut self) {
        self.process.kill();
    }

    /// Whether it has ended.
    pub fn has_exited(&mut self) -> bool {
        self.process.has_exited()
    }

    /// Waits up to `within` for it to end: how it ended, or none if it did not.
    pub fn wait(&mut self, within: Duration) -> Result<Option<ExitStatus>> {
        self.process.wait(within)
    }

    /// Waits up to `within` for it to end: the code it exited with, or none if
    /// it did not end (or was ended by a signal, which has no code).
    pub fn exit_code(&mut self, within: Duration) -> Result<Option<i32>> {
        Ok(self.wait(within)?.and_then(|status| status.code()))
    }

    /// Ends it as the app ends it, by its input, and waits [`STOP`] for it to
    /// go; a daemon that stays is killed, so that a case never leaves one
    /// running.
    pub fn stop(&mut self) -> Result<()> {
        self.end_input();
        if self.wait(STOP)?.is_none() {
            self.kill();
        }
        Ok(())
    }
}

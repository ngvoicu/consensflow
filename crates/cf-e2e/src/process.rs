//! The one place cf-e2e starts a program. `clippy.toml` lets only cf-process do
//! that, and a black-box suite depends on no crate of the product, cf-process
//! included: it starts programs with std, runs each to its end (or to the limit
//! it is given) or leaves it running for the case to drive ([`Spawned`]), and
//! keeps what it printed, what it said and the code it exited with.
//!
//! A program gets the environment a case names and no other: the case's home
//! points `cf` at its own folders, and nothing of the machine's `~/.consensflow`
//! or of a window this test may itself run in reaches it. A case that needs the
//! test's own environment under its variables asks for it
//! ([`Run::inheriting_env`]). The program's input is closed, unless the case
//! gives it some ([`Run::input`]) or drives it ([`Run::spawn`]).
//!
//! On Windows a program is also given the few variables the system needs to
//! start it, where the case names none, from the test's own environment: the
//! JavaScript suites started `cf` with Node, whose libuv does that for every
//! child, and winsock does not start without `SYSTEMROOT`, for one. A program
//! that starts other programs by their names asks for the two more that find
//! them ([`Run::finding_programs`]). This module reads the test's environment
//! for that, and for the few variables a stand-in program or a suite is told
//! its settings by ([`own_var`]).
//!
//! A program that leaves a child of its own holding its output after it is
//! ended holds the run too; the verbs of `cf` start none.
// The one place a program starts and the test's environment is read; see above.
#![allow(clippy::disallowed_methods)]

mod pid;
mod spawned;
#[cfg(test)]
mod tests;

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::{Error, Result};

pub use pid::{is_alive, signal, Signal};
pub use spawned::{Sink, Spawned};

/// How long a program may run before it is ended. A verb of `cf` answers at
/// once, and one that waited for ever would hold the whole suite with it.
pub const LIMIT: Duration = Duration::from_secs(30);

/// How often a program that has not ended is looked at again.
const POLL: Duration = Duration::from_millis(2);

/// The variables a program on Windows is given from the test's own environment
/// where the case names none (libuv's `required_vars`, as cf-process has them).
const REQUIRED_ON_WINDOWS: [&str; 11] = [
    "HOMEDRIVE",
    "HOMEPATH",
    "LOGONSERVER",
    "PATH",
    "SYSTEMDRIVE",
    "SYSTEMROOT",
    "TEMP",
    "USERDOMAIN",
    "USERNAME",
    "USERPROFILE",
    "WINDIR",
];

/// What a program that starts others by their names needs besides, on Windows:
/// the command interpreter, and which extensions make a file a program.
const PROGRAM_LOOKUP_ON_WINDOWS: [&str; 2] = ["COMSPEC", "PATHEXT"];

/// The environment variable `name` of this process, or none when it is not set
/// (or is not text). A stand-in program reads what the window it stands in for
/// gave it, and a suite its size, here and nowhere else.
pub fn own_var(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

/// What a program is given to read.
#[derive(Debug, Clone)]
enum Input {
    /// What the way it is started does: nothing, for [`Run::run`]; a pipe the
    /// case writes to, for [`Run::spawn`].
    Default,
    /// Nothing: the program finds the end of its input at once.
    Closed,
    /// These bytes first. [`Run::run`] then ends the input; [`Run::spawn`]
    /// leaves it open.
    Bytes(Vec<u8>),
}

/// One word of a program's command line.
#[derive(Debug, Clone)]
enum Arg {
    /// A word, quoted for the program as the platform quotes one.
    Plain(OsString),
    /// A word written as it is, which `cmd.exe` reads by rules of its own.
    #[cfg(windows)]
    Raw(OsString),
}

/// A program to run: which, with what words, in which folder, and with what
/// environment.
#[derive(Debug, Clone)]
pub struct Run {
    program: OsString,
    args: Vec<Arg>,
    vars: Vec<(OsString, OsString)>,
    inherit: bool,
    finding_programs: bool,
    cwd: Option<PathBuf>,
    limit: Option<Duration>,
    input: Input,
}

impl Run {
    /// `program`, with no words, none of the test's environment and the
    /// [`LIMIT`]. A path is started as it is; a bare name is left to the
    /// system to find, which the cases here do not rely on.
    pub fn new(program: impl AsRef<OsStr>) -> Self {
        Self {
            program: program.as_ref().to_os_string(),
            args: Vec::new(),
            vars: Vec::new(),
            inherit: false,
            finding_programs: false,
            cwd: None,
            limit: Some(LIMIT),
            input: Input::Default,
        }
    }

    /// With one more word.
    #[must_use]
    pub fn arg(mut self, arg: impl AsRef<OsStr>) -> Self {
        self.args.push(Arg::Plain(arg.as_ref().to_os_string()));
        self
    }

    /// With more words, each as it is: nothing splits or quotes them.
    #[must_use]
    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.args.extend(
            args.into_iter()
                .map(|arg| Arg::Plain(arg.as_ref().to_os_string())),
        );
        self
    }

    /// With one more word, written into the command line as it is. For
    /// `cmd.exe /c`, which reads a line by rules of its own and takes the
    /// quotes Windows' usual quoting of a word would put in a command as its
    /// own.
    #[cfg(windows)]
    #[must_use]
    pub fn raw_arg(mut self, arg: impl AsRef<OsStr>) -> Self {
        self.args.push(Arg::Raw(arg.as_ref().to_os_string()));
        self
    }

    /// With the variable `name` set to `value`; a later value of the same name
    /// wins.
    #[must_use]
    pub fn var(mut self, name: impl AsRef<OsStr>, value: impl AsRef<OsStr>) -> Self {
        self.vars
            .push((name.as_ref().to_os_string(), value.as_ref().to_os_string()));
        self
    }

    /// With each of `vars` set, in order.
    #[must_use]
    pub fn vars<I, K, V>(mut self, vars: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: AsRef<OsStr>,
        V: AsRef<OsStr>,
    {
        for (name, value) in vars {
            self = self.var(name, value);
        }
        self
    }

    /// With the test's own environment underneath its variables, instead of
    /// the variables alone.
    #[must_use]
    pub fn inheriting_env(mut self) -> Self {
        self.inherit = true;
        self
    }

    /// For a program that starts others by their names: on Windows it is given
    /// the command interpreter and the list of extensions that make a file a
    /// program too, from the test's own environment. Nothing elsewhere.
    #[must_use]
    pub fn finding_programs(mut self) -> Self {
        self.finding_programs = true;
        self
    }

    /// Run in the folder `dir`, instead of the test's own.
    #[must_use]
    pub fn cwd(mut self, dir: impl Into<PathBuf>) -> Self {
        self.cwd = Some(dir.into());
        self
    }

    /// Ended after `limit` instead of the [`LIMIT`].
    #[must_use]
    pub fn limit(mut self, limit: Duration) -> Self {
        self.limit = Some(limit);
        self
    }

    /// Never ended for running long: for a build, which takes what it takes.
    #[must_use]
    pub fn unlimited(mut self) -> Self {
        self.limit = None;
        self
    }

    /// Given `bytes` to read: all of them, and then (for [`Run::run`]) the end
    /// of its input. A [`Run::spawn`]ed program is left its input open after.
    #[must_use]
    pub fn input(mut self, bytes: impl Into<Vec<u8>>) -> Self {
        self.input = Input::Bytes(bytes.into());
        self
    }

    /// Given no input at all, not even a pipe to write to: what a [`Run::run`]
    /// is given anyway, and a [`Run::spawn`]ed program is not unless told.
    #[must_use]
    pub fn closed_input(mut self) -> Self {
        self.input = Input::Closed;
        self
    }

    /// The program's command, with everything but its streams.
    fn command(&self) -> Command {
        let mut command = Command::new(&self.program);
        if !self.inherit {
            command
                .env_clear()
                .envs(required_on_windows(&self.vars, self.finding_programs));
        }
        command.envs(self.vars.iter().map(|(name, value)| (name, value)));
        for arg in &self.args {
            arg.add_to(&mut command);
        }
        if let Some(dir) = &self.cwd {
            command.current_dir(dir);
        }
        command
    }

    /// Runs the program to its end, or to its limit, and keeps what it left.
    /// Only a program that did not start is an error: a code other than 0 is an
    /// answer, for the case to read.
    pub fn run(self) -> Result<Ran> {
        let program = self.program.to_string_lossy().into_owned();
        let mut command = self.command();
        command
            .stdin(match self.input {
                Input::Bytes(_) => Stdio::piped(),
                Input::Default | Input::Closed => Stdio::null(),
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().map_err(|source| Error::Program {
            action: "start",
            program: program.clone(),
            source,
        })?;
        // The input goes in on a thread of its own, and the pipes are read
        // while the program runs: one that fills while the other is waited on
        // would stop it.
        let feed = match self.input {
            Input::Bytes(bytes) => Some(feed(child.stdin.take(), bytes)),
            Input::Default | Input::Closed => None,
        };
        let stdout = drain(child.stdout.take());
        let stderr = drain(child.stderr.take());
        let ended = wait(&mut child, self.limit);
        let (stdout, stderr) = (collected(stdout), collected(stderr));
        if let Some(feed) = feed {
            // A program that ended without reading it all has nothing to say of it.
            let _ = feed.join();
        }
        let (code, timed_out) = ended.map_err(|source| Error::Program {
            action: "wait for",
            program,
            source,
        })?;
        Ok(Ran {
            code,
            stdout,
            stderr,
            timed_out,
        })
    }

    /// Starts the program and leaves it running, for the case to drive: its
    /// streams are pipes ([`Spawned`]), its input one too unless it was told to
    /// be closed. Ending it is the case's, or its owner's drop.
    pub fn spawn(self) -> Result<Spawned> {
        let program = self.program.to_string_lossy().into_owned();
        let mut command = self.command();
        command
            .stdin(match self.input {
                Input::Closed => Stdio::null(),
                Input::Default | Input::Bytes(_) => Stdio::piped(),
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let child = command.spawn().map_err(|source| Error::Program {
            action: "start",
            program: program.clone(),
            source,
        })?;
        let first = match self.input {
            Input::Bytes(bytes) => Some(bytes),
            Input::Default | Input::Closed => None,
        };
        Ok(Spawned::new(child, program, first))
    }
}

impl Arg {
    fn add_to(&self, command: &mut Command) {
        match self {
            Self::Plain(word) => {
                command.arg(word);
            }
            #[cfg(windows)]
            Self::Raw(word) => {
                use std::os::windows::process::CommandExt;
                command.raw_arg(word);
            }
        }
    }
}

/// What Windows needs of the test's own environment, for each variable of
/// [`REQUIRED_ON_WINDOWS`] (and of [`PROGRAM_LOOKUP_ON_WINDOWS`], where the
/// program finds others) that `vars` do not name (the names of Windows'
/// variables are not case-sensitive) and the test has. Nothing elsewhere.
fn required_on_windows(
    vars: &[(OsString, OsString)],
    finding_programs: bool,
) -> Vec<(&'static str, OsString)> {
    if !cfg!(windows) {
        return Vec::new();
    }
    let lookup: &[&str] = if finding_programs {
        &PROGRAM_LOOKUP_ON_WINDOWS
    } else {
        &[]
    };
    REQUIRED_ON_WINDOWS
        .into_iter()
        .chain(lookup.iter().copied())
        .filter(|name| {
            !vars
                .iter()
                .any(|(given, _)| given.eq_ignore_ascii_case(name))
        })
        .filter_map(|name| std::env::var_os(name).map(|value| (name, value)))
        .collect()
}

/// Writes `bytes` to `input` on a thread of its own, and closes it.
fn feed(input: Option<impl Write + Send + 'static>, bytes: Vec<u8>) -> JoinHandle<()> {
    thread::spawn(move || {
        if let Some(mut input) = input {
            // A program that stops reading ends the write; its end says the rest.
            let _ = input.write_all(&bytes);
        }
    })
}

/// Reads `pipe` to its end on a thread of its own.
fn drain(pipe: Option<impl Read + Send + 'static>) -> JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(mut pipe) = pipe {
            // A read that fails leaves what was read; the program's end says the rest.
            let _ = pipe.read_to_end(&mut bytes);
        }
        bytes
    })
}

/// What a [`drain`] read, as text.
fn collected(reader: JoinHandle<Vec<u8>>) -> String {
    String::from_utf8_lossy(&reader.join().unwrap_or_default()).into_owned()
}

/// Waits for `child` to end: the code it exited with (none when a signal ended
/// it) and whether it was ended here, for running past `limit`.
fn wait(child: &mut Child, limit: Option<Duration>) -> io::Result<(Option<i32>, bool)> {
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok((status.code(), false)),
            Ok(None) => {}
            Err(cause) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(cause);
            }
        }
        if limit.is_some_and(|limit| started.elapsed() >= limit) {
            // It may have ended just now, which is no matter to say.
            let _ = child.kill();
            let _ = child.wait();
            return Ok((None, true));
        }
        thread::sleep(POLL);
    }
}

/// What a program that ran left.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ran {
    /// The code it exited with; none when a signal ended it or it was ended for
    /// running past its limit.
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    /// Whether it was ended for running past its limit.
    pub timed_out: bool,
}

impl Ran {
    /// What it printed and what it said, one after the other: for the cases
    /// that look in either.
    pub fn output(&self) -> String {
        format!("{}{}", self.stdout, self.stderr)
    }

    /// What it printed, read as JSON.
    pub fn json(&self) -> Result<serde_json::Value> {
        serde_json::from_str(&self.stdout).map_err(|source| Error::Json {
            text: self.stdout.clone(),
            source,
        })
    }
}

/// The whole run, for the message of an assertion that did not hold.
impl fmt::Display for Ran {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.code, self.timed_out) {
            (_, true) => write!(f, "ended for running past its limit")?,
            (Some(code), false) => write!(f, "exit code {code}")?,
            (None, false) => write!(f, "ended by a signal")?,
        }
        write!(f, "\nstdout:\n{}\nstderr:\n{}", self.stdout, self.stderr)
    }
}

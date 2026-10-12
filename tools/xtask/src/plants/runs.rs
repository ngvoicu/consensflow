//! Running the commands a plant is tried with, and reading what they wrote.
//!
//! A command runs to its end, or to the time its family gives it, in the
//! checkout's root with xtask's environment less `NODE_TEST_CONTEXT` (which a
//! `node --test` that this run happens to be under would pass on to the ones it
//! starts). `cf-process` starts it, as it does every program the product starts:
//! the command leads a process group of its own on Unix, ended whole at its time,
//! and on Windows its tree is. What it wrote to either stream is read as one text,
//! the standard output first. A stop (Ctrl-C, a terminal closed, `kill`) heard
//! meanwhile ends the command in hand the same way and is told to the driver,
//! which puts the sources back.
//!
//! `cargo xtask …` is this xtask itself, started as the program it is. Cargo would
//! first build again the xtask it starts, and that is the one running the plants:
//! stale as soon as a plant has touched what xtask is built from, and one that
//! runs cannot be replaced on Windows.
//!
//! What a run says is read by the lines it writes. A test failed where
//! `cargo test` says `test <name> ... FAILED`, `node --test` says `✖ <name> (<n>ms)`
//! and its TAP reporter says `not ok <n> - <name>`. A run that ended badly with no
//! test left to say so (a test binary that died, a signal nothing was ready for)
//! is the catch itself. A run whose source does not build is no catch.

use std::ffi::{OsStr, OsString};
use std::future::{poll_fn, Future};
use std::path::{Path, PathBuf};
use std::pin::pin;
use std::task::Poll;
use std::time::Duration;

use cf_base::env::Env;
use cf_process::{capture, CaptureFailed, Captured, Limits, Run};
use tokio::runtime::Runtime;

use super::Error;
use crate::context::Context;
use crate::process;
use crate::stops::Stops;

/// How much one stream of a run may say: far more than any run of tests does.
const OUTPUT_LIMIT: usize = 64 * 1024 * 1024;

/// The variable `node --test` sets for the ones it starts.
const NODE_TEST_CONTEXT: &str = "NODE_TEST_CONTEXT";

/// What a command did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ran {
    /// Its words, the program first.
    pub command: Vec<String>,
    /// The code it exited with: none where a signal ended it, or its time ran out.
    pub code: Option<i32>,
    /// Whether its time ran out and it was ended.
    pub hung: bool,
    /// What it wrote: its standard output, then its standard error.
    pub output: String,
}

/// How a command ended, or that this program was stopped while it ran.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Execution {
    Ended(Ran),
    Stopped,
}

/// Something that runs the commands of the plants.
pub trait Runner {
    /// Runs `command` (the program first) to its end, within the time the family gives.
    fn run(&mut self, command: &[String]) -> Result<Execution, Error>;

    /// Whether this program was asked to stop since it was last asked: a stop that
    /// came with no command in hand is not lost.
    fn stopped(&mut self) -> bool;
}

impl Ran {
    /// Whether the source built: no `could not compile`, and no error code of rustc's.
    pub fn compiled(&self) -> bool {
        !self.output.contains("could not compile") && !has_error_code(&self.output)
    }

    /// What caught the plant, if this run did: the names of the tests it says failed,
    /// each once, in the order it says them; else, for a run that ended with a code
    /// other than 0 and no test to say so, the run itself.
    pub fn caught(&self) -> Vec<String> {
        let mut caught = failed_tests(&self.output);
        if caught.is_empty() && self.compiled() && !self.hung && self.code != Some(0) {
            let words = self.command.get(1..).unwrap_or_default().join(" ");
            caught.push(match died(&self.output) {
                Some(how) => format!("{words}: the test binary died ({how})"),
                None => match self.code {
                    Some(code) => format!("{words}: ended with {code}"),
                    None => format!("{words}: was ended by a signal"),
                },
            });
        }
        caught
    }
}

/// Whether `output` holds an error code of rustc's: `error[E0432]`.
fn has_error_code(output: &str) -> bool {
    output.match_indices("error[E").any(|(at, found)| {
        let rest = &output[at + found.len()..];
        let digits = rest.chars().take_while(char::is_ascii_digit).count();
        digits > 0 && rest[digits..].starts_with(']')
    })
}

/// The tests `output` says failed, in the order it says them, each once.
fn failed_tests(output: &str) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for line in output.lines() {
        let named = cargo_name(line)
            .or_else(|| spec_name(line))
            .or_else(|| tap_name(line));
        if let Some(name) = named.filter(|name| !found.iter().any(|one| one == name)) {
            found.push(name.to_owned());
        }
    }
    found
}

/// `test <name> ... FAILED`: the name has no space in it.
fn cargo_name(line: &str) -> Option<&str> {
    let name = line.strip_prefix("test ")?.strip_suffix(" ... FAILED")?;
    (!name.is_empty() && !name.contains(char::is_whitespace)).then_some(name)
}

/// `✖ <name> (<n>ms)`, as `node --test` says a test failed, in whatever indent it is nested.
fn spec_name(line: &str) -> Option<&str> {
    let rest = line.trim_start().strip_prefix("✖ ")?.strip_suffix("ms)")?;
    let open = rest.rfind(" (")?;
    let time = &rest[open + 2..];
    let number = time.starts_with(|c: char| c.is_ascii_digit())
        && time.chars().all(|c| c.is_ascii_digit() || c == '.');
    (number && open > 0).then(|| &rest[..open])
}

/// `not ok <n> - <name>`, as the TAP reporter says a test failed.
fn tap_name(line: &str) -> Option<&str> {
    let rest = line.trim_start().strip_prefix("not ok ")?;
    let digits = rest.chars().take_while(char::is_ascii_digit).count();
    let name = rest[digits..].strip_prefix(" - ").filter(|_| digits > 0)?;
    (!name.is_empty()).then_some(name)
}

/// What cargo says of a test binary that was ended: ``process didn't exit
/// successfully: `<command>` (<how>)``, the `<how>` of the first.
fn died(output: &str) -> Option<&str> {
    let marker = "process didn't exit successfully: `";
    let after = &output[output.find(marker)? + marker.len()..];
    let rest = after[after.find('`')? + 1..].strip_prefix(" (")?;
    rest.split(')').next()
}

/// Runs the commands as processes, within a limit of time, through `cf-process`.
pub struct Execute {
    runtime: Runtime,
    root: PathBuf,
    own: PathBuf,
    env: Env,
    limits: Limits,
    stops: Stops,
}

impl Execute {
    /// A runner of commands from the checkout's root, none given more than `limit`.
    /// `own` is this xtask, which `cargo xtask` is. The stops are listened for from here on.
    pub fn new(context: &Context, own: PathBuf, limit: Duration) -> Result<Self, Error> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(Error::files(
                "start the runtime to run the plants in",
                &context.root,
            ))?;
        let stops = {
            let _inside = runtime.enter();
            Stops::listen().map_err(Error::files("listen for a stop", &context.root))?
        };
        Ok(Self::with_stops(context, own, limit, runtime, stops))
    }

    /// As [`new`](Self::new), with the stops it is given and the runtime they were made in.
    pub(super) fn with_stops(
        context: &Context,
        own: PathBuf,
        limit: Duration,
        runtime: Runtime,
        stops: Stops,
    ) -> Self {
        let env = Env::from_vars(
            context
                .env
                .iter()
                .filter(|(name, _)| *name != OsStr::new(NODE_TEST_CONTEXT))
                .map(|(name, value)| (name.to_owned(), value.to_owned())),
        );
        Self {
            runtime,
            root: context.root.clone(),
            own,
            env,
            limits: Limits {
                timeout: limit,
                max_buffer: OUTPUT_LIMIT,
            },
            stops,
        }
    }

    /// The program to start for `command`, and its words.
    fn start(&self, command: &[String]) -> Result<Run, Error> {
        let (program, args): (OsString, &[String]) = match command {
            [cargo, xtask, args @ ..] if cargo == "cargo" && xtask == "xtask" => {
                (self.own.clone().into_os_string(), args)
            }
            [program, args @ ..] => (process::resolved(OsStr::new(program), &self.env), args),
            [] => (OsString::new(), command),
        };
        // A bare name that the PATH did not give is a program that is not there.
        if Path::new(&program).components().count() <= 1 {
            return Err(process::Failure::NotFound {
                program: program.to_string_lossy().into_owned(),
            }
            .into());
        }
        Ok(Run {
            program: PathBuf::from(program),
            args: args.iter().map(OsString::from).collect(),
            verbatim: false,
        })
    }
}

impl Runner for Execute {
    fn run(&mut self, command: &[String]) -> Result<Execution, Error> {
        let run = self.start(command)?;
        let Self {
            runtime,
            root,
            env,
            limits,
            stops,
            ..
        } = self;
        let result = runtime.block_on(async {
            let mut running = pin!(capture(&run, Some(root.as_path()), env, *limits, |_| {}));
            // Dropped with the block, the command in hand is ended whole.
            poll_fn(|cx| {
                if let Poll::Ready(done) = running.as_mut().poll(cx) {
                    return Poll::Ready(Some(done));
                }
                stops.poll(cx).map(|()| None)
            })
            .await
        });
        match result {
            None => Ok(Execution::Stopped),
            Some(done) => Ran::of(command, done).map(Execution::Ended),
        }
    }

    fn stopped(&mut self) -> bool {
        let Self { runtime, stops, .. } = self;
        runtime.block_on(async {
            // A stop is told to who listens when the runtime has its turn.
            tokio::task::yield_now().await;
            poll_fn(|cx| Poll::Ready(stops.poll(cx).is_ready())).await
        })
    }
}

impl Ran {
    /// What `command` did, from what `cf-process` answered of it.
    fn of(command: &[String], done: Result<Captured, CaptureFailed>) -> Result<Self, Error> {
        let (code, hung, stdout, stderr) = match done {
            Ok(Captured { stdout, stderr }) => (Some(0), false, stdout, stderr),
            Err(failed) if failed.message.starts_with("spawn ") && !failed.killed => {
                return Err(Error::Process(process::Failure::Refused {
                    program: command.first().cloned().unwrap_or_default(),
                    cause: std::io::Error::other(failed.message),
                }));
            }
            Err(failed) => (failed.code, failed.killed, failed.stdout, failed.stderr),
        };
        Ok(Self {
            command: command.to_vec(),
            code,
            hung,
            output: stdout + &stderr,
        })
    }
}

#[cfg(test)]
mod tests;

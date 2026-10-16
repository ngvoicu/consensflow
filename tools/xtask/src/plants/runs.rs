//! Running the commands a plant is tried with, and reading what they wrote.
//!
//! A command runs to its end, or to the time its family gives it, in the
//! checkout's root with xtask's environment less `NODE_TEST_CONTEXT` (which a
//! `node --test` that this run happens to be under would pass on to the ones it
//! starts), and with no colour (`NO_COLOR=1`, `FORCE_COLOR=0`). `cf-process`
//! starts it, as it does every program the product starts: the command leads a
//! process group of its own on Unix, ended whole at its time, and on Windows its
//! tree is. What it wrote to either stream is read as one text, the standard
//! output first, with any terminal formatting taken out. A stop (Ctrl-C, a
//! terminal closed, `kill`) heard meanwhile ends the command in hand the same way
//! and is told to the driver, which puts the sources back.
//!
//! `cargo xtask …` is this xtask itself, started as the program it is. Cargo would
//! first build again the xtask it starts, and that is the one running the plants:
//! stale as soon as a plant has touched what xtask is built from, and one that
//! runs cannot be replaced on Windows. `cargo test …` is asked to say how its
//! build went in its own structured words (`--message-format=json-render-diagnostics`,
//! whose `build-finished` says whether it built), which no test's output can pass for.
//!
//! What a run says is read from its reporter's own result lines (`reporters`:
//! libtest's for `cargo test` and the `cargo xtask` suites, Node's, Playwright's,
//! by the command), never from what a test printed; and only in a run that exited
//! with a failure. A run that ended badly with no test left to say so (a test
//! binary that died, a signal nothing was ready for, a Node test file ended after
//! tests of it ran) is the catch itself, which names no test. A run whose source
//! does not build is no catch, nor is one in which no test could see the plant (a
//! test file that does not parse, a module that is not there or throws as it is
//! loaded, a filter that finds nothing), whatever test failed beside it. What makes
//! a catch of all this is the driver's: the test a plant was meant for failing with
//! it, where the failure names tests.

mod reporters;

use std::ffi::{OsStr, OsString};
use std::future::{poll_fn, Future};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::pin::pin;
use std::task::Poll;
use std::time::Duration;

use cf_base::env::Env;
use cf_process::{capture, CaptureFailed, Captured, Limits, Run};
use tokio::runtime::Runtime;

use self::reporters::Reporter;
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

    /// What the checkout is now, as far as a command's run can tell: the same text
    /// where nothing of it changed, whatever wrote it, and another where it cannot be
    /// told. How a command did without a plant is known in the state it ran in alone.
    fn state(&mut self) -> String;
}

impl Ran {
    /// Whether the source built, by what was said before any test ran: cargo builds
    /// everything a run tests first, and what comes after is a test's own output (a
    /// broken build a test quotes, its structured words too, or one a test makes,
    /// which the driver builds first where a run builds the native `cf`). Where cargo
    /// said so in its structured words, they decide, every build having to succeed;
    /// else what cargo and rustc write of a failed build.
    pub fn compiled(&self) -> bool {
        let before = before_tests(&self.output);
        if let Some(built) = build_finished(before) {
            return built;
        }
        !before.contains("could not compile") && !has_error_code(before)
    }

    /// Why no test of the run could see the plant, if none could, as its reporter says
    /// (`reporters`): Node that a test file failed whole and it ran no test besides
    /// (a module that does not parse, imports what is not there, or throws as it is
    /// loaded, after a top-level `await` too), or that one failed whole with its
    /// module loader's error (a stack through `node:internal/modules/`); Playwright
    /// that it found no test (`Error: No tests found.`, which a spec that cannot be
    /// imported leaves too), or that an error came outside every test. A test file
    /// that ended after tests of it ran (`process.exit` in one) is none of these.
    pub fn broken(&self) -> Option<String> {
        match Reporter::of(&self.command) {
            Reporter::Libtest => None,
            Reporter::Node => {
                let files = self.failed_files();
                let file = files.first()?;
                let counted = reporters::node_counted(&self.output).unwrap_or(files.len());
                if counted <= files.len() {
                    return Some(format!("the test file {file} ran no test"));
                }
                self.output
                    .contains("node:internal/modules/")
                    .then(|| format!("the test file {file} could not be loaded"))
            }
            Reporter::Playwright => self.output.lines().map(str::trim).find_map(|line| {
                (line.starts_with("Error: No tests found")
                    || line.contains("not a part of any test"))
                .then(|| format!("Playwright: {line}"))
            }),
        }
    }

    /// Whether the run's failure names a test of its own, which a plant can be
    /// attributed by: a test file that failed whole names none.
    pub fn names_tests(&self) -> bool {
        let files = self.failed_files();
        self.failed().iter().any(|name| !files.contains(name))
    }

    /// The tests the run's reporter says failed, each once, in the order it says them.
    fn failed(&self) -> Vec<String> {
        Reporter::of(&self.command)
            .failed(&self.output)
            .into_iter()
            .map(|test| test.name)
            .collect()
    }

    /// The test files Node says failed whole: a failed test named by the file's
    /// path (spaces and all), which Node says is written at the file's start.
    fn failed_files(&self) -> Vec<String> {
        Reporter::of(&self.command)
            .failed(&self.output)
            .into_iter()
            .filter(|test| {
                is_test_file(&test.name)
                    && test
                        .location
                        .as_ref()
                        .is_some_and(|at| at.ends_with(&format!("{}:1:1", test.name)))
            })
            .map(|test| test.name)
            .collect()
    }

    /// Whether cargo's builds succeeded, where it said so in its structured words.
    pub fn build_finished(&self) -> Option<bool> {
        build_finished(&self.output)
    }

    /// What caught the plant, if this run did, which only a run that exited with a
    /// failure can: the names of the tests it says failed, each once, in the order
    /// it says them; else, for a run with no test to say so, the run itself.
    pub fn caught(&self) -> Vec<String> {
        if self.code == Some(0) {
            return Vec::new();
        }
        let mut caught = self.failed();
        if caught.is_empty() && self.compiled() && !self.hung {
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

/// Whether cargo's builds succeeded, where it said so in its structured words:
/// `{"reason":"build-finished","success":…}`, once for each build.
fn build_finished(output: &str) -> Option<bool> {
    output
        .lines()
        .filter(|line| line.starts_with(r#"{"reason":"build-finished""#))
        .map(|line| line.contains(r#""success":true"#))
        .reduce(|all, built| all && built)
}

/// What `output` says before its first test ran (`running <n> tests` of cargo,
/// `Running <n> tests` of Playwright): all of it where none did.
fn before_tests(output: &str) -> &str {
    let mut at = 0;
    for line in output.split_inclusive('\n') {
        let started = ["running ", "Running "].iter().any(|word| {
            line.strip_prefix(word).is_some_and(|rest| {
                let digits = rest.chars().take_while(char::is_ascii_digit).count();
                digits > 0 && rest[digits..].starts_with(" test")
            })
        });
        if started {
            return &output[..at];
        }
        at += line.len();
    }
    output
}

/// Whether `name` is named as a test file is: a path to a script.
fn is_test_file(name: &str) -> bool {
    [".mjs", ".cjs", ".js", ".mts", ".cts", ".ts"]
        .iter()
        .any(|extension| name.ends_with(extension))
}

/// `output` with the terminal's formatting taken out: its control sequences
/// (`ESC [ … m`, and any other `ESC [` sequence and two-character escape).
fn plain(output: &str) -> String {
    let mut plain = String::with_capacity(output.len());
    let mut characters = output.chars().peekable();
    while let Some(character) = characters.next() {
        if character != '\u{1b}' {
            plain.push(character);
            continue;
        }
        if characters.next_if_eq(&'[').is_some() {
            // Parameters and intermediates up to the final byte, `@` to `~`.
            for next in characters.by_ref() {
                if ('@'..='~').contains(&next) {
                    break;
                }
            }
        } else {
            characters.next();
        }
    }
    plain
}

/// The words a command is started with: a `cargo test` is asked for its build's
/// structured status.
fn words(command: &[String]) -> Vec<String> {
    match command {
        [cargo, test, rest @ ..] if cargo == "cargo" && test == "test" => [
            test.clone(),
            "--message-format=json-render-diagnostics".to_owned(),
        ]
        .into_iter()
        .chain(rest.iter().cloned())
        .collect(),
        [_, rest @ ..] => rest.to_vec(),
        [] => Vec::new(),
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
    /// How many states could not be told, each told apart from every other.
    untold: u64,
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
        let colour = [OsStr::new("NO_COLOR"), OsStr::new("FORCE_COLOR")];
        let env = Env::from_vars(
            context
                .env
                .iter()
                .filter(|(name, _)| *name != OsStr::new(NODE_TEST_CONTEXT))
                .filter(|(name, _)| !colour.contains(name))
                .map(|(name, value)| (name.to_owned(), value.to_owned()))
                .chain([
                    (OsString::from("NO_COLOR"), OsString::from("1")),
                    (OsString::from("FORCE_COLOR"), OsString::from("0")),
                ]),
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
            untold: 0,
        }
    }

    /// What `git` says to `words` in the checkout, where it says it and succeeds.
    fn git(&mut self, words: &[&str]) -> Option<String> {
        let command: Vec<String> = ["git"]
            .iter()
            .chain(words)
            .map(|word| (*word).to_owned())
            .collect();
        match self.run(&command) {
            Ok(Execution::Ended(ran)) if ran.code == Some(0) => Some(ran.output),
            _ => None,
        }
    }

    /// The program to start for `command`, and its words.
    fn start(&self, command: &[String]) -> Result<Run, Error> {
        let (program, args): (OsString, Vec<String>) = match command {
            [cargo, xtask, args @ ..] if cargo == "cargo" && xtask == "xtask" => {
                (self.own.clone().into_os_string(), args.to_vec())
            }
            [program, ..] => (
                process::resolved(OsStr::new(program), &self.env),
                words(command),
            ),
            [] => (OsString::new(), Vec::new()),
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

    /// The commit the checkout is at, what git says of it beyond that (every file
    /// changed, added or untracked, ignored ones aside), and the bytes of each such
    /// file, hashed: a file put back as it was is the state it was. Where git says
    /// nothing (no checkout of git's, a stop), a state of its own each time.
    fn state(&mut self) -> String {
        let head = self.git(&["rev-parse", "HEAD"]);
        let status = self.git(&["status", "--porcelain=v1", "-z", "--untracked-files=all"]);
        let (Some(head), Some(status)) = (head, status) else {
            self.untold += 1;
            return format!("untold {}", self.untold);
        };
        let mut hasher = DefaultHasher::new();
        head.hash(&mut hasher);
        status.hash(&mut hasher);
        for path in changed(&status) {
            std::fs::read(self.root.join(path)).ok().hash(&mut hasher);
        }
        format!("{:016x}", hasher.finish())
    }
}

/// The paths of `git status --porcelain=v1 -z`: each entry `XY <path>`, a rename's or
/// a copy's followed by the path it was.
fn changed(status: &str) -> Vec<&str> {
    let mut paths = Vec::new();
    let mut entries = status.split('\0').filter(|entry| !entry.is_empty());
    while let Some(entry) = entries.next() {
        let (Some(code), Some(path)) = (entry.get(..2), entry.get(3..)) else {
            continue;
        };
        paths.push(path);
        if code.contains(['R', 'C']) {
            entries.next();
        }
    }
    paths
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
            output: plain(&(stdout + &stderr)),
        })
    }
}

#[cfg(test)]
mod tests;

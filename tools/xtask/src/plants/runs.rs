//! Running the commands a plant is tried with, and reading what their tests did.
//!
//! A command runs to its end, or to the time its family gives it, in the
//! checkout's root with xtask's environment less `NODE_TEST_CONTEXT` (which a
//! `node --test` that this run happens to be under would pass on to the ones it
//! starts), and with no colour (`NO_COLOR=1`, `FORCE_COLOR=0`). `cf-process`
//! starts it, as it does every program the product starts: the command leads a
//! process group of its own on Unix, ended whole at its time, and on Windows its
//! tree is. A stop (Ctrl-C, a terminal closed, `kill`) heard meanwhile ends the
//! command in hand the same way and is told to the driver, which puts the sources
//! back. A run that writes more than `OUTPUT_LIMIT` to a stream is ended for it.
//!
//! `cargo xtask …` is this xtask itself, started as the program it is. Cargo would
//! first build again the xtask it starts, and that is the one running the plants:
//! stale as soon as a plant has touched what xtask is built from, and one that
//! runs cannot be replaced on Windows.
//!
//! What a run's tests did is read from the report its reporter wrote to a folder
//! of the run's own (`reporters`, `results`): which tests ran, which failed, which
//! test files ran no test, which errors came outside every test. What the run
//! printed is read for none of it: it is what is said of a plant, no more. A run
//! with no report names no test: it failed, or it did not, as its exit status
//! says, and the driver confirms such a failure twice each way.

mod reporters;
pub(super) mod results;

use std::ffi::{OsStr, OsString};
use std::fs;
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
use self::results::Results;
use super::Error;
use crate::context::Context;
use crate::dispatch::{Console, Failure};
use crate::process::{self, Invocation};
use crate::stops::Stops;

/// How much one stream of a run may say: far more than any run of tests does.
const OUTPUT_LIMIT: usize = 64 * 1024 * 1024;

/// The variable `node --test` sets for the ones it starts.
const NODE_TEST_CONTEXT: &str = "NODE_TEST_CONTEXT";

/// Where each run's report is written, under the build folder git leaves out.
const RUNS: &str = "app/src-tauri/target/plants-runs";

/// Where the workspace's tools are installed (`cargo install --root`).
const TOOLS: &str = "app/src-tauri/target/tools";

/// The status nextest ends with where the build failed.
const NEXTEST_BUILD_FAILED: i32 = 101;

/// The status nextest ends with where no test was there to run.
const NEXTEST_NO_TESTS: i32 = 4;

/// The status nextest ends with where a test binary could not say its tests.
const NEXTEST_UNLISTED: i32 = 104;

/// What a command did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ran {
    /// Its words, the program first, as the plant's table says them.
    pub command: Vec<String>,
    /// The code it exited with: none where a signal ended it, or it was ended.
    pub code: Option<i32>,
    /// Whether its time ran out and it was ended.
    pub hung: bool,
    /// Whether it wrote more than a run may and was ended for it.
    pub overflowed: bool,
    /// What it wrote: its standard output, then its standard error.
    pub output: String,
    /// What its reporter's report says its tests did: none where it wrote none.
    pub results: Option<Results>,
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
    /// A run of `command` that ended with `code`, wrote `output` and reported `results`.
    #[cfg(test)]
    pub fn new(
        command: &[&str],
        code: Option<i32>,
        output: &str,
        results: Option<Results>,
    ) -> Self {
        Self {
            command: command.iter().map(|word| (*word).to_owned()).collect(),
            code,
            hung: false,
            overflowed: false,
            output: output.to_owned(),
            results,
        }
    }

    /// Whether the source built: nextest says a build that failed by its status; of
    /// a suite of xtask's, what cargo and rustc wrote of a build that failed where no
    /// test binary ran, so that no test wrote it. A run of Node or Playwright builds
    /// nothing.
    pub fn compiled(&self) -> bool {
        match Reporter::of(&self.command) {
            Reporter::Nextest => self.code != Some(NEXTEST_BUILD_FAILED),
            Reporter::Libtest => {
                self.results.is_some()
                    || (!self.output.contains("could not compile") && !has_error_code(&self.output))
            }
            Reporter::Node | Reporter::Playwright | Reporter::Status => true,
        }
    }

    /// Why no test of the run could see the plant, if none could, as its reporter
    /// says: a test file of Node's that failed whole and ran no test of its own (it
    /// did not parse, imported what is not there, threw as it loaded, after a
    /// top-level `await` too, or ended before a test of it was told), whatever the
    /// other files did; an error Playwright says came outside every test, or no test
    /// it found; no test nextest found to run, or a test binary that could not say
    /// its tests; a run that failed with no test run at all. A test file that failed
    /// whole after tests of it ran is none of these.
    pub fn broken(&self) -> Option<String> {
        let reporter = Reporter::of(&self.command);
        if reporter == Reporter::Nextest {
            match self.code {
                Some(NEXTEST_NO_TESTS) => return Some("nextest found no test to run".to_owned()),
                Some(NEXTEST_UNLISTED) => {
                    return Some("nextest: a test binary could not say its tests".to_owned())
                }
                _ => {}
            }
        }
        let results = self.results.as_ref()?;
        let none_ran = results.tests.is_empty() && self.code != Some(0);
        match reporter {
            Reporter::Node => match results.files_that_ran_none().first() {
                Some(file) => Some(format!("the test file {file} ran no test")),
                None => none_ran.then(|| "Node ran no test".to_owned()),
            },
            Reporter::Playwright => match results.outside.first() {
                Some(error) => Some(format!("Playwright: an error outside every test: {error}")),
                None => results
                    .tests
                    .is_empty()
                    .then(|| "Playwright found no test".to_owned()),
            },
            Reporter::Nextest | Reporter::Libtest => none_ran.then(|| "it ran no test".to_owned()),
            Reporter::Status => None,
        }
    }

    /// Whether the run's failure names a test of its own, which a plant can be
    /// attributed by: a test file that failed whole names none.
    pub fn names_tests(&self) -> bool {
        self.results
            .as_ref()
            .is_some_and(|results| results.failed().iter().any(|test| !test.is_whole_file()))
    }

    /// The names of every test the run's report says ran, failed or not.
    pub fn tests_ran(&self) -> Vec<&str> {
        self.results
            .iter()
            .flat_map(|results| &results.tests)
            .map(|test| test.name.as_str())
            .collect()
    }

    /// What caught the plant, if this run did, which only a run that exited with a
    /// failure can: the tests its report says failed, each once, in the order it says
    /// them; else, for a run that names no test, the run itself.
    pub fn caught(&self) -> Vec<String> {
        if self.code == Some(0) {
            return Vec::new();
        }
        let failed: Vec<String> = self
            .results
            .iter()
            .flat_map(Results::failed)
            .map(|test| test.name.clone())
            .collect();
        if !failed.is_empty() || !self.compiled() || self.hung || self.overflowed {
            return failed;
        }
        let words = self.command.get(1..).unwrap_or_default().join(" ");
        vec![match died(&self.output) {
            Some(how) => format!("{words}: the test binary died ({how})"),
            None => match self.code {
                Some(code) => format!("{words}: ended with {code}"),
                None => format!("{words}: was ended by a signal"),
            },
        }]
    }

    /// Whether cargo's builds succeeded, where it said so in its structured words.
    pub fn build_finished(&self) -> Option<bool> {
        self.output
            .lines()
            .filter(|line| line.starts_with(r#"{"reason":"build-finished""#))
            .map(|line| line.contains(r#""success":true"#))
            .reduce(|all, built| all && built)
    }
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

/// Whether `output` holds an error code of rustc's: `error[E0432]`.
fn has_error_code(output: &str) -> bool {
    output.match_indices("error[E").any(|(at, found)| {
        let rest = &output[at + found.len()..];
        let digits = rest.chars().take_while(char::is_ascii_digit).count();
        digits > 0 && rest[digits..].starts_with(']')
    })
}

/// What cargo says of a test binary that was ended: ``process didn't exit
/// successfully: `<command>` (<how>)``, the `<how>` of the first; told, not read.
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
    /// The variable cargo reads its target runner from on this machine, once asked.
    runner_variable: Option<String>,
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
            runner_variable: None,
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

    /// A folder for a run's report, under `RUNS`, removed once dropped: one of its own
    /// for each run, whatever else runs beside it.
    fn folder(&self) -> Result<tempfile::TempDir, Error> {
        let runs = self.root.join(RUNS);
        fs::create_dir_all(&runs)
            .and_then(|()| tempfile::Builder::new().prefix("run-").tempdir_in(&runs))
            .map_err(Error::files("make a folder for a run's report in", &runs))
    }

    /// Where nextest is: installed for the workspace (`TOOLS`), or on the PATH.
    fn nextest(&self) -> Result<String, Error> {
        let name = if cfg!(windows) {
            "cargo-nextest.exe"
        } else {
            "cargo-nextest"
        };
        let installed = self.root.join(TOOLS).join("bin").join(name);
        if installed.is_file() {
            return Ok(installed.display().to_string());
        }
        let found = process::resolved(OsStr::new("cargo-nextest"), &self.env);
        if Path::new(&found).components().count() > 1 {
            return Ok(found.to_string_lossy().into_owned());
        }
        Err(process::Failure::NotFound {
            program: format!(
                "cargo-nextest (install it for the workspace: cargo install cargo-nextest --locked --root {TOOLS})"
            ),
        }
        .into())
    }

    /// The variable cargo reads the target runner of this machine's own target from
    /// (`CARGO_TARGET_<HOST>_RUNNER`), the host as `rustc -vV` says it.
    fn runner_variable(&mut self) -> Result<String, Error> {
        if let Some(known) = &self.runner_variable {
            return Ok(known.clone());
        }
        let asked = ["rustc".to_owned(), "-vV".to_owned()];
        let host = match self.run(&asked)? {
            Execution::Ended(ran) => ran
                .output
                .lines()
                .find_map(|line| line.strip_prefix("host: "))
                .map(str::trim)
                .map(str::to_owned),
            Execution::Stopped => None,
        };
        let host = host.ok_or_else(|| process::Failure::NotFound {
            program: "rustc, which says the machine's target (rustc -vV)".to_owned(),
        })?;
        let variable = format!(
            "CARGO_TARGET_{}_RUNNER",
            host.to_ascii_uppercase().replace(['-', '.'], "_")
        );
        self.runner_variable = Some(variable.clone());
        Ok(variable)
    }

    /// The program to start for `words` (the program first), and its words.
    fn start(&self, words: &[String]) -> Result<Run, Error> {
        let (program, args): (OsString, Vec<String>) = match words {
            [cargo, xtask, args @ ..] if cargo == "cargo" && xtask == "xtask" => {
                (self.own.clone().into_os_string(), args.to_vec())
            }
            [program, args @ ..] => (
                process::resolved(OsStr::new(program), &self.env),
                args.to_vec(),
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
        let reporter = Reporter::of(command);
        // A folder of the run's own, gone with it, for a reporter that writes one.
        let folder = match reporter {
            Reporter::Status => None,
            _ => Some(self.folder()?),
        };
        let dir = folder
            .as_ref()
            .map_or_else(|| self.root.clone(), |folder| folder.path().to_path_buf());
        let dir = dir.as_path();
        let nextest = match reporter {
            Reporter::Nextest => self.nextest()?,
            _ => String::new(),
        };
        let variable = match reporter {
            Reporter::Libtest => Some(self.runner_variable()?),
            _ => None,
        };
        let own = self.own.display().to_string();
        let runner = variable.as_deref().map(|variable| (variable, own.as_str()));
        let setup = reporter
            .setup(command, dir, &nextest, runner)
            .map_err(Error::files("write the config of a run in", dir))?;
        let run = self.start(&setup.words)?;
        let env = Env::from_vars(
            self.env
                .iter()
                .map(|(name, value)| (name.to_owned(), value.to_owned()))
                .chain(setup.env),
        );
        let Self {
            runtime,
            root,
            limits,
            stops,
            ..
        } = self;
        let result = runtime.block_on(async {
            let mut running = pin!(capture(&run, Some(root.as_path()), &env, *limits, |_| {}));
            // Dropped with the block, the command in hand is ended whole.
            poll_fn(|cx| {
                if let Poll::Ready(done) = running.as_mut().poll(cx) {
                    return Poll::Ready(Some(done));
                }
                stops.poll(cx).map(|()| None)
            })
            .await
        });
        let execution = match result {
            None => Ok(Execution::Stopped),
            Some(done) => Ran::of(command, done).map(|mut ran| {
                match reporter.results(dir) {
                    Some(Ok(results)) => ran.results = Some(results),
                    Some(Err(why)) => ran.output.push_str(&format!(
                        "\nthe report of its tests could not be read: {why}\n"
                    )),
                    None => {}
                }
                Execution::Ended(ran)
            }),
        };
        drop(folder);
        execution
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
    /// What `command` did, from what `cf-process` answered of it: a stream that said
    /// more than a run may is `cf-process`'s own words for it.
    fn of(command: &[String], done: Result<Captured, CaptureFailed>) -> Result<Self, Error> {
        let (code, hung, overflowed, stdout, stderr) = match done {
            Ok(Captured { stdout, stderr }) => (Some(0), false, false, stdout, stderr),
            Err(failed) if failed.message.starts_with("spawn ") && !failed.killed => {
                return Err(Error::Process(process::Failure::Refused {
                    program: command.first().cloned().unwrap_or_default(),
                    cause: std::io::Error::other(failed.message),
                }));
            }
            Err(failed) => {
                let overflowed = failed.message.ends_with("maxBuffer length exceeded");
                (
                    failed.code,
                    failed.killed,
                    overflowed,
                    failed.stdout,
                    failed.stderr,
                )
            }
        };
        Ok(Self {
            command: command.to_vec(),
            code,
            hung,
            overflowed,
            output: plain(&(stdout + &stderr)),
            results: None,
        })
    }
}

/// `cargo xtask plants-log <folder> <test binary> [its words…]`: cargo's target
/// runner while a plant's run is a suite of xtask's. Runs the test binary with its
/// words and asks libtest to log each test's result to a file of its own in
/// `<folder>`, which no test's output reaches; ends as the binary did.
pub fn log(context: &Context, args: &[OsString], _console: &mut Console) -> Result<i32, Failure> {
    let [folder, binary, words @ ..] = args else {
        return Err(Failure::Usage(
            "plants-log takes a folder, a test binary and its words".to_owned(),
        ));
    };
    let here = std::env::current_dir()?;
    Ok(process::run(
        &logged(Path::new(folder), binary, words, here),
        &context.env,
    )?)
}

/// The test binary `binary` with its `words`, run in `here`, asking libtest to log to
/// the next file of `folder`, named in turn and by the binary's name.
fn logged(folder: &Path, binary: &OsStr, words: &[OsString], here: PathBuf) -> Invocation {
    let count = fs::read_dir(folder).map(Iterator::count).unwrap_or(0);
    let name = Path::new(binary)
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    Invocation::new(binary, here)
        .args(words)
        .arg("--logfile")
        .arg(folder.join(format!("{count:04}-{name}.log")))
}

#[cfg(test)]
mod tests;

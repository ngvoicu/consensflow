//! Running the commands a plant is tried with, and reading what their tests did.
//!
//! A command runs to its end, or to the time its family gives it, in the
//! checkout's root (a suite of xtask's in the folder the suite runs its tests in,
//! with the variables it sets: `reporters`) with xtask's environment less
//! `NODE_TEST_CONTEXT` (which a
//! `node --test` that this run happens to be under would pass on to the ones it
//! starts), and with no colour (`NO_COLOR=1`, `FORCE_COLOR=0`). `cf-process`
//! starts it, as it does every program the product starts, held whole
//! (`capture_whole`): a command asked to end because its time ran out hung,
//! whatever it exits with then; and nothing it started outlives it, before the
//! driver puts the sources back. On Unix it leads a process group of its own, which
//! is ended once the command has ended, however it ended; on Windows it runs in a
//! job of its own that nothing it starts can leave, ended with it, and waited for
//! until each process of the job has ended, as its handle says. What it started
//! that did not end, or that still held its output a moment later, is an error, and
//! no verdict: the run is not over. It has ended when it exits, not when its output
//! closes. A stop (Ctrl-C, a terminal closed, `kill`) heard meanwhile ends the
//! command in hand the same way and is told to the driver, which puts the sources
//! back. A run that writes more than `OUTPUT_LIMIT` to a stream is ended for it.
//!
//! A run of the tests of `cf-e2e` is preceded by the release builds of the
//! programs they run, from the sources as they are, with the plant or without it
//! (`CF_E2E_PROGRAMS`), and its tests are told where cargo says it put those
//! (`CF_E2E_BUILT`) and build nothing; a build that does not succeed ends the run,
//! with no test run.
//!
//! `cargo xtask …` that is no suite of tests is this xtask itself, started as the
//! program it is. Cargo would first build again the xtask it starts, and that is
//! the one running the plants: stale as soon as a plant has touched what xtask is
//! built from, and one that runs cannot be replaced on Windows.
//!
//! What a run's tests did is read from the report its reporter wrote to a folder
//! of the run's own (`reporters`, `results`): which tests ran, which failed, which
//! test files ran no test, which errors came outside every test. What the run
//! printed is read for none of it: it is what is said of a plant, no more. A run
//! whose reporter wrote no report, or one that cannot be read whole, names no test
//! and catches nothing, whatever its exit status: no test was seen to run; nor does
//! one that failed while its report names no test that failed. Only a command that
//! has no reporter (a smoke that says no more than its status) is judged by its
//! status, and the driver confirms such a failure twice each way.

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
use cf_process::{capture_whole, CaptureFailed, Captured, Limits, Run, LEFT_RUNNING};
use tokio::runtime::Runtime;

use self::reporters::Reporter;
use self::results::{Results, Test};
use super::Error;
use crate::context::Context;
use crate::process::{self, Invocation};
use crate::stops::Stops;

/// How much one stream of a run may say: far more than any run of tests does.
const OUTPUT_LIMIT: usize = 64 * 1024 * 1024;

/// The variable `node --test` sets for the ones it starts.
const NODE_TEST_CONTEXT: &str = "NODE_TEST_CONTEXT";

/// The variables a program is told its temp folder by: Unix's, and Windows' two.
const TEMP_FOLDER: [&str; 3] = ["TMPDIR", "TMP", "TEMP"];

/// Where each run's report is written, under the build folder git leaves out.
const RUNS: &str = "app/src-tauri/target/plants-runs";

/// Where the workspace's tools are installed (`cargo install --root`).
const TOOLS: &str = "app/src-tauri/target/tools";

/// The variable that tells the tests of `cf-e2e` the folder the programs they run
/// were built in before the run (crates/cf-e2e/src/cf.rs, `BUILT`).
const CF_E2E_BUILT: &str = "CF_E2E_BUILT";

/// The release builds of the programs the tests of `cf-e2e` run, each as the rig
/// makes it (crates/cf-e2e/src/cf.rs; `build-cf` builds `cf` so too): `cf`, then
/// the pane host, apart. Built in one, cargo gives `cf` the features the two take
/// between them, another `cf`, and builds `cf-daemon` and `cf` again each time one
/// way follows the other (2026-10-16). They install nothing, and say in cargo's
/// structured words whether they built.
pub const CF_E2E_PROGRAMS: [[&str; 10]; 2] = [
    [
        "cargo",
        "build",
        "--release",
        "--locked",
        "--offline",
        "--message-format=json-render-diagnostics",
        "-p",
        "cf",
        "--bin",
        "cf",
    ],
    [
        "cargo",
        "build",
        "--release",
        "--locked",
        "--offline",
        "--message-format=json-render-diagnostics",
        "-p",
        "cf-panes",
        "--bin",
        "consensflow-bridge",
    ],
];

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
    /// What its reporter's report says its tests did, or why it could not be read:
    /// none for a command that has no reporter.
    pub report: Option<Result<Results, String>>,
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

    /// Gives the commands it runs from now on `folder` as their temp folder. A plant
    /// makes tests fail on purpose, and a rig of the integration suites keeps a report
    /// of each case that fails (and leaves its home when its run was ended): in the
    /// machine's own temp folder those stay for good, in the run's they go with it.
    /// A runner that runs nothing has none to give.
    fn temp_in(&mut self, _folder: &Path) {}

    /// What the checkout is now, as far as a command's run can tell: the same text
    /// where nothing of it changed, whatever wrote it. How a command did without a
    /// plant is known in the state it ran in alone. `None` is a stop, heard while it
    /// was asked.
    fn state(&mut self) -> Option<String>;

    /// The checkout's root, where git says nothing of it: what a run changes in it cannot
    /// be told, and no plant is tried there (the driver refuses).
    fn gitless(&mut self) -> Option<PathBuf> {
        None
    }
}

impl Ran {
    /// A run of `command` that ended with `code`, wrote `output` and reported `report`.
    #[cfg(test)]
    pub fn new(
        command: &[&str],
        code: Option<i32>,
        output: &str,
        report: Option<Result<Results, String>>,
    ) -> Self {
        Self {
            command: command.iter().map(|word| (*word).to_owned()).collect(),
            code,
            hung: false,
            overflowed: false,
            output: output.to_owned(),
            report,
        }
    }

    /// Whether the source built, which nextest says by its status where it did not.
    /// A run of Node or Playwright builds nothing.
    pub fn compiled(&self) -> bool {
        Reporter::of(&self.command) != Reporter::Nextest || self.code != Some(NEXTEST_BUILD_FAILED)
    }

    /// Why no test of the run could see the plant, if none could, as its reporter
    /// says: no report it could read whole; a test file of Node's that failed whole
    /// and ran no test of its own (it did not parse, imported what is not there,
    /// threw as it loaded, after a top-level `await` too, or ended before a test of
    /// it was told), whatever the other files did; an error Playwright says came
    /// outside every test (a hook of a test that failed too), or no test it found; no
    /// test nextest found to run, a test it could not start, or a test binary that
    /// could not say its tests; a run that failed with no test run
    /// at all; and a run that failed while its report names no test that did, which
    /// no test's own failure made fail (a test cancelled or aborted, a hook that
    /// failed, a Node test file that failed whole after tests of it ran, an error
    /// after a test had ended). A test file that loaded and holds no test is none of
    /// these.
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
        let results = match self.report.as_ref()? {
            Ok(results) => results,
            Err(why) => return Some(format!("no report of its tests could be read: {why}")),
        };
        let none_ran = results.tests.is_empty() && self.code != Some(0);
        let why = match reporter {
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
            Reporter::Nextest => match results.outside.first() {
                Some(error) => Some(format!("{error}: it never ran")),
                None => none_ran.then(|| "it ran no test".to_owned()),
            },
            Reporter::Status => None,
        };
        why.or_else(|| self.unnamed(results))
    }

    /// Why the run failed with no failure of a test's own, where its report names no
    /// test that failed: how it ended, the test files that failed whole, and the tests
    /// that neither failed nor passed.
    fn unnamed(&self, results: &Results) -> Option<String> {
        if self.code == Some(0) || self.names_tests() {
            return None;
        }
        let mut why = match self.code {
            Some(code) => format!("it ended with {code}, and its report names no test that failed"),
            None => "a signal ended it, and its report names no test that failed".to_owned(),
        };
        for whole in results
            .tests
            .iter()
            .filter(|test| test.whole && test.failed)
        {
            why.push_str(&format!("; the test file {} failed whole", whole.name));
        }
        if !results.others.is_empty() {
            let names: Vec<&str> = results
                .others
                .iter()
                .map(|test| test.name.as_str())
                .collect();
            why.push_str(&format!(
                "; neither failed nor passed (skipped, to do, cancelled): {}",
                names.join(", ")
            ));
        }
        Some(why)
    }

    /// Whether the run's failure names a test of its own, which a plant can be
    /// attributed by: a test file that failed whole names none.
    pub fn names_tests(&self) -> bool {
        self.tests().iter().any(|test| test.failed && !test.whole)
    }

    /// Every test the run's report says ran, failed or not: none where it has no
    /// report it could read.
    pub fn tests(&self) -> &[Test] {
        match &self.report {
            Some(Ok(results)) => &results.tests,
            _ => &[],
        }
    }

    /// Every test the run's report says was registered and neither failed nor passed
    /// (`Results::others`): none where it has no report it could read.
    pub fn others(&self) -> &[Test] {
        match &self.report {
            Some(Ok(results)) => &results.others,
            _ => &[],
        }
    }

    /// What caught the plant, if this run did, which only a run that exited with a
    /// failure can. For a run that has a reporter, the tests its report says failed,
    /// each once, in the order it says them, and nothing else: a failure its report
    /// names no test of is no test's catch (`broken`), and a run whose reporter wrote
    /// no report it could read catches nothing. For a command that has none, the run
    /// itself, where it was not ended for its time or its output.
    pub fn caught(&self) -> Vec<String> {
        if self.code == Some(0) {
            return Vec::new();
        }
        match &self.report {
            Some(Ok(results)) => results
                .failed()
                .into_iter()
                .map(|test| test.name.clone())
                .collect(),
            Some(Err(_)) => Vec::new(),
            None if self.hung || self.overflowed => Vec::new(),
            None => {
                let words = self.command.get(1..).unwrap_or_default().join(" ");
                vec![match self.code {
                    Some(code) => format!("{words}: ended with {code}"),
                    None => format!("{words}: was ended by a signal"),
                }]
            }
        }
    }

    /// Whether cargo's builds succeeded, where it said so in its structured words.
    pub fn build_finished(&self) -> Option<bool> {
        self.output
            .lines()
            .filter(|line| line.starts_with(r#"{"reason":"build-finished""#))
            .map(|line| line.contains(r#""success":true"#))
            .reduce(|all, built| all && built)
    }

    /// Where cargo put the program `bin` it built, built now or already, as it says
    /// in its structured words (`compiler-artifact`), the last time it says it: none
    /// where it does not.
    pub fn executable(&self, bin: &str) -> Option<PathBuf> {
        self.output
            .lines()
            .filter(|line| line.starts_with(r#"{"reason":"compiler-artifact""#))
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .filter(|artifact| {
                artifact["target"]["name"] == bin
                    && artifact["target"]["kind"]
                        .as_array()
                        .is_some_and(|kinds| kinds.iter().any(|kind| kind == "bin"))
            })
            .filter_map(|artifact| artifact["executable"].as_str().map(PathBuf::from))
            .next_back()
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

/// Runs the commands as processes, within a limit of time, through `cf-process`.
pub struct Execute {
    runtime: Runtime,
    /// The checkout, and the environment the commands are given.
    checkout: Context,
    own: PathBuf,
    limits: Limits,
    stops: Stops,
    /// How many states could not be told, each told apart from every other.
    untold: u64,
    /// The builds before a run of cf-e2e's tests (`CF_E2E_PROGRAMS`).
    programs: Vec<Vec<String>>,
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
                // Only a run of cf-e2e's tests is told where its programs were built.
                .filter(|(name, _)| *name != OsStr::new(CF_E2E_BUILT))
                .filter(|(name, _)| !colour.contains(name))
                .map(|(name, value)| (name.to_owned(), value.to_owned()))
                .chain([
                    (OsString::from("NO_COLOR"), OsString::from("1")),
                    (OsString::from("FORCE_COLOR"), OsString::from("0")),
                ]),
        );
        Self {
            runtime,
            checkout: Context {
                root: context.root.clone(),
                env,
            },
            own,
            limits: Limits {
                timeout: limit,
                max_buffer: OUTPUT_LIMIT,
            },
            stops,
            untold: 0,
            programs: CF_E2E_PROGRAMS
                .iter()
                .map(|build| build.iter().map(|word| (*word).to_owned()).collect())
                .collect(),
        }
    }

    /// What `git` says to `words` in the checkout, where it says it and succeeds: the
    /// outer `None` is a stop, heard while it ran, which is the driver's to hear.
    fn git(&mut self, words: &[&str]) -> Option<Option<String>> {
        let command: Vec<String> = ["git"]
            .iter()
            .chain(words)
            .map(|word| (*word).to_owned())
            .collect();
        match self.run(&command) {
            Ok(Execution::Stopped) => None,
            Ok(Execution::Ended(ran)) if ran.code == Some(0) => Some(Some(ran.output)),
            _ => Some(None),
        }
    }

    /// A folder for a run's report, under `RUNS`, removed once dropped: one of its own
    /// for each run, whatever else runs beside it.
    fn folder(&self) -> Result<tempfile::TempDir, Error> {
        let runs = self.checkout.root.join(RUNS);
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
        let installed = self.checkout.root.join(TOOLS).join("bin").join(name);
        if installed.is_file() {
            return Ok(installed.display().to_string());
        }
        let found = process::resolved(OsStr::new("cargo-nextest"), &self.checkout.env);
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

    /// Builds the programs the tests of `cf-e2e` run (`CF_E2E_PROGRAMS`), before a run
    /// of `command`: the folder they are in, where cargo says it put each program a
    /// build names (`--bin`), whatever folder it builds in (`CARGO_TARGET_DIR`, its
    /// config); or, where a build did not succeed, how the run ends: stopped; the
    /// build itself, where its time ran out; else as `command` with the build's
    /// status and words and no test run, which a build cargo says failed is one that
    /// did not build. Builds that put the programs in two folders, or that say of
    /// none where they put it, are no run of the tests either.
    fn programs(&mut self, command: &[String]) -> Result<Result<PathBuf, Execution>, Error> {
        let mut folder: Option<PathBuf> = None;
        for words in self.programs.clone() {
            let ran = match self.run(&words)? {
                Execution::Stopped => return Ok(Err(Execution::Stopped)),
                Execution::Ended(ran) if ran.code == Some(0) => {
                    let bin = words.iter().skip_while(|word| *word != "--bin").nth(1);
                    let Some(bin) = bin else { continue };
                    let built = ran.executable(bin);
                    let Some(there) = built.as_deref().and_then(Path::parent) else {
                        let why = format!("{} said nowhere it put {bin}", words.join(" "));
                        return Ok(Err(Execution::Ended(Self::unbuilt(command, ran, why))));
                    };
                    if let Some(other) = folder.as_ref().filter(|other| *other != there) {
                        let why = format!(
                            "{} put {bin} in {}, and the build before it in {}",
                            words.join(" "),
                            there.display(),
                            other.display()
                        );
                        return Ok(Err(Execution::Ended(Self::unbuilt(command, ran, why))));
                    }
                    folder = Some(there.to_path_buf());
                    continue;
                }
                // Ended for its time or its output, it is said as the build it is.
                Execution::Ended(ran) if ran.hung || ran.overflowed => {
                    return Ok(Err(Execution::Ended(ran)))
                }
                Execution::Ended(ran) => ran,
            };
            let code = if ran.build_finished() == Some(false) {
                Some(NEXTEST_BUILD_FAILED)
            } else {
                ran.code
            };
            let why = format!("{} did not succeed", words.join(" "));
            return Ok(Err(Execution::Ended(Self::unbuilt(
                command,
                Ran { code, ..ran },
                why,
            ))));
        }
        Ok(match folder {
            Some(folder) => Ok(folder),
            None => Err(Execution::Ended(Ran {
                command: command.to_vec(),
                code: None,
                hung: false,
                overflowed: false,
                output: String::new(),
                report: Some(Err(
                    "no build before it named a program it built, and no test was run".to_owned(),
                )),
            })),
        })
    }

    /// A run of `command` that ran no test, the build `ran` before it having come to
    /// `why`: its status, its words and its end are the build's.
    fn unbuilt(command: &[String], ran: Ran, why: String) -> Ran {
        Ran {
            command: command.to_vec(),
            report: Some(Err(format!("{why}, and no test was run"))),
            ..ran
        }
    }

    /// The program to start for `invocation`, and its words: `cargo xtask` is this xtask.
    fn start(&self, invocation: &Invocation) -> Result<Run, Error> {
        let (program, args): (OsString, &[OsString]) = match invocation.args.as_slice() {
            [xtask, args @ ..] if invocation.program == "cargo" && xtask == "xtask" => {
                (self.own.clone().into_os_string(), args)
            }
            args => (
                process::resolved(&invocation.program, &self.checkout.env),
                args,
            ),
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
            args: args.to_vec(),
            verbatim: false,
        })
    }
}

/// Why `command` cannot be run in the checkout `context`, if it cannot: an option of
/// `cargo test` that nextest has no word for (`reporters`).
pub fn refused(command: &[String], context: &Context) -> Option<String> {
    Reporter::of(command).refused(command, context)
}

impl Runner for Execute {
    fn temp_in(&mut self, folder: &Path) {
        let kept = self
            .checkout
            .env
            .iter()
            .filter(|(name, _)| !TEMP_FOLDER.iter().any(|temp| *name == OsStr::new(temp)))
            .map(|(name, value)| (name.to_owned(), value.to_owned()))
            .collect::<Vec<_>>();
        let given = TEMP_FOLDER
            .iter()
            .map(|name| (OsString::from(name), folder.as_os_str().to_owned()));
        self.checkout.env = Env::from_vars(kept.into_iter().chain(given));
    }

    fn run(&mut self, command: &[String]) -> Result<Execution, Error> {
        let reporter = Reporter::of(command);
        if let Some(why) = reporter.refused(command, &self.checkout) {
            return Err(process::Failure::Refused {
                program: command.join(" "),
                cause: std::io::Error::other(why),
            }
            .into());
        }
        // The programs the tests of cf-e2e run, built once for the run, before it: each
        // of its tests is a process of its own, which would build them for itself.
        let built = if reporter.runs_cf_e2e(command, &self.checkout) {
            match self.programs(command)? {
                Ok(folder) => Some(folder),
                Err(ended) => return Ok(ended),
            }
        } else {
            None
        };
        // A folder of the run's own, gone with it, for a reporter that writes one.
        let folder = match reporter {
            Reporter::Status => None,
            _ => Some(self.folder()?),
        };
        let dir = folder.as_ref().map_or_else(
            || self.checkout.root.clone(),
            |folder| folder.path().to_path_buf(),
        );
        let dir = dir.as_path();
        let nextest = match reporter {
            Reporter::Nextest => self.nextest()?,
            _ => String::new(),
        };
        let invocation = reporter
            .setup(command, dir, &self.checkout, &nextest)
            .map_err(Error::files("write the config of a run in", dir))?;
        let invocation = match built {
            Some(folder) => invocation.var(CF_E2E_BUILT, folder),
            None => invocation,
        };
        let run = self.start(&invocation)?;
        // The environment, with the variables the invocation sets or takes away.
        let given = |name: &OsStr| invocation.vars.iter().any(|(set, _)| set == name);
        let env = Env::from_vars(
            self.checkout
                .env
                .iter()
                .filter(|(name, _)| !given(name))
                .map(|(name, value)| (name.to_owned(), value.to_owned()))
                .chain(
                    invocation
                        .vars
                        .iter()
                        .filter_map(|(name, value)| Some((name.clone(), value.clone()?))),
                ),
        );
        let Self {
            runtime,
            limits,
            stops,
            ..
        } = self;
        let cwd = invocation.cwd.as_path();
        let result = runtime.block_on(async {
            let mut running = pin!(capture_whole(&run, Some(cwd), &env, *limits, |_| {}));
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
                ran.report = reporter.results(dir);
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
    /// nothing this once (its index locked, say: no plant is tried in a checkout it says
    /// nothing of, `gitless`), a state of its own, told apart from every other: a state
    /// it cannot tell is never taken for one a command was known to pass in. A stop
    /// heard while git is asked is none.
    fn state(&mut self) -> Option<String> {
        let head = self.git(&["rev-parse", "HEAD"])?;
        let status = self.git(&["status", "--porcelain=v1", "-z", "--untracked-files=all"])?;
        let (Some(head), Some(status)) = (head, status) else {
            self.untold += 1;
            return Some(format!("untold {}", self.untold));
        };
        let mut hasher = DefaultHasher::new();
        head.hash(&mut hasher);
        status.hash(&mut hasher);
        for path in changed(&status) {
            std::fs::read(self.checkout.root.join(path))
                .ok()
                .hash(&mut hasher);
        }
        Some(format!("{:016x}", hasher.finish()))
    }

    /// The checkout's root where git says nothing of it: no commit there to tell a state
    /// from (`state`). A stop heard while it is asked is the driver's to hear, which
    /// says nothing here.
    fn gitless(&mut self) -> Option<PathBuf> {
        match self.git(&["rev-parse", "HEAD"]) {
            Some(None) => Some(self.checkout.root.clone()),
            Some(Some(_)) | None => None,
        }
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
            // What it started did not end: the run is not over, and no verdict.
            Err(failed) if failed.message.starts_with(LEFT_RUNNING) => {
                return Err(Error::LeftRunning {
                    command: command.join(" "),
                    said: failed.message,
                });
            }
            Err(failed) => {
                let overflowed = failed.message.ends_with("maxBuffer length exceeded");
                // Ended for its time, it exited with no code of its own, whatever it
                // exited with once asked to end (`capture_whole`).
                let code = if failed.killed { None } else { failed.code };
                (
                    code,
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
            report: None,
        })
    }
}

#[cfg(test)]
mod tests;

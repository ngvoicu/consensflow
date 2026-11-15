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

mod execution;
mod outcome;
mod reporters;
pub(super) mod results;

use std::path::{Path, PathBuf};

pub use self::execution::Execute;
pub use self::outcome::Ran;
use self::reporters::Reporter;
use super::Error;
use crate::context::Context;

#[cfg(test)]
use self::results::Results;
#[cfg(test)]
use crate::process::{self, Invocation};
#[cfg(test)]
use crate::stops::Stops;
#[cfg(test)]
use cf_base::env::Env;
#[cfg(test)]
use cf_process::{CaptureFailed, Captured, LEFT_RUNNING};
#[cfg(test)]
use std::ffi::{OsStr, OsString};
#[cfg(test)]
use std::fs;

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

/// How the runner ends the reason of a run that ran no test because a build before it failed.
const NONE_RAN: &str = ", and no test was run";

/// How many of the last lines of a run are said when its report cannot be read, and how many
/// characters of them.
const ENDED_LINES: usize = 6;
const ENDED_CHARS: usize = 900;

/// The status nextest ends with where the build failed.
const NEXTEST_BUILD_FAILED: i32 = 101;

/// The status nextest ends with where no test was there to run.
const NEXTEST_NO_TESTS: i32 = 4;

/// The status nextest ends with where a test binary could not say its tests.
const NEXTEST_UNLISTED: i32 = 104;

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

    /// Why the machine lacks the harness `harness` a plant's runs need, in words
    /// ([`super::kit::lacks`]): none where it has it, as a runner that runs nothing
    /// says.
    fn lacks(&mut self, _harness: &str) -> Option<String> {
        None
    }

    /// What is said of `command` that takes no threads (a Node or Playwright run, a
    /// script): that the threads the driver was asked for have no effect on it. For a
    /// command a plants table names under `[run]`, never for the driver's own, at
    /// most once a run of the driver.
    fn say_no_effect(&mut self, _command: &[String]) {}
}

/// Why `command` cannot be run in the checkout `context`, if it cannot: an option of
/// `cargo test` that nextest has no word for (`reporters`).
pub fn refused(command: &[String], context: &Context) -> Option<String> {
    Reporter::of(command).refused(command, context)
}

#[cfg(test)]
mod tests;

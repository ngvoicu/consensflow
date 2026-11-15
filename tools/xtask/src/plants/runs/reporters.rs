//! Who reports a run's tests, how its command is run so that the report lands in a
//! file of the run's own, and what that file says (`results`): attribution reads it
//! alone, never what the run printed.
//!
//! - `cargo test …` runs under nextest (`cargo nextest run`, its options as nextest
//!   names what cargo means by them: `nextest_words`), which runs each test in a
//!   process of its own and writes a JUnit report: no test's output can pass for
//!   another's result. Its exit status says that the build failed (101), or that no
//!   test was there to run (4). A run of the tests of `cf-e2e` is given the
//!   programs they run built before it (`names_cf_e2e` says why).
//! - A suite of xtask's that is a `cargo test` (`cargo xtask app test …`, `cargo
//!   xtask test <suite> …`, and the packaged smoke, `cargo xtask smoke …`, on a
//!   Mac) is that `cargo test`, as the suite makes it (its folder, its variables,
//!   its words), run under nextest the same way. The smoke's is without the
//!   `--nocapture` that a person's run has: nextest keeps what its test wrote.
//! - `node --test …` writes the report of the plants' own reporter of Node's tests
//!   (`reporters/node.mjs`, written into the run's folder) beside the report it
//!   prints, its tests run in Node's own process (`node_words` says why, and what
//!   it costs).
//! - Playwright's CLI writes the report of the plants' own reporter of its tests
//!   (`reporters/playwright.mjs`, written into the run's folder) beside the list it
//!   prints: its JSON report says no step of a test, and a test whose `beforeAll`
//!   threw is `unexpected` there though its body never ran, where the reporter's
//!   steps say which hook threw (Playwright 1.63, 2026-10-17). Each test is given one
//!   attempt (`--retries=0`) and run once (`--repeat-each=1`), and a run that asks for
//!   more attempts is refused; a group that
//!   asks for retries of its own is given them over that, and the reporter says a
//!   test run more than once (Sol's look at round 7).
//! - Anything else, every other `cargo xtask …` among them, says no more than its
//!   exit status.

mod nextest;
mod node;
mod playwright;

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use self::nextest::{cargo_test, names_cf_e2e, nextest_words, take_out_threads, uncaptured, Suite};
use super::super::{threads_value, THREADS_VAR};
use super::results::{self, Results};
use crate::context::Context;
use crate::process::Invocation;

#[cfg(test)]
use self::nextest::{nextest_config, os, with_the_threads_of_the_environment};
#[cfg(test)]
use self::node::{node_words, NODE_REPORTER};
#[cfg(test)]
use self::playwright::{playwright_words, retried, PLAYWRIGHT_REPORTER, PLAYWRIGHT_REPORT_VAR};

/// What reads a report of one kind.
type Reader = fn(&str) -> Result<Results, String>;

/// The threads the plants driver was asked for (`CF_PLANTS_TEST_THREADS`), as nextest
/// takes them: none where neither flag nor variable asked, and a variable of no number
/// of threads is none.
pub(super) fn caller_threads(context: &Context) -> Option<String> {
    context
        .env
        .text(THREADS_VAR)
        .and_then(threads_value)
        .map(|threads| threads.to_string())
}

/// Whether `command` is a `cargo test` in the shape a person writes it.
pub(super) fn is_cargo_test(command: &[String]) -> bool {
    matches!(command, [cargo, test, ..] if cargo == "cargo" && test == "test")
}

/// Who reports the tests of a command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Reporter {
    Nextest,
    Node,
    Playwright,
    Status,
}

impl Reporter {
    /// Who reports the tests of `command`.
    pub(super) fn of(command: &[String]) -> Self {
        match command {
            [cargo, test, ..] if cargo == "cargo" && test == "test" => Self::Nextest,
            [cargo, xtask, words @ ..] if cargo == "cargo" && xtask == "xtask" => {
                match Suite::of(words) {
                    Some(_) => Self::Nextest,
                    None => Self::Status,
                }
            }
            [node, rest @ ..] if node == "node" || node.ends_with("node.exe") => {
                if rest.iter().any(|word| word.contains("playwright")) {
                    Self::Playwright
                } else if rest.iter().any(|word| word == "--test") {
                    Self::Node
                } else {
                    Self::Status
                }
            }
            _ => Self::Status,
        }
    }

    /// How `command` is started in the checkout `context` for its report to land in
    /// `dir`, a folder under the checkout's root: the program as a person names it
    /// (`cargo xtask` for this xtask), its words, its folder and its variables.
    /// `nextest` is where nextest is, for a run of it.
    pub(super) fn setup(
        self,
        command: &[String],
        dir: &Path,
        context: &Context,
        nextest: &str,
    ) -> Result<Invocation, std::io::Error> {
        let as_written =
            || Invocation::new(command[0].clone(), &context.root).args(command[1..].iter());
        Ok(match self {
            Self::Nextest => nextest::setup(command, dir, context, nextest)?,
            Self::Node => node::setup(command, dir, context)?,
            Self::Playwright => playwright::setup(command, dir, context)?,
            Self::Status => {
                let mut invocation = as_written();
                // A `cargo test` that nextest is not running is given the threads after
                // its `--`, where the caller's number wins over a table's own.
                if is_cargo_test(command) {
                    if let Some(threads) = caller_threads(context) {
                        let before = invocation.args.len();
                        take_out_threads(&mut invocation.args, before);
                        if !invocation.args.contains(&OsString::from("--")) {
                            invocation.args.push(OsString::from("--"));
                        }
                        invocation
                            .args
                            .push(OsString::from(format!("--test-threads={threads}")));
                    }
                }
                invocation
            }
        })
    }

    /// Whether `command` runs tests of `cf-e2e` in the checkout `context`, which are
    /// given the programs they run built before the run (`names_cf_e2e`).
    pub(super) fn runs_cf_e2e(self, command: &[String], context: &Context) -> bool {
        self == Self::Nextest && names_cf_e2e(&cargo_test(command, context).args)
    }

    /// Why `command` cannot be run as its reporter runs it in the checkout `context`,
    /// if it cannot: an option of `cargo test` that nextest has no word for
    /// (`nextest_words`), or one that turns capture off (`uncaptured`); retries of
    /// Playwright's (`retried`).
    pub(super) fn refused(self, command: &[String], context: &Context) -> Option<String> {
        match self {
            Self::Nextest => {
                let tests = cargo_test(command, context);
                nextest_words(&tests.args)
                    .err()
                    .or_else(|| uncaptured(&tests))
            }
            Self::Playwright => playwright::retried(command),
            Self::Node | Self::Status => None,
        }
    }

    /// Whether `command` takes the threads the driver was asked for: a run under
    /// nextest, and a plain `cargo test` it is not under (given them after its `--`).
    pub(super) fn takes_threads(self, command: &[String]) -> bool {
        self == Self::Nextest || is_cargo_test(command)
    }

    /// What the report in `dir` says the run's tests did: none for a command that
    /// writes none, and an error for a report that is not there, cannot be read, or
    /// is not whole, which says why.
    pub(super) fn results(self, dir: &Path) -> Option<Result<Results, String>> {
        let (path, read): (PathBuf, Reader) = match self {
            Self::Nextest => (
                dir.join(nextest::NEXTEST_PROFILE).join("junit.xml"),
                results::junit,
            ),
            Self::Node => (dir.join(node::NODE_REPORT), results::node),
            Self::Playwright => (dir.join(playwright::PLAYWRIGHT_REPORT), results::playwright),
            Self::Status => return None,
        };
        Some(match fs::read_to_string(&path) {
            Ok(text) => read(&text).map_err(|why| format!("{}: {why}", path.display())),
            Err(cause) => Err(format!("{} could not be read: {cause}", path.display())),
        })
    }
}

/// `path`, under `root`, as a module of the run's is named to Node from there: a
/// relative URL, whatever the system's separator.
fn from_root(path: &Path, root: &Path) -> Result<String, std::io::Error> {
    let relative = path.strip_prefix(root).map_err(std::io::Error::other)?;
    let parts: Vec<String> = relative
        .components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect();
    Ok(format!("./{}", parts.join("/")))
}

#[cfg(test)]
mod tests;

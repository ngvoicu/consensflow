//! Who reports a run's tests, how its command is run so that the report lands in a
//! file of the run's own, and what that file says (`results`): attribution reads it
//! alone, never what the run printed.
//!
//! - `cargo test …` runs under nextest (`cargo nextest run`, the same arguments),
//!   which runs each test in a process of its own and writes a JUnit report: no
//!   test's output can pass for another's result. Its exit status says that the
//!   build failed (101), or that no test was there to run (4). The tests of
//!   `cf-e2e` run one at a time (`names_cf_e2e` says why).
//! - A suite of xtask's that is a `cargo test` (`cargo xtask app test …`, `cargo
//!   xtask test <suite> …`, and the packaged smoke, `cargo xtask smoke …`, on a
//!   Mac) is that `cargo test`, as the suite makes it (its folder, its variables,
//!   its words), run under nextest the same way.
//! - `node --test …` writes the report of the plants' own reporter of Node's tests
//!   (`reporters/node.mjs`, written into the run's folder) beside the report it
//!   prints.
//! - Playwright's CLI writes its JSON report beside the list it prints.
//! - Anything else, every other `cargo xtask …` among them, says no more than its
//!   exit status.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use super::results::{self, Results};
use crate::context::Context;
use crate::process::Invocation;

/// The profile of nextest the plants' runs use, which the run's own config file says.
const NEXTEST_PROFILE: &str = "plants";

/// The plants' reporter of Node's tests, and the files it is written to and writes.
const NODE_REPORTER: &str = include_str!("reporters/node.mjs");
const NODE_REPORTER_FILE: &str = "plants.mjs";
const NODE_REPORT: &str = "node.jsonl";

/// What reads a report of one kind.
type Reader = fn(&str) -> Result<Results, String>;

/// Who reports the tests of a command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Reporter {
    Nextest,
    Node,
    Playwright,
    Status,
}

/// A suite of xtask's that is a `cargo test` of its own, by the words after `cargo xtask`.
enum Suite<'a> {
    /// `app test …`: the app crate's tests.
    App(&'a [String]),
    /// `test <name> …`: a suite of `cf-e2e`'s.
    Named(crate::suites::Suite, &'a [String]),
    /// `smoke …`: the packaged smoke, on the app it names, if it names one.
    Smoke(Option<PathBuf>),
}

impl<'a> Suite<'a> {
    fn of(words: &'a [String]) -> Option<Self> {
        match words {
            [app, test, rest @ ..] if app == "app" && test == "test" => Some(Self::App(rest)),
            [test, name, rest @ ..] if test == "test" => {
                crate::suites::named(name).map(|suite| Self::Named(suite, rest))
            }
            // Elsewhere the command refuses, and says no more than that.
            [smoke, rest @ ..] if smoke == "smoke" && cfg!(target_os = "macos") => {
                crate::smoke::app_of(&os(rest)).ok().map(Self::Smoke)
            }
            _ => None,
        }
    }

    /// The `cargo test` the suite runs in the checkout `context`.
    fn invocation(&self, context: &Context) -> Invocation {
        match self {
            Self::App(rest) => crate::app::test(context, &os(rest)),
            Self::Named(suite, rest) => crate::suites::cargo_test(context, *suite, &os(rest)),
            Self::Smoke(app) => crate::smoke::invocation(context, app.as_deref()),
        }
    }
}

fn os(words: &[String]) -> Vec<OsString> {
    words.iter().map(OsString::from).collect()
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
            Self::Nextest => {
                let tests = match command {
                    [_, _, words @ ..] if command[1] == "xtask" => Suite::of(words)
                        .map(|suite| suite.invocation(context))
                        .unwrap_or_else(as_written),
                    _ => as_written(),
                };
                // nextest reads its own variables (`NEXTEST_TEST_THREADS`, `NEXTEST_RETRIES`)
                // over its config: none that this run was given, nor a nextest this runs
                // under, steers the run.
                let tests = context
                    .env
                    .iter()
                    .map(|(name, _)| name)
                    .filter(|name| name.to_string_lossy().starts_with("NEXTEST"))
                    .fold(tests, |tests, name| tests.without(name));
                let config = dir.join("nextest.toml");
                fs::write(&config, nextest_config(dir, names_cf_e2e(&tests.args)))?;
                under_nextest(tests, nextest, &config)
            }
            Self::Node => {
                fs::write(dir.join(NODE_REPORTER_FILE), NODE_REPORTER)?;
                let reporter = from_root(&dir.join(NODE_REPORTER_FILE), &context.root)?;
                Invocation::new(command[0].clone(), &context.root).args(node_words(
                    command,
                    &reporter,
                    &dir.join(NODE_REPORT),
                ))
            }
            Self::Playwright => Invocation::new(command[0].clone(), &context.root)
                .args(playwright_words(command).into_iter().skip(1))
                .var("PLAYWRIGHT_JSON_OUTPUT_FILE", dir.join("report.json")),
            Self::Status => as_written(),
        })
    }

    /// What the report in `dir` says the run's tests did: none for a command that
    /// writes none, and an error for a report that is not there, cannot be read, or
    /// is not whole, which says why.
    pub(super) fn results(self, dir: &Path) -> Option<Result<Results, String>> {
        let (path, read): (PathBuf, Reader) = match self {
            Self::Nextest => (dir.join(NEXTEST_PROFILE).join("junit.xml"), results::junit),
            Self::Node => (dir.join(NODE_REPORT), results::node),
            Self::Playwright => (dir.join("report.json"), results::playwright),
            Self::Status => return None,
        };
        Some(match fs::read_to_string(&path) {
            Ok(text) => read(&text).map_err(|why| format!("{}: {why}", path.display())),
            Err(cause) => Err(format!("{} could not be read: {cause}", path.display())),
        })
    }
}

/// The `cargo test` of `tests`, run by nextest at `nextest` with the config file
/// `config`: in its folder, with its variables, and its words after `test`.
fn under_nextest(tests: Invocation, nextest: &str, config: &Path) -> Invocation {
    let words = tests.args.iter().skip(1).cloned();
    Invocation {
        program: OsString::from(nextest),
        args: [
            "nextest",
            "run",
            "--profile",
            NEXTEST_PROFILE,
            "--config-file",
        ]
        .into_iter()
        .map(OsString::from)
        .chain([config.as_os_str().to_owned()])
        .chain(words)
        .collect(),
        cwd: tests.cwd,
        vars: tests.vars,
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

/// Whether the cargo words `words` name the package `cf-e2e`, before a `--`: each
/// process of one of its tests builds the `cf` it runs, once for the process, and
/// cargo links the binary again on every build, up to date or not, so that one
/// test's build takes away the `cf` another test is starting where they run beside
/// each other ("could not start …/target/release/cf: No such file or directory",
/// seen 2026-10-16). Its tests run one at a time.
fn names_cf_e2e(words: &[OsString]) -> bool {
    let words: Vec<&OsString> = words.iter().take_while(|word| *word != "--").collect();
    words
        .windows(2)
        .any(|pair| (pair[0] == "-p" || pair[0] == "--package") && pair[1] == "cf-e2e")
        || words
            .iter()
            .any(|word| *word == "--package=cf-e2e" || *word == "-pcf-e2e")
}

/// nextest's config for a run whose report lands in `dir`: every test run once, all
/// of them whatever fails, one at a time where `one_at_a_time` (`names_cf_e2e`),
/// and the JUnit report at `<dir>/plants/junit.xml`.
fn nextest_config(dir: &Path, one_at_a_time: bool) -> String {
    let store = dir.display().to_string().replace('\\', "/");
    let threads = if one_at_a_time {
        "test-threads = 1\n"
    } else {
        ""
    };
    format!(
        "[store]\ndir = \"{store}\"\n\n[profile.{NEXTEST_PROFILE}]\nretries = 0\nfail-fast = false\n{threads}\n\
         [profile.{NEXTEST_PROFILE}.junit]\npath = \"junit.xml\"\n"
    )
}

/// The words after the program of a `node --test` that also has the plants'
/// reporter (the module `reporter`) write its report to `report`: the report it
/// prints is the one the command asked for, the spec report where none.
fn node_words(command: &[String], reporter: &str, report: &Path) -> Vec<String> {
    let reporting = |word: &String| {
        word.starts_with("--test-reporter=") || word.starts_with("--test-reporter-destination=")
    };
    let printed = command
        .iter()
        .find_map(|word| word.strip_prefix("--test-reporter="))
        .unwrap_or("spec");
    let mut words = vec![
        format!("--test-reporter={printed}"),
        "--test-reporter-destination=stdout".to_owned(),
        format!("--test-reporter={reporter}"),
        format!("--test-reporter-destination={}", report.display()),
    ];
    words.extend(
        command
            .iter()
            .skip(1)
            .filter(|word| !reporting(word))
            .cloned(),
    );
    words
}

/// The words of a Playwright run that also writes its JSON report: the list it
/// prints and the JSON report, whatever reporter the command named.
fn playwright_words(command: &[String]) -> Vec<String> {
    let mut words: Vec<String> = command
        .iter()
        .filter(|word| !word.starts_with("--reporter="))
        .cloned()
        .collect();
    let test = words
        .iter()
        .position(|word| word == "test")
        .map_or(words.len(), |at| at + 1);
    words.insert(test, "--reporter=list,json".to_owned());
    words
}

#[cfg(test)]
mod tests;

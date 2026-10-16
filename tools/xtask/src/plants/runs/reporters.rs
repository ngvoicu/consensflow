//! Who reports a run's tests, how its command is run so that the report lands in a
//! file of the run's own, and what that file says (`results`): attribution reads it
//! alone, never what the run printed.
//!
//! - `cargo test …` runs under nextest (`cargo nextest run`, the same arguments),
//!   which runs each test in a process of its own and writes a JUnit report: no
//!   test's output can pass for another's result. Its exit status says that the
//!   build failed (101), or that no test was there to run (4).
//! - `cargo xtask …` runs a suite of xtask's, which starts `cargo test` itself:
//!   cargo starts each test binary through `cargo xtask plants-log` (its target
//!   runner), which asks libtest to write its log (`--logfile`) for the binary.
//!   libtest warns that `--logfile` is deprecated: once it is gone, these runs say
//!   nothing of their tests, and their failures are no catch by a test's name.
//! - `node --test …` writes a JUnit report beside the report it prints.
//! - Playwright's CLI writes its JSON report beside the list it prints.
//! - Anything else says no more than its exit status.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use super::results::{self, Results};

/// The profile of nextest the plants' runs use, which the run's own config file says.
const NEXTEST_PROFILE: &str = "plants";

/// Who reports the tests of a command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Reporter {
    Nextest,
    Libtest,
    Node,
    Playwright,
    Status,
}

/// How a command is started for its report to land in the run's folder.
pub(super) struct Setup {
    /// Its words, the program first: `cargo-nextest` for nextest, the command's own else.
    pub words: Vec<String>,
    /// What its environment holds more than the runner's.
    pub env: Vec<(OsString, OsString)>,
}

impl Reporter {
    /// Who reports the tests of `command`.
    pub(super) fn of(command: &[String]) -> Self {
        match command {
            [cargo, test, ..] if cargo == "cargo" && test == "test" => Self::Nextest,
            [cargo, xtask, ..] if cargo == "cargo" && xtask == "xtask" => Self::Libtest,
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

    /// How `command` is started for its report to land in `dir`: nextest is
    /// `nextest`, and cargo's target runner `runner`, where they are needed.
    pub(super) fn setup(
        self,
        command: &[String],
        dir: &Path,
        nextest: &str,
        runner: Option<(&str, &str)>,
    ) -> Result<Setup, std::io::Error> {
        let words = |words: Vec<String>| Setup {
            words,
            env: Vec::new(),
        };
        Ok(match self {
            Self::Nextest => {
                let config = dir.join("nextest.toml");
                fs::write(&config, nextest_config(dir))?;
                words(
                    [
                        nextest.to_owned(),
                        "nextest".to_owned(),
                        "run".to_owned(),
                        "--profile".to_owned(),
                        NEXTEST_PROFILE.to_owned(),
                        "--config-file".to_owned(),
                        config.display().to_string(),
                    ]
                    .into_iter()
                    .chain(command.iter().skip(2).cloned())
                    .collect(),
                )
            }
            Self::Libtest => Setup {
                words: command.to_vec(),
                env: runner
                    .map(|(name, own)| {
                        let value = format!("{own} plants-log {}", dir.display());
                        (OsString::from(name), OsString::from(value))
                    })
                    .into_iter()
                    .collect(),
            },
            Self::Node => words(node_words(command, &dir.join("junit.xml"))),
            Self::Playwright => Setup {
                words: playwright_words(command),
                env: vec![(
                    OsString::from("PLAYWRIGHT_JSON_OUTPUT_FILE"),
                    dir.join("report.json").into_os_string(),
                )],
            },
            Self::Status => words(command.to_vec()),
        })
    }

    /// What the report in `dir` says the run's tests did: none where it wrote none.
    pub(super) fn results(self, dir: &Path) -> Option<Result<Results, String>> {
        let read = |path: PathBuf| fs::read_to_string(path).ok();
        match self {
            Self::Nextest => read(dir.join(NEXTEST_PROFILE).join("junit.xml"))
                .map(|xml| results::junit(&xml, false)),
            Self::Libtest => {
                let mut logs: Vec<PathBuf> = fs::read_dir(dir)
                    .ok()?
                    .flatten()
                    .map(|entry| entry.path())
                    .filter(|path| path.extension().is_some_and(|extension| extension == "log"))
                    .collect();
                logs.sort();
                let logs: Vec<String> = logs.into_iter().filter_map(read).collect();
                (!logs.is_empty()).then(|| Ok(results::libtest_logs(&logs)))
            }
            Self::Node => read(dir.join("junit.xml")).map(|xml| results::junit(&xml, true)),
            Self::Playwright => {
                read(dir.join("report.json")).map(|json| results::playwright(&json))
            }
            Self::Status => None,
        }
    }
}

/// nextest's config for a run whose report lands in `dir`: every test run once, all
/// of them whatever fails, and the JUnit report at `<dir>/plants/junit.xml`.
fn nextest_config(dir: &Path) -> String {
    let store = dir.display().to_string().replace('\\', "/");
    format!(
        "[store]\ndir = \"{store}\"\n\n[profile.{NEXTEST_PROFILE}]\nretries = 0\nfail-fast = false\n\n\
         [profile.{NEXTEST_PROFILE}.junit]\npath = \"junit.xml\"\n"
    )
}

/// The words of a `node --test` that also writes its JUnit report to `junit`: the
/// report it prints is the one the command asked for, the spec report where none.
fn node_words(command: &[String], junit: &Path) -> Vec<String> {
    let reporter = |word: &String| {
        word.starts_with("--test-reporter=") || word.starts_with("--test-reporter-destination=")
    };
    let printed = command
        .iter()
        .find_map(|word| word.strip_prefix("--test-reporter="))
        .unwrap_or("spec");
    let mut words = vec![command[0].clone()];
    words.extend([
        format!("--test-reporter={printed}"),
        "--test-reporter-destination=stdout".to_owned(),
        "--test-reporter=junit".to_owned(),
        format!("--test-reporter-destination={}", junit.display()),
    ]);
    words.extend(
        command
            .iter()
            .skip(1)
            .filter(|word| !reporter(word))
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

//! Interpret a captured command and the results its reporter wrote.

use std::path::PathBuf;

use cf_process::{CaptureFailed, Captured, LEFT_RUNNING};

use super::reporters::Reporter;
use super::results::{Results, Test};
use super::{
    Error, ENDED_CHARS, ENDED_LINES, NEXTEST_BUILD_FAILED, NEXTEST_NO_TESTS, NEXTEST_UNLISTED,
    NONE_RAN,
};
use crate::process;

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

    /// Why a run left no report of its tests to read, as a run without the plant is told:
    /// the report's own words, and how the run ended and the end of what it wrote, the only
    /// word there is of why its tests did not run (a nextest that ended with 102 in two
    /// seconds because its `cargo metadata --offline` lacked a crate, zeewin2, T-627: the
    /// path of its report was only where it would have been). A reason the runner gave itself
    /// (a build that failed) says why already.
    pub fn unread(&self) -> Option<String> {
        let Some(Err(why)) = &self.report else {
            return None;
        };
        let said = format!("no report of its tests could be read: {why}");
        Some(if why.ends_with(NONE_RAN) {
            said
        } else {
            format!("{said}; {}", self.ended())
        })
    }

    /// How the run ended, and the end of what it wrote.
    fn ended(&self) -> String {
        let how = match self.code {
            Some(code) => format!("it ended with {code}"),
            None => "a signal ended it".to_owned(),
        };
        let lines: Vec<&str> = self
            .output
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect();
        let end = lines[lines.len().saturating_sub(ENDED_LINES)..].join(" / ");
        if end.is_empty() {
            return format!("{how}, saying nothing");
        }
        let end: String = end.chars().take(ENDED_CHARS).collect();
        format!("{how}, saying (its last lines): {end}")
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

impl Ran {
    /// What `command` did, from what `cf-process` answered of it: a stream that said
    /// more than a run may is `cf-process`'s own words for it.
    pub(super) fn of(
        command: &[String],
        done: Result<Captured, CaptureFailed>,
    ) -> Result<Self, Error> {
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

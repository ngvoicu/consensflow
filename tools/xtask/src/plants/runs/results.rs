//! What a run's tests did, as its reporter wrote it to a file of its own
//! (`reporters` says which file, and how the command is asked to write it): never
//! what the run printed, which a test may make say anything.
//!
//! - nextest's JUnit report: a `<testcase>` for each test, failed where it holds a
//!   `<failure>` or an `<error>`, and by its test binary (`classname`); one nextest
//!   could not start is none that ran (`junit`).
//! - Node's report, which the plants' own reporter of Node's tests writes
//!   (`reporters/node.mjs`): a line of JSON for each test registered that ended,
//!   named by its own title and those of the tests it is in, and how it ended; then
//!   a line that the run ended. A test file that failed whole is a test named by
//!   its path, which the line says it is.
//! - Playwright's report, which the plants' own reporter of Playwright's tests writes
//!   (`reporters/playwright.mjs`): every test registered and how it ended in each
//!   project, and whether its own body failed (a hook's or a fixture's failure is
//!   theirs), named from its first group on, one test however many projects ran it;
//!   the errors that came outside every test; that the run ended.
//!
//! A report is whole or it is an error: one cut short (no end to its root, or no
//! last line) says nothing of the tests it did not get to. A test that was skipped,
//! or marked to do (Node runs a test to do, and it may fail), neither failed nor
//! passed: it is none of the tests that ran, and is kept apart (`others`); so is a
//! test of Node's that ended for what is not its own code (cancelled by its parent,
//! aborted, its hook failed).
//!
//! Each test of `Results` is one test registered, whatever came of it: two of a
//! name and place are two tests, which the name cannot tell apart.

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use serde::Deserialize;

/// The tests a run's reporter says it ran, and the errors it says came outside them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Results {
    /// The tests that ran: each passed or failed.
    pub tests: Vec<Test>,
    /// Errors outside every test, each as its first line.
    pub outside: Vec<String>,
    /// The tests registered that neither failed nor passed (skipped, to do, ended
    /// for what is not their own code), none of them failed.
    pub others: Vec<Test>,
}

/// A test, as its reporter names it, and whether it failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Test {
    pub name: String,
    /// Where it is written, which tells it from another test of its name: its file
    /// and place (Node, Playwright), its test binary (nextest).
    pub at: String,
    /// The file it is written in, where the reporter says (Node).
    pub file: Option<String>,
    /// Whether it is a test file Node says ended whole, as a test of its own named
    /// by its path: none of the file's tests, nor a test of it.
    pub whole: bool,
    pub failed: bool,
}

impl Results {
    /// The tests that failed, in the order the reporter says them, each once.
    pub fn failed(&self) -> Vec<&Test> {
        let mut failed: Vec<&Test> = Vec::new();
        for test in self.tests.iter().filter(|test| test.failed) {
            if !failed.iter().any(|seen| seen.name == test.name) {
                failed.push(test);
            }
        }
        failed
    }

    /// The test files Node says failed whole and that ran no test of their own:
    /// none was loaded, or none was registered before the file ended. A file that
    /// loaded and holds no test did not fail.
    pub fn files_that_ran_none(&self) -> Vec<&str> {
        self.tests
            .iter()
            .filter(|whole| whole.whole && whole.failed)
            .filter(|whole| {
                !self
                    .tests
                    .iter()
                    .any(|test| !test.whole && test.file == whole.file)
            })
            .map(|whole| whole.name.as_str())
            .collect()
    }
}

/// What nextest 0.9.148 says of a test whose process it could not start: an `<error>`
/// of this type, where it starts the test itself (on Windows).
const EXECUTION_FAILURE: &str = "execution failure";

/// On Unix nextest starts each test through a process of its own (its double-spawn),
/// which execs the test binary: where that exec fails, nextest says the test failed
/// with this status, and the double-spawn says why on the test's standard error, which
/// begins so.
const DOUBLE_SPAWN_FAILED: (&str, &str) = (
    "test failure with exit code 70",
    "error: [double-spawn] failed to exec",
);

/// A `<testcase>` of nextest's JUnit report that is open, until its end.
struct Case {
    test: Test,
    skipped: bool,
    /// The type of its `<failure>` or `<error>`, where it holds one.
    failure: Option<String>,
    /// Whether its `<system-err>` is open, and its text as far as read.
    in_err: bool,
    err: String,
}

/// What nextest writes for a test's standard error where capture was off.
const NOT_CAPTURED: &str = "(stderr not captured)";

impl Case {
    /// Whether nothing proves nextest started the test: its own error that says it
    /// could not; or the double-spawn's status, where the test's standard error is the
    /// double-spawn's words, or was not kept to say otherwise (capture off, which says
    /// `NOT_CAPTURED`, or nothing written): a test that exits with 70 of itself and says
    /// nothing is no failure of its own to take (both reviewers' look at round 6).
    fn unstarted(&self) -> bool {
        match self.failure.as_deref() {
            Some(EXECUTION_FAILURE) => true,
            Some(failure) => {
                let err = self.err.trim();
                failure == DOUBLE_SPAWN_FAILED.0
                    && (err.is_empty()
                        || err == NOT_CAPTURED
                        || err.starts_with(DOUBLE_SPAWN_FAILED.1))
            }
            None => false,
        }
    }
}

/// The tests of nextest's JUnit report. A test nextest could not start (its binary is
/// not there to run, or cannot be run) never ran: it neither failed nor passed, and
/// that it could not be started is an error outside every test. It is the one thing
/// read of what a test wrote: the double-spawn's words on its standard error (Unix),
/// or none kept there (`Case::unstarted`), which can make a test's failure read as one
/// that never started, never the other way. A run with capture off is refused before
/// it starts (`reporters`).
pub fn junit(xml: &str) -> Result<Results, String> {
    let mut reader = Reader::from_str(xml);
    let mut results = Results::default();
    let mut open: Option<Case> = None;
    let mut ended = false;
    loop {
        match reader.read_event().map_err(|cause| cause.to_string())? {
            Event::Start(element) if element.name().as_ref() == b"testcase" => {
                open = Some(Case {
                    test: testcase(&element)?,
                    skipped: false,
                    failure: None,
                    in_err: false,
                    err: String::new(),
                });
            }
            Event::Empty(element) if element.name().as_ref() == b"testcase" => {
                results.tests.push(testcase(&element)?);
            }
            Event::Empty(element) if element.name().as_ref() == b"testsuites" => ended = true,
            Event::Start(element) | Event::Empty(element) => {
                if let Some(case) = open.as_mut() {
                    match element.name().as_ref() {
                        b"failure" | b"error" => {
                            case.test.failed = true;
                            case.failure = Some(attribute(&element, "type")?.unwrap_or_default());
                        }
                        b"skipped" => case.skipped = true,
                        b"system-err" => case.in_err = true,
                        _ => {}
                    }
                }
            }
            Event::Text(text) => {
                if let Some(case) = open.as_mut().filter(|case| case.in_err) {
                    case.err.push_str(&String::from_utf8_lossy(text.as_ref()));
                }
            }
            Event::End(element) if element.name().as_ref() == b"system-err" => {
                if let Some(case) = open.as_mut() {
                    case.in_err = false;
                }
            }
            Event::End(element) if element.name().as_ref() == b"testcase" => {
                let Some(case) = open.take() else { continue };
                let unstarted = case.unstarted();
                let test = Test {
                    failed: case.test.failed && !case.skipped && !unstarted,
                    ..case.test
                };
                if unstarted {
                    results.outside.push(format!(
                        "nextest could not start {} ({})",
                        test.name, test.at
                    ));
                }
                if case.skipped || unstarted {
                    results.others.push(test);
                } else {
                    results.tests.push(test);
                }
            }
            Event::End(element) if element.name().as_ref() == b"testsuites" => ended = true,
            Event::Eof => break,
            _ => {}
        }
    }
    if !ended {
        return Err("the report ends before its <testsuites> does".to_owned());
    }
    Ok(results)
}

/// The test a `<testcase>` names, not failed until a `<failure>` in it says so.
fn testcase(element: &BytesStart<'_>) -> Result<Test, String> {
    Ok(Test {
        name: attribute(element, "name")?.ok_or("a <testcase> with no name")?,
        at: attribute(element, "classname")?.unwrap_or_default(),
        file: None,
        whole: false,
        failed: false,
    })
}

/// The value of the attribute `key` of `element`, its entities read, where it has one.
fn attribute(element: &BytesStart<'_>, key: &str) -> Result<Option<String>, String> {
    let Some(found) = element
        .try_get_attribute(key)
        .map_err(|cause| cause.to_string())?
    else {
        return Ok(None);
    };
    let value = found
        .normalized_value(quick_xml::XmlVersion::Explicit1_0)
        .map_err(|cause| cause.to_string())?;
    Ok(Some(value.into_owned()))
}

/// A line of the plants' reporter of Node's tests: a test that ended, or the end.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NodeLine {
    #[serde(default)]
    end: bool,
    #[serde(default)]
    path: Vec<String>,
    file: Option<String>,
    line: Option<u64>,
    column: Option<u64>,
    #[serde(default)]
    whole: bool,
    ended: Option<Ended>,
}

/// How a test of Node's ended, as the plants' reporter says.
#[derive(Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum Ended {
    Passed,
    /// Its own code failed.
    Failed,
    Skipped,
    Todo,
    /// It ended for what is not its own code.
    Cancelled,
}

/// The tests of the report of the plants' reporter of Node's tests, each named by
/// the titles of the tests it is in and its own, joined as Playwright joins them.
pub fn node(report: &str) -> Result<Results, String> {
    let (mut tests, mut others) = (Vec::new(), Vec::new());
    let mut ended = false;
    for (number, text) in report.lines().enumerate() {
        let read: NodeLine =
            serde_json::from_str(text).map_err(|cause| format!("line {}: {cause}", number + 1))?;
        let test = !read.end && !read.path.is_empty() && read.ended.is_some();
        let end = read.end && read.path.is_empty() && read.ended.is_none();
        if ended || !(test || end) {
            return Err(format!("line {}: not a test, nor the end", number + 1));
        }
        ended = read.end;
        let Some(how) = read.ended else {
            continue;
        };
        let file = read.file.unwrap_or_default();
        let place = |at: Option<u64>| at.map(|at| at.to_string()).unwrap_or_default();
        let test = Test {
            name: read.path.join(" › "),
            at: format!("{file}:{}:{}", place(read.line), place(read.column)),
            file: Some(file),
            whole: read.whole,
            failed: how == Ended::Failed,
        };
        match how {
            Ended::Passed | Ended::Failed => tests.push(test),
            Ended::Skipped | Ended::Todo | Ended::Cancelled => others.push(test),
        }
    }
    if !ended {
        return Err("the report ends before the run did".to_owned());
    }
    Ok(Results {
        tests,
        outside: Vec::new(),
        others,
    })
}

/// The report of the plants' reporter of Playwright's tests.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PlaywrightReport {
    tests: Vec<PlaywrightTest>,
    errors: Vec<String>,
    end: bool,
}

/// A test of Playwright's in one project it was given to, as the plants' reporter says.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PlaywrightTest {
    path: Vec<String>,
    file: String,
    line: u64,
    column: u64,
    project: String,
    outcome: Outcome,
    /// Whether its own body failed, an assertion of it, and nothing else of its one
    /// attempt did.
    own: bool,
    /// Why it is no failure of its body's own, where it failed and is not, or was run
    /// more than once.
    why: Option<String>,
}

/// Playwright's last word on a test, across its retries.
#[derive(Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum Outcome {
    Expected,
    Unexpected,
    /// It failed, and passed when run again.
    Flaky,
    Skipped,
}

/// The tests of the report of the plants' reporter of Playwright's tests, and the
/// errors outside them: one test for each test written, however many projects ran it,
/// named by the titles of the groups it is in (the file's left out) and its own,
/// joined with ` › `. It failed where its own body failed in a project, as the reporter
/// proves it. It ran where its body ran in one (it passed, or failed of itself); else it
/// neither failed nor passed (`others`): skipped in every project it was given to, or
/// failed with no proof it was of itself (a hook that failed, a body beside it too, a
/// worker that exited, no assertion of its body that failed), which is an error outside
/// it, said with the reporter's why, as is a test run more than once, whatever came of
/// it in the end.
pub fn playwright(json: &str) -> Result<Results, String> {
    let report: PlaywrightReport = serde_json::from_str(json).map_err(|cause| cause.to_string())?;
    if !report.end {
        return Err("the report does not say the run ended".to_owned());
    }
    // Each test written, with what each project made of it, in the order said.
    let mut written: Vec<(Test, Vec<&PlaywrightTest>)> = Vec::new();
    for test in &report.tests {
        let name = test.path.join(" › ");
        let at = format!("{}:{}:{}", test.file, test.line, test.column);
        match written
            .iter_mut()
            .find(|(seen, _)| seen.name == name && seen.at == at)
        {
            Some((_, projects)) => projects.push(test),
            None => written.push((
                Test {
                    name,
                    at,
                    file: Some(test.file.clone()),
                    whole: false,
                    failed: false,
                },
                vec![test],
            )),
        }
    }
    let mut results = Results {
        outside: report.errors,
        ..Results::default()
    };
    for (test, projects) in written {
        let own = |project: &&PlaywrightTest| project.outcome == Outcome::Unexpected && project.own;
        let failed = projects.iter().any(own);
        let ran = failed
            || projects
                .iter()
                .any(|project| matches!(project.outcome, Outcome::Expected | Outcome::Flaky));
        for project in projects.iter().filter(|project| {
            (project.outcome == Outcome::Unexpected && !project.own) || project.why.is_some()
        }) {
            let place = if project.project.is_empty() {
                String::new()
            } else {
                format!(" in {}", project.project)
            };
            results.outside.push(format!(
                "{}{place} did not fail of itself: {}",
                test.name,
                project.why.as_deref().unwrap_or("it says no more")
            ));
        }
        let test = Test { failed, ..test };
        if ran {
            results.tests.push(test);
        } else {
            results.others.push(test);
        }
    }
    Ok(results)
}

#[cfg(test)]
mod tests;

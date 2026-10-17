//! What a run's tests did, as its reporter wrote it to a file of its own
//! (`reporters` says which file, and how the command is asked to write it): never
//! what the run printed, which a test may make say anything.
//!
//! - nextest's JUnit report: a `<testcase>` for each test, failed where it holds a
//!   `<failure>` or an `<error>`, and by its test binary (`classname`).
//! - Node's report, which the plants' own reporter of Node's tests writes
//!   (`reporters/node.mjs`): a line of JSON for each test registered that ended,
//!   named by its own title and those of the tests it is in, and how it ended; then
//!   a line that the run ended. A test file that failed whole is a test named by
//!   its path, which the line says it is.
//! - Playwright's JSON report: every test of every suite and its status in each
//!   project it ran in (`unexpected` is a failure), named from its first group on,
//!   one test however many projects ran it; the errors that came outside every test
//!   (`errors`).
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

/// The tests of nextest's JUnit report.
pub fn junit(xml: &str) -> Result<Results, String> {
    let mut reader = Reader::from_str(xml);
    let (mut tests, mut others) = (Vec::new(), Vec::new());
    // The test whose `<testcase>` is open, until its end, and whether it was skipped.
    let mut open: Option<(Test, bool)> = None;
    let mut ended = false;
    loop {
        match reader.read_event().map_err(|cause| cause.to_string())? {
            Event::Start(element) if element.name().as_ref() == b"testcase" => {
                open = Some((testcase(&element)?, false));
            }
            Event::Empty(element) if element.name().as_ref() == b"testcase" => {
                tests.push(testcase(&element)?);
            }
            Event::Empty(element) if element.name().as_ref() == b"testsuites" => ended = true,
            Event::Start(element) | Event::Empty(element) => {
                if let Some((test, skipped)) = open.as_mut() {
                    match element.name().as_ref() {
                        b"failure" | b"error" => test.failed = true,
                        b"skipped" => *skipped = true,
                        _ => {}
                    }
                }
            }
            Event::End(element) if element.name().as_ref() == b"testcase" => match open.take() {
                Some((test, false)) => tests.push(test),
                Some((test, true)) => others.push(Test {
                    failed: false,
                    ..test
                }),
                None => {}
            },
            Event::End(element) if element.name().as_ref() == b"testsuites" => ended = true,
            Event::Eof => break,
            _ => {}
        }
    }
    if !ended {
        return Err("the report ends before its <testsuites> does".to_owned());
    }
    Ok(Results {
        tests,
        outside: Vec::new(),
        others,
    })
}

/// The test a `<testcase>` names, not failed until a `<failure>` in it says so.
fn testcase(element: &BytesStart<'_>) -> Result<Test, String> {
    let attribute = |key: &str| -> Result<Option<String>, String> {
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
    };
    Ok(Test {
        name: attribute("name")?.ok_or("a <testcase> with no name")?,
        at: attribute("classname")?.unwrap_or_default(),
        file: None,
        whole: false,
        failed: false,
    })
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

/// The tests of Playwright's JSON report, and the errors outside them.
pub fn playwright(json: &str) -> Result<Results, String> {
    let report: serde_json::Value =
        serde_json::from_str(json).map_err(|cause| cause.to_string())?;
    let suites = report["suites"]
        .as_array()
        .ok_or("the report has no suites")?;
    let (mut tests, mut others) = (Vec::new(), Vec::new());
    for file in suites {
        // The file's own suite is no group of a test's name.
        suite(file, &[], &mut tests, &mut others);
    }
    let outside = report["errors"]
        .as_array()
        .ok_or("the report has no errors")?
        .iter()
        .map(|error| {
            error["message"]
                .as_str()
                .unwrap_or_default()
                .lines()
                .next()
                .unwrap_or_default()
                .to_owned()
        })
        .collect();
    Ok(Results {
        tests,
        outside,
        others,
    })
}

/// The tests of a suite of Playwright's report and of the suites in it, each named
/// by `groups` (the titles of the suites around it, the file's left out) and its own:
/// one test for each spec, which failed where it failed in a project, and is one of
/// the `others` where it was skipped in every project it was given to.
fn suite(
    suite: &serde_json::Value,
    groups: &[String],
    tests: &mut Vec<Test>,
    others: &mut Vec<Test>,
) {
    for spec in suite["specs"].as_array().into_iter().flatten() {
        let title = spec["title"].as_str().unwrap_or_default();
        let name = groups
            .iter()
            .map(String::as_str)
            .chain([title])
            .collect::<Vec<_>>()
            .join(" › ");
        let at = format!(
            "{}:{}:{}",
            spec["file"].as_str().unwrap_or_default(),
            spec["line"],
            spec["column"]
        );
        let statuses: Vec<&str> = spec["tests"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|test| test["status"].as_str().unwrap_or_default())
            .filter(|status| *status != "skipped")
            .collect();
        let test = Test {
            name,
            at,
            file: None,
            whole: false,
            failed: statuses.contains(&"unexpected"),
        };
        if statuses.is_empty() {
            others.push(test);
        } else {
            tests.push(test);
        }
    }
    for inner in suite["suites"].as_array().into_iter().flatten() {
        let mut deeper = groups.to_vec();
        deeper.push(inner["title"].as_str().unwrap_or_default().to_owned());
        self::suite(inner, &deeper, tests, others);
    }
}

#[cfg(test)]
mod tests;

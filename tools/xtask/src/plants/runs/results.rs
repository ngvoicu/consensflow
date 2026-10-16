//! What a run's tests did, as its reporter wrote it to a file of its own
//! (`reporters` says which file, and how the command is asked to write it): never
//! what the run printed, which a test may make say anything.
//!
//! - nextest's JUnit report: a `<testcase>` for each test, failed where it holds a
//!   `<failure>` or an `<error>`, and by its test binary (`classname`).
//! - Node's report, which the plants' own reporter of Node's tests writes
//!   (`reporters/node.mjs`): a line of JSON for each test that ended, named by its
//!   own title and those of the tests it is in, and whether its own code failed;
//!   then a line that the run ended. A test file that failed whole is a test named
//!   by its path.
//! - Playwright's JSON report: every test of every suite and its status
//!   (`unexpected` is a failure), named from its first group on; the errors that
//!   came outside every test (`errors`).
//!
//! A report is whole or it is an error: one cut short (no end to its root, or no
//! last line) says nothing of the tests it did not get to. A test that was skipped,
//! or marked to do (Node runs a test to do, and it may fail), neither failed nor
//! passed: it is left out.

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use serde::Deserialize;

/// The tests a run's reporter says it ran, and the errors it says came outside them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Results {
    pub tests: Vec<Test>,
    /// Errors outside every test, each as its first line.
    pub outside: Vec<String>,
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
    pub failed: bool,
}

impl Test {
    /// Whether this is a test file Node says ended whole, as a test of its own:
    /// named by its path, which is the file it says it is in.
    pub fn is_whole_file(&self) -> bool {
        self.file
            .as_ref()
            .is_some_and(|file| file.ends_with(self.name.as_str()) && is_test_file(&self.name))
    }
}

/// Whether `name` is named as a test file is: a path to a script.
fn is_test_file(name: &str) -> bool {
    [".mjs", ".cjs", ".js", ".mts", ".cts", ".ts"]
        .iter()
        .any(|extension| name.ends_with(extension))
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
            .filter(|whole| whole.is_whole_file() && whole.failed)
            .filter(|whole| {
                !self
                    .tests
                    .iter()
                    .any(|test| !test.is_whole_file() && test.file == whole.file)
            })
            .map(|whole| whole.name.as_str())
            .collect()
    }
}

/// The tests of nextest's JUnit report.
pub fn junit(xml: &str) -> Result<Results, String> {
    let mut reader = Reader::from_str(xml);
    let mut tests = Vec::new();
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
            Event::End(element) if element.name().as_ref() == b"testcase" => {
                if let Some((test, false)) = open.take() {
                    tests.push(test);
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
    Ok(Results {
        tests,
        outside: Vec::new(),
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
    failed: bool,
    #[serde(default)]
    skipped: bool,
}

/// The tests of the report of the plants' reporter of Node's tests, each named by
/// the titles of the tests it is in and its own, joined as Playwright joins them.
pub fn node(report: &str) -> Result<Results, String> {
    let mut tests = Vec::new();
    let mut ended = false;
    for (number, text) in report.lines().enumerate() {
        let read: NodeLine =
            serde_json::from_str(text).map_err(|cause| format!("line {}: {cause}", number + 1))?;
        if ended || (read.end != read.path.is_empty()) {
            return Err(format!("line {}: not a test, nor the end", number + 1));
        }
        ended = read.end;
        if read.end || read.skipped {
            continue;
        }
        let file = read.file.unwrap_or_default();
        let place = |at: Option<u64>| at.map(|at| at.to_string()).unwrap_or_default();
        tests.push(Test {
            name: read.path.join(" › "),
            at: format!("{file}:{}:{}", place(read.line), place(read.column)),
            file: Some(file),
            failed: read.failed,
        });
    }
    if !ended {
        return Err("the report ends before the run did".to_owned());
    }
    Ok(Results {
        tests,
        outside: Vec::new(),
    })
}

/// The tests of Playwright's JSON report, and the errors outside them.
pub fn playwright(json: &str) -> Result<Results, String> {
    let report: serde_json::Value =
        serde_json::from_str(json).map_err(|cause| cause.to_string())?;
    let suites = report["suites"]
        .as_array()
        .ok_or("the report has no suites")?;
    let mut tests = Vec::new();
    for file in suites {
        // The file's own suite is no group of a test's name.
        suite(file, &[], &mut tests);
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
    Ok(Results { tests, outside })
}

/// The tests of a suite of Playwright's report and of the suites in it, each named
/// by `groups` (the titles of the suites around it, the file's left out) and its own.
fn suite(suite: &serde_json::Value, groups: &[String], tests: &mut Vec<Test>) {
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
        for test in spec["tests"].as_array().into_iter().flatten() {
            let status = test["status"].as_str().unwrap_or_default();
            if status == "skipped" {
                continue;
            }
            tests.push(Test {
                name: name.clone(),
                at: at.clone(),
                file: None,
                failed: status == "unexpected",
            });
        }
    }
    for inner in suite["suites"].as_array().into_iter().flatten() {
        let mut deeper = groups.to_vec();
        deeper.push(inner["title"].as_str().unwrap_or_default().to_owned());
        self::suite(inner, &deeper, tests);
    }
}

#[cfg(test)]
mod tests;

//! What a run's tests did, as its reporter wrote it to a file of its own
//! (`reporters` says which file, and how the command is asked to write it): never
//! what the run printed, which a test may make say anything.
//!
//! - nextest's JUnit report and Node's: a `<testcase>` for each test, failed where
//!   it holds a `<failure>` or an `<error>`. Node names a test file that failed
//!   whole by its path, and says the file of each test (`file`); it escapes a `"`
//!   of a name twice (`&amp;quot;`), which is undone.
//! - libtest's log (`--logfile`), one file for each test binary: `ok <name>`,
//!   `failed <name>`, `ignored <name>`, a line for each test as it ended.
//! - Playwright's JSON report: every test of every suite and its status
//!   (`unexpected` is a failure), named from its first group on; the errors that
//!   came outside every test (`errors`).

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

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
    /// The file it is written in, where the reporter says (Node).
    pub file: Option<String>,
    pub failed: bool,
}

impl Test {
    /// Whether this is a test file Node says failed whole, as a test of its own:
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
    /// none was loaded, or none was registered before the file ended.
    pub fn files_that_ran_none(&self) -> Vec<&str> {
        self.tests
            .iter()
            .filter(|whole| whole.is_whole_file())
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

/// The tests of a JUnit report: nextest's, or Node's (`node`, whose names a `"` of
/// is escaped twice).
pub fn junit(xml: &str, node: bool) -> Result<Results, String> {
    let mut reader = Reader::from_str(xml);
    let mut tests = Vec::new();
    // The test whose `<testcase>` is open, until its end.
    let mut open: Option<Test> = None;
    loop {
        match reader.read_event().map_err(|cause| cause.to_string())? {
            Event::Start(element) if element.name().as_ref() == b"testcase" => {
                open = Some(testcase(&element, node)?);
            }
            Event::Empty(element) if element.name().as_ref() == b"testcase" => {
                tests.push(testcase(&element, node)?);
            }
            Event::Start(element) | Event::Empty(element)
                if matches!(element.name().as_ref(), b"failure" | b"error") =>
            {
                if let Some(test) = open.as_mut() {
                    test.failed = true;
                }
            }
            Event::End(element) if element.name().as_ref() == b"testcase" => {
                tests.extend(open.take());
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(Results {
        tests,
        outside: Vec::new(),
    })
}

/// The test a `<testcase>` names, not failed until a `<failure>` in it says so.
fn testcase(element: &BytesStart<'_>, node: bool) -> Result<Test, String> {
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
        // Node escapes a `"` as `&quot;`, then the `&` of that as `&amp;`.
        Ok(Some(if node {
            value.replace("&quot;", "\"")
        } else {
            value.into_owned()
        }))
    };
    Ok(Test {
        name: attribute("name")?.ok_or("a <testcase> with no name")?,
        file: attribute("file")?,
        failed: false,
    })
}

/// The tests of libtest's logs, one for each test binary that ran: a record for each
/// test, `<result> <name>`, the name last. A record is a line and the lines under it
/// that begin with a space: libtest's message for a `should_panic` test whose panic
/// was not the one (`failed: <why>`) can take lines, the panic quoted on one of its
/// own. A test that was ignored did not run.
pub fn libtest_logs(logs: &[String]) -> Results {
    let mut tests = Vec::new();
    for log in logs {
        let mut records: Vec<String> = Vec::new();
        for line in log.lines() {
            match records.last_mut() {
                Some(record) if line.starts_with(char::is_whitespace) => {
                    record.push('\n');
                    record.push_str(line);
                }
                _ => records.push(line.to_owned()),
            }
        }
        for record in records {
            let failed = match record.split([' ', ':']).next() {
                Some("ok") => false,
                Some("failed") => true,
                _ => continue,
            };
            if let Some(name) = record.split_whitespace().last() {
                tests.push(Test {
                    name: name.to_owned(),
                    file: None,
                    failed,
                });
            }
        }
    }
    Results {
        tests,
        outside: Vec::new(),
    }
}

/// The tests of Playwright's JSON report, and the errors outside them.
pub fn playwright(json: &str) -> Result<Results, String> {
    let report: serde_json::Value =
        serde_json::from_str(json).map_err(|cause| cause.to_string())?;
    let mut tests = Vec::new();
    for file in report["suites"].as_array().into_iter().flatten() {
        // The file's own suite is no group of a test's name.
        suite(file, &[], &mut tests);
    }
    let outside = report["errors"]
        .as_array()
        .into_iter()
        .flatten()
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
        for test in spec["tests"].as_array().into_iter().flatten() {
            let status = test["status"].as_str().unwrap_or_default();
            if status == "skipped" {
                continue;
            }
            tests.push(Test {
                name: name.clone(),
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

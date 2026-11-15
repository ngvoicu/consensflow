//! Prepare Node's test reporter and its command-line options.

use std::fs;
use std::path::Path;

use crate::context::Context;
use crate::process::Invocation;

/// The plants' reporter of Node's tests, and the files it is written to and writes.
pub(super) const NODE_REPORTER: &str = include_str!("node.mjs");
const NODE_REPORTER_FILE: &str = "plants.mjs";
pub(super) const NODE_REPORT: &str = "node.jsonl";

pub(super) fn setup(
    command: &[String],
    dir: &Path,
    context: &Context,
) -> Result<Invocation, std::io::Error> {
    fs::write(dir.join(NODE_REPORTER_FILE), NODE_REPORTER)?;
    let reporter = super::from_root(&dir.join(NODE_REPORTER_FILE), &context.root)?;
    Ok(
        Invocation::new(command[0].clone(), &context.root).args(node_words(
            command,
            &reporter,
            &dir.join(NODE_REPORT),
        )),
    )
}

/// The words after the program of a `node --test` that also has the plants'
/// reporter (the module `reporter`) write its report to `report`: the report it
/// prints is the one the command asked for, the spec report where none.
///
/// Its tests run in Node's own process (`--test-isolation=none`), whatever the
/// command asked for: a test file run in a process of its own tells its events to
/// Node through its standard output, where whatever the test writes is read too, and
/// a frame of Node's written there (a `node` it starts that inherits its output, a
/// test that writes one) is an event of a test that is not there. In one process the
/// files share it (what one leaves in the process is there for the next; one that
/// ends the process ends the run, before the report's last line), and run one at a
/// time; and Node 26.8.1 drops the error of a file that throws once it has
/// registered a test (the file is no failure, and its tests after it are not there).
pub(super) fn node_words(command: &[String], reporter: &str, report: &Path) -> Vec<String> {
    // The options this sets, each `--option=value` or `--option value`.
    const SET: [&str; 4] = [
        "--test-reporter",
        "--test-reporter-destination",
        "--test-isolation",
        "--experimental-test-isolation",
    ];
    let mut printed = None;
    let mut rest = Vec::new();
    let mut words = command.iter().skip(1);
    while let Some(word) = words.next() {
        let Some((option, value)) = SET.iter().find_map(|option| {
            let value = word.strip_prefix(option)?;
            match value.strip_prefix('=') {
                Some(value) => Some((*option, Some(value))),
                None => value.is_empty().then_some((*option, None)),
            }
        }) else {
            rest.push(word.clone());
            continue;
        };
        let value = value.or_else(|| words.next().map(String::as_str));
        if option == "--test-reporter" && printed.is_none() {
            printed = value;
        }
    }
    let mut words = vec![
        "--test-isolation=none".to_owned(),
        format!("--test-reporter={}", printed.unwrap_or("spec")),
        "--test-reporter-destination=stdout".to_owned(),
        format!("--test-reporter={reporter}"),
        format!("--test-reporter-destination={}", report.display()),
    ];
    words.extend(rest);
    words
}

//! Prepare Playwright's reporter and enforce a single attempt per test.

use std::fs;
use std::path::Path;

use super::super::super::threads_value;
use super::caller_threads;
use crate::context::Context;
use crate::process::Invocation;

/// The plants' reporter of Playwright's tests, the file it is written to, the file it
/// writes, and the variable that tells it where.
pub(super) const PLAYWRIGHT_REPORTER: &str = include_str!("playwright.mjs");
const PLAYWRIGHT_REPORTER_FILE: &str = "plants-playwright.mjs";
pub(super) const PLAYWRIGHT_REPORT: &str = "playwright.json";
pub(super) const PLAYWRIGHT_REPORT_VAR: &str = "PLANTS_PLAYWRIGHT_REPORT";

pub(super) fn setup(
    command: &[String],
    dir: &Path,
    context: &Context,
) -> Result<Invocation, std::io::Error> {
    fs::write(dir.join(PLAYWRIGHT_REPORTER_FILE), PLAYWRIGHT_REPORTER)?;
    let reporter = super::from_root(&dir.join(PLAYWRIGHT_REPORTER_FILE), &context.root)?;
    Ok(Invocation::new(command[0].clone(), &context.root)
        .args(
            playwright_words(command, &reporter, context)
                .into_iter()
                .skip(1),
        )
        .var(PLAYWRIGHT_REPORT_VAR, dir.join(PLAYWRIGHT_REPORT)))
}

/// Why a run of Playwright cannot be run as the plants run it, if it cannot: it asks
/// for retries (`--retries N` above 0), where a plant's run takes one attempt, the one
/// its report says (`playwright_words`).
pub(super) fn retried(command: &[String]) -> Option<String> {
    let mut words = command.iter();
    while let Some(word) = words.next() {
        let count = match word.strip_prefix("--retries") {
            Some("") => words.next().map(String::as_str),
            Some(value) => value.strip_prefix('='),
            None => continue,
        };
        if count.is_none_or(|count| count != "0") {
            return Some(format!(
                "Playwright's --retries {}: a plant's run takes one attempt",
                count.unwrap_or_default()
            ));
        }
    }
    None
}

/// The words of a Playwright run that also has the plants' reporter (the module
/// `reporter`) write its report: the list it prints and that report, whatever
/// reporter the command named; and one attempt of each test, whatever the config
/// file says (`--retries=0`), so that the report's word on a test is of the run of it.
/// A command that asks for retries is refused (`retried`); one that asks for none
/// is given it once. A group's `test.describe.configure({ retries })` is over this
/// word, and a test it runs again is no catch (the reporter's `why`). Each test is
/// run once too (`--repeat-each=1`, over the command's and the config's), its
/// repeats being one test to the report (Calliope's look at round 8).
///
/// The run is also given its width, `--workers`: the threads the driver was asked for
/// (`caller_threads`), over the command's own; else the command's own, as written; else
/// the machine's limit on tests at once (`RUST_TEST_THREADS`, which nextest is given too);
/// else none, and Playwright picks its own, as nextest does.
pub(super) fn playwright_words(
    command: &[String],
    reporter: &str,
    context: &Context,
) -> Vec<String> {
    let mut command = command.to_vec();
    let mut width = caller_threads(context);
    if width.is_some() {
        command = without_workers(&command);
    } else if without_workers(&command) == command {
        width = context
            .env
            .text("RUST_TEST_THREADS")
            .and_then(threads_value)
            .map(|threads| threads.to_string());
    }
    let mut words: Vec<String> = Vec::new();
    let mut given = command.iter();
    while let Some(word) = given.next() {
        if word == "--retries" || word == "--repeat-each" {
            given.next();
        } else if !["--reporter=", "--retries=", "--repeat-each="]
            .iter()
            .any(|option| word.starts_with(option))
        {
            words.push(word.clone());
        }
    }
    let test = words
        .iter()
        .position(|word| word == "test")
        .map_or(words.len(), |at| at + 1);
    let mut ours = vec![
        format!("--reporter=list,{reporter}"),
        "--retries=0".to_owned(),
        "--repeat-each=1".to_owned(),
    ];
    ours.extend(width.map(|width| format!("--workers={width}")));
    words.splice(test..test, ours);
    words
}

/// `command` without the words that say how many workers it has, in each way Playwright
/// takes them: `--workers N`, `--workers=N`, `-j N`, `-jN` and `-j=N`.
fn without_workers(command: &[String]) -> Vec<String> {
    let mut words = Vec::new();
    let mut given = command.iter();
    while let Some(word) = given.next() {
        if word == "--workers" || word == "-j" {
            given.next();
        } else if !word.starts_with("--workers=") && !word.starts_with("-j") {
            words.push(word.clone());
        }
    }
    words
}

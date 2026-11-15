//! Prepare Playwright's reporter and enforce a single attempt per test.

use std::fs;
use std::path::Path;

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
        .args(playwright_words(command, &reporter).into_iter().skip(1))
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
pub(super) fn playwright_words(command: &[String], reporter: &str) -> Vec<String> {
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
    words.splice(
        test..test,
        [
            format!("--reporter=list,{reporter}"),
            "--retries=0".to_owned(),
            "--repeat-each=1".to_owned(),
        ],
    );
    words
}

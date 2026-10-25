//! Who reports a run's tests, how its command is run so that the report lands in a
//! file of the run's own, and what that file says (`results`): attribution reads it
//! alone, never what the run printed.
//!
//! - `cargo test …` runs under nextest (`cargo nextest run`, its options as nextest
//!   names what cargo means by them: `nextest_words`), which runs each test in a
//!   process of its own and writes a JUnit report: no test's output can pass for
//!   another's result. Its exit status says that the build failed (101), or that no
//!   test was there to run (4). A run of the tests of `cf-e2e` is given the
//!   programs they run built before it (`names_cf_e2e` says why).
//! - A suite of xtask's that is a `cargo test` (`cargo xtask app test …`, `cargo
//!   xtask test <suite> …`, and the packaged smoke, `cargo xtask smoke …`, on a
//!   Mac) is that `cargo test`, as the suite makes it (its folder, its variables,
//!   its words), run under nextest the same way. The smoke's is without the
//!   `--nocapture` that a person's run has: nextest keeps what its test wrote.
//! - `node --test …` writes the report of the plants' own reporter of Node's tests
//!   (`reporters/node.mjs`, written into the run's folder) beside the report it
//!   prints, its tests run in Node's own process (`node_words` says why, and what
//!   it costs).
//! - Playwright's CLI writes the report of the plants' own reporter of its tests
//!   (`reporters/playwright.mjs`, written into the run's folder) beside the list it
//!   prints: its JSON report says no step of a test, and a test whose `beforeAll`
//!   threw is `unexpected` there though its body never ran, where the reporter's
//!   steps say which hook threw (Playwright 1.63, 2026-10-17). Each test is given one
//!   attempt (`--retries=0`) and run once (`--repeat-each=1`), and a run that asks for
//!   more attempts is refused; a group that
//!   asks for retries of its own is given them over that, and the reporter says a
//!   test run more than once (Sol's look at round 7).
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

/// The plants' reporter of Playwright's tests, the file it is written to, the file it
/// writes, and the variable that tells it where.
const PLAYWRIGHT_REPORTER: &str = include_str!("reporters/playwright.mjs");
const PLAYWRIGHT_REPORTER_FILE: &str = "plants-playwright.mjs";
const PLAYWRIGHT_REPORT: &str = "playwright.json";
const PLAYWRIGHT_REPORT_VAR: &str = "PLANTS_PLAYWRIGHT_REPORT";

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
    /// `smoke …`: the packaged smoke, on the app it names, if it names one, with capture on.
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
            Self::Smoke(app) => crate::smoke::captured(context, app.as_deref()),
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
                let tests = cargo_test(command, context);
                let words = nextest_words(&tests.args).map_err(std::io::Error::other)?;
                let config = dir.join("nextest.toml");
                fs::write(&config, nextest_config(dir))?;
                under_nextest(tests, words, nextest, &config)
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
            Self::Playwright => {
                fs::write(dir.join(PLAYWRIGHT_REPORTER_FILE), PLAYWRIGHT_REPORTER)?;
                let reporter = from_root(&dir.join(PLAYWRIGHT_REPORTER_FILE), &context.root)?;
                Invocation::new(command[0].clone(), &context.root)
                    .args(playwright_words(command, &reporter).into_iter().skip(1))
                    .var(PLAYWRIGHT_REPORT_VAR, dir.join(PLAYWRIGHT_REPORT))
            }
            Self::Status => as_written(),
        })
    }

    /// Whether `command` runs tests of `cf-e2e` in the checkout `context`, which are
    /// given the programs they run built before the run (`names_cf_e2e`).
    pub(super) fn runs_cf_e2e(self, command: &[String], context: &Context) -> bool {
        self == Self::Nextest && names_cf_e2e(&cargo_test(command, context).args)
    }

    /// Why `command` cannot be run as its reporter runs it in the checkout `context`,
    /// if it cannot: an option of `cargo test` that nextest has no word for
    /// (`nextest_words`), or one that turns capture off (`uncaptured`); retries of
    /// Playwright's (`retried`).
    pub(super) fn refused(self, command: &[String], context: &Context) -> Option<String> {
        match self {
            Self::Nextest => {
                let tests = cargo_test(command, context);
                nextest_words(&tests.args)
                    .err()
                    .or_else(|| uncaptured(&tests))
            }
            Self::Playwright => retried(command),
            Self::Node | Self::Status => None,
        }
    }

    /// What the report in `dir` says the run's tests did: none for a command that
    /// writes none, and an error for a report that is not there, cannot be read, or
    /// is not whole, which says why.
    pub(super) fn results(self, dir: &Path) -> Option<Result<Results, String>> {
        let (path, read): (PathBuf, Reader) = match self {
            Self::Nextest => (dir.join(NEXTEST_PROFILE).join("junit.xml"), results::junit),
            Self::Node => (dir.join(NODE_REPORT), results::node),
            Self::Playwright => (dir.join(PLAYWRIGHT_REPORT), results::playwright),
            Self::Status => return None,
        };
        Some(match fs::read_to_string(&path) {
            Ok(text) => read(&text).map_err(|why| format!("{}: {why}", path.display())),
            Err(cause) => Err(format!("{} could not be read: {cause}", path.display())),
        })
    }
}

/// The `cargo test` that `command` runs in the checkout `context`: a suite of xtask's
/// as the suite makes it, any other as it is written; and given none of nextest's
/// own variables (`NEXTEST_TEST_THREADS`, `NEXTEST_RETRIES`), which it reads over its
/// config, that this run was given or a nextest this runs under set.
fn cargo_test(command: &[String], context: &Context) -> Invocation {
    let as_written =
        || Invocation::new(command[0].clone(), &context.root).args(command[1..].iter());
    let tests = match command {
        [_, xtask, words @ ..] if xtask == "xtask" => Suite::of(words)
            .map(|suite| suite.invocation(context))
            .unwrap_or_else(as_written),
        _ => as_written(),
    };
    context
        .env
        .iter()
        .map(|(name, _)| name)
        .filter(|name| name.to_string_lossy().starts_with("NEXTEST"))
        .fold(tests, |tests, name| tests.without(name))
}

/// Why the `cargo test` of `tests` cannot be run under nextest with capture off, if
/// it would be: a word that turns it off (`--nocapture`, libtest's, which nextest takes
/// after `--`, or nextest's own `--no-capture`), or a variable the suite sets that does
/// (`NEXTEST_NO_CAPTURE`, libtest's `RUST_TEST_NOCAPTURE`). Without capture nextest
/// keeps nothing a test wrote, and a test it could not start fails with the status of
/// one that ran (both reviewers' look at round 6).
fn uncaptured(tests: &Invocation) -> Option<String> {
    const WORDS: [&str; 2] = ["--nocapture", "--no-capture"];
    const VARIABLES: [&str; 2] = ["NEXTEST_NO_CAPTURE", "RUST_TEST_NOCAPTURE"];
    let why = "nextest keeps nothing its tests wrote without capture, and a test it could not \
               start reads as one that failed";
    if let Some(word) = tests
        .args
        .iter()
        .find(|word| WORDS.iter().any(|off| *word == *off))
    {
        return Some(format!("cargo test {}: {why}", word.to_string_lossy()));
    }
    tests.vars.iter().find_map(|(name, value)| {
        let off = VARIABLES.iter().any(|variable| name == *variable)
            && value
                .as_ref()
                .is_some_and(|value| !value.is_empty() && value != "0");
        off.then(|| format!("{}: {why}", name.to_string_lossy()))
    })
}

/// Why a run of Playwright cannot be run as the plants run it, if it cannot: it asks
/// for retries (`--retries N` above 0), where a plant's run takes one attempt, the one
/// its report says (`playwright_words`).
fn retried(command: &[String]) -> Option<String> {
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

/// What an option of `cargo test` is, before its `--`.
#[derive(Clone, Copy)]
enum Takes {
    /// No value.
    Nothing,
    /// A value, the next word or after `=` (and, for a short name, right after it).
    Value,
}

/// How nextest is given an option of `cargo test`.
#[derive(Clone, Copy)]
enum Given {
    /// As it is: nextest's option of that name is cargo's.
    Same,
    /// As nextest's option of this name, where its own of cargo's name means another
    /// thing (`--profile` is nextest's profile, `-j` its tests at a time, `-v` and
    /// `--message-format` its own output).
    As(&'static str),
    /// Not at all: why.
    Refused(&'static str),
}

/// An option of `cargo test`: its name, its short name, what it takes, and how
/// nextest is given it.
type CargoOption = (&'static str, Option<char>, Takes, Given);

/// Every option of `cargo test` (Rust 1.9x's `cargo test --help`), its short name,
/// what it takes, and how nextest 0.9.148 is given it (its `cargo nextest run
/// --help`).
const CARGO_TEST: [CargoOption; 39] = [
    (
        "--no-run",
        None,
        Takes::Nothing,
        Given::Refused("it runs no test, which no plant can be caught by"),
    ),
    ("--no-fail-fast", None, Takes::Nothing, Given::Same),
    (
        "--future-incompat-report",
        None,
        Takes::Nothing,
        Given::Same,
    ),
    (
        "--message-format",
        None,
        Takes::Value,
        Given::As("--cargo-message-format"),
    ),
    (
        "--quiet",
        Some('q'),
        Takes::Nothing,
        Given::As("--cargo-quiet"),
    ),
    (
        "--verbose",
        Some('v'),
        Takes::Nothing,
        Given::As("--cargo-verbose"),
    ),
    ("--color", None, Takes::Value, Given::Same),
    ("--config", None, Takes::Value, Given::Same),
    (
        "-Z",
        None,
        Takes::Value,
        Given::Refused("nextest takes no unstable flag of cargo's"),
    ),
    (
        "--help",
        Some('h'),
        Takes::Nothing,
        Given::Refused("it runs no test"),
    ),
    ("--package", Some('p'), Takes::Value, Given::Same),
    ("--workspace", None, Takes::Nothing, Given::Same),
    ("--exclude", None, Takes::Value, Given::Same),
    ("--all", None, Takes::Nothing, Given::Same),
    ("--lib", None, Takes::Nothing, Given::Same),
    ("--bins", None, Takes::Nothing, Given::Same),
    ("--bin", None, Takes::Value, Given::Same),
    ("--examples", None, Takes::Nothing, Given::Same),
    ("--example", None, Takes::Value, Given::Same),
    ("--tests", None, Takes::Nothing, Given::Same),
    ("--test", None, Takes::Value, Given::Same),
    ("--benches", None, Takes::Nothing, Given::Same),
    ("--bench", None, Takes::Value, Given::Same),
    ("--all-targets", None, Takes::Nothing, Given::Same),
    (
        "--doc",
        None,
        Takes::Nothing,
        Given::Refused("nextest runs no doctest"),
    ),
    ("--features", Some('F'), Takes::Value, Given::Same),
    ("--all-features", None, Takes::Nothing, Given::Same),
    ("--no-default-features", None, Takes::Nothing, Given::Same),
    ("--jobs", Some('j'), Takes::Value, Given::As("--build-jobs")),
    ("--release", Some('r'), Takes::Nothing, Given::Same),
    (
        "--profile",
        None,
        Takes::Value,
        Given::As("--cargo-profile"),
    ),
    ("--target", None, Takes::Value, Given::Same),
    ("--target-dir", None, Takes::Value, Given::Same),
    ("--unit-graph", None, Takes::Nothing, Given::Same),
    ("--timings", None, Takes::Nothing, Given::Same),
    (
        "--manifest-path",
        Some('m'),
        Takes::Value,
        Given::As("--manifest-path"),
    ),
    ("--ignore-rust-version", None, Takes::Nothing, Given::Same),
    ("--locked", None, Takes::Nothing, Given::Same),
    ("--offline", None, Takes::Nothing, Given::Same),
];

/// The words nextest is given for the `cargo test` of `args` (its words, `test`
/// first): each option before its `--` as nextest names what cargo means by it
/// (`CARGO_TEST`), a word that is no option (the name the tests are chosen by) as it
/// is, and the words from its `--` on as they are, which nextest reads as libtest
/// does. An option that nextest has no word for, one that is no option of `cargo
/// test`, an option with no value, and short options written together are refused,
/// and why is said.
fn nextest_words(args: &[OsString]) -> Result<Vec<OsString>, String> {
    let mut words = Vec::new();
    let mut rest = args.iter().skip(1);
    while let Some(word) = rest.next() {
        let text = word.to_string_lossy();
        if text == "--" {
            return after_the_dashes(words, rest.cloned());
        }
        if !text.starts_with('-') || text == "-" {
            words.push(word.clone());
            continue;
        }
        let ((long, _, takes, given), attached) = option_of(&text)?;
        if let Given::Refused(why) = given {
            return Err(format!("cargo test {text}: {why}"));
        }
        // The value in the word itself, or the next word.
        let next = match (takes, attached) {
            (Takes::Nothing, None) | (Takes::Value, Some(_)) => None,
            (Takes::Nothing, Some(_)) => {
                return Err(format!("cargo test {text}: {long} takes no value"))
            }
            (Takes::Value, None) => Some(
                rest.next()
                    .filter(|value| !value.to_string_lossy().starts_with('-'))
                    .cloned()
                    .ok_or_else(|| format!("cargo test {text}: {long} is given no value"))?,
            ),
        };
        match given {
            Given::As(name) => {
                words.push(OsString::from(name));
                words.extend(attached.map(OsString::from));
            }
            _ => words.push(word.clone()),
        }
        words.extend(next);
    }
    Ok(words)
}

/// The words of nextest's that `words` (the words before a `--`, as nextest names them)
/// and `after` (libtest's, after it) are: libtest's threads (`--test-threads N` or
/// `--test-threads=N`) are nextest's own option before the `--`, which nextest takes
/// nowhere else (0.9.148 emulates `--ignored`, `--include-ignored`, `--skip`, `--exact`
/// and `--nocapture` there), and the rest stays after it.
fn after_the_dashes(
    mut words: Vec<OsString>,
    after: impl Iterator<Item = OsString>,
) -> Result<Vec<OsString>, String> {
    let mut libtest = Vec::new();
    let mut after = after.peekable();
    while let Some(word) = after.next() {
        let text = word.to_string_lossy().into_owned();
        let threads = match text.strip_prefix("--test-threads") {
            Some("") => Some(
                after
                    .next_if(|value| !value.to_string_lossy().starts_with('-'))
                    .ok_or_else(|| {
                        "cargo test -- --test-threads: --test-threads is given no value".to_owned()
                    })?,
            ),
            Some(joined) => joined.strip_prefix('=').map(OsString::from),
            None => None,
        };
        match threads {
            Some(threads) => words.extend([OsString::from("--test-threads"), threads]),
            None => libtest.push(word),
        }
    }
    words.push(OsString::from("--"));
    words.extend(libtest);
    Ok(words)
}

/// The option of `cargo test` that `text` (a word that starts with `-`) is, and the
/// value written in it, if one is: `--name=value`, `-x` and `-xvalue`, and `-vv`
/// (which is `-v` twice, told as `--cargo-verbose` once: nextest takes it once).
fn option_of(text: &str) -> Result<(CargoOption, Option<&str>), String> {
    let unknown = || format!("cargo test {text}: no option of cargo test");
    if let Some(name) = text.strip_prefix("--") {
        let (name, value) = match name.split_once('=') {
            Some((name, value)) => (name, Some(value)),
            None => (name, None),
        };
        let option = CARGO_TEST
            .iter()
            .find(|(long, ..)| long.strip_prefix("--") == Some(name))
            .ok_or_else(unknown)?;
        return Ok((*option, value));
    }
    let mut letters = text[1..].chars();
    let first = letters.next().ok_or_else(unknown)?;
    let after = &text[1 + first.len_utf8()..];
    let option = CARGO_TEST
        .iter()
        .find(|(long, short, ..)| *short == Some(first) || *long == format!("-{first}"))
        .ok_or_else(unknown)?;
    match option.2 {
        Takes::Value => Ok((*option, (!after.is_empty()).then_some(after))),
        Takes::Nothing if after.chars().all(|letter| letter == 'v') && first == 'v' => {
            Ok((*option, None))
        }
        Takes::Nothing if after.is_empty() => Ok((*option, None)),
        Takes::Nothing => Err(format!("cargo test {text}: write each short option apart")),
    }
}

/// The `cargo test` of `tests`, run by nextest at `nextest` with the config file
/// `config`, given `words` (`nextest_words`): in its folder, with its variables.
fn under_nextest(
    tests: Invocation,
    words: Vec<OsString>,
    nextest: &str,
    config: &Path,
) -> Invocation {
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

/// Whether the cargo words `words` name the package `cf-e2e`, before a `--`. Each
/// process of one of its tests builds the `cf` and the pane host it runs, once for
/// the process, and one test's build took away the `cf` another test was starting
/// where they ran beside each other ("could not start …/target/release/cf: No such
/// file or directory", 2026-10-16): cargo linked it again, the build before the rig
/// having made it of other features. The run builds them once, before its tests, as
/// they would (`CF_E2E_PROGRAMS`), and tells them where (`CF_E2E_BUILT`): they
/// build nothing, and run beside each other.
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
/// of them whatever fails, and the JUnit report at `<dir>/plants/junit.xml`, which
/// names every test the run registered, the skipped ones too (nextest 0.9.148 leaves
/// them out unless asked): an ignored test of the meant name is a test of it all the
/// same.
fn nextest_config(dir: &Path) -> String {
    let store = dir.display().to_string().replace('\\', "/");
    format!(
        "[store]\ndir = \"{store}\"\n\n[profile.{NEXTEST_PROFILE}]\nretries = 0\nfail-fast = false\n\n\
         [profile.{NEXTEST_PROFILE}.junit]\npath = \"junit.xml\"\nreport-skipped = \"all\"\n"
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
fn node_words(command: &[String], reporter: &str, report: &Path) -> Vec<String> {
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

/// The words of a Playwright run that also has the plants' reporter (the module
/// `reporter`) write its report: the list it prints and that report, whatever
/// reporter the command named; and one attempt of each test, whatever the config
/// file says (`--retries=0`), so that the report's word on a test is of the run of it.
/// A command that asks for retries is refused (`retried`); one that asks for none
/// is given it once. A group's `test.describe.configure({ retries })` is over this
/// word, and a test it runs again is no catch (the reporter's `why`). Each test is
/// run once too (`--repeat-each=1`, over the command's and the config's), its
/// repeats being one test to the report (Calliope's look at round 8).
fn playwright_words(command: &[String], reporter: &str) -> Vec<String> {
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

#[cfg(test)]
mod tests;

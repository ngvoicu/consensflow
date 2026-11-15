//! Prepare cargo tests for nextest and interpret the command's options.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use super::caller_threads;
use crate::context::Context;
use crate::process::Invocation;

/// The profile of nextest the plants' runs use, which the run's own config file says.
pub(super) const NEXTEST_PROFILE: &str = "plants";

pub(super) fn setup(
    command: &[String],
    dir: &Path,
    context: &Context,
    nextest: &str,
) -> Result<Invocation, std::io::Error> {
    let tests = cargo_test(command, context);
    let words = nextest_words(&tests.args).map_err(std::io::Error::other)?;
    let words = with_the_threads_of_the_environment(words, context);
    let config = dir.join("nextest.toml");
    std::fs::write(&config, nextest_config(dir))?;
    Ok(under_nextest(tests, words, nextest, &config))
}

/// A suite of xtask's that is a `cargo test` of its own, by the words after `cargo xtask`.
pub(super) enum Suite<'a> {
    /// `app test …`: the app crate's tests.
    App(&'a [String]),
    /// `test <name> …`: a suite of `cf-e2e`'s.
    Named(crate::suites::Suite, &'a [String]),
    /// `smoke …`: the packaged smoke, on the app it names, if it names one, with capture on.
    Smoke(Option<PathBuf>),
}

impl<'a> Suite<'a> {
    pub(super) fn of(words: &'a [String]) -> Option<Self> {
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

pub(super) fn os(words: &[String]) -> Vec<OsString> {
    words.iter().map(OsString::from).collect()
}

/// The `cargo test` that `command` runs in the checkout `context`: a suite of xtask's
/// as the suite makes it, any other as it is written; and given none of nextest's
/// own variables (`NEXTEST_TEST_THREADS`, `NEXTEST_RETRIES`), which it reads over its
/// config, that this run was given or a nextest this runs under set.
pub(super) fn cargo_test(command: &[String], context: &Context) -> Invocation {
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
pub(super) fn uncaptured(tests: &Invocation) -> Option<String> {
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
pub(super) fn nextest_words(args: &[OsString]) -> Result<Vec<OsString>, String> {
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

/// Takes out every `--test-threads` of `words` in their first `before` words, either way
/// it is written (the two words, and the word with its value after `=`), with its value
/// where the two-word form carries one: the caller's number is put in its place after,
/// and both would reach the runner otherwise.
pub(super) fn take_out_threads(words: &mut Vec<OsString>, before: usize) {
    loop {
        let before = before.min(words.len());
        let Some(at) = words[..before].iter().position(|word| {
            let word = word.to_string_lossy();
            word == "--test-threads" || word.starts_with("--test-threads=")
        }) else {
            return;
        };
        if words[at].to_string_lossy() == "--test-threads" {
            if at + 1 < before {
                words.remove(at + 1);
            } else {
                return;
            }
        }
        words.remove(at);
    }
}

/// `words` (nextest's, as [`nextest_words`] makes them) with the threads libtest is told by
/// the environment (`RUST_TEST_THREADS`) given to nextest as its own option, if the command
/// names none: nextest does not read that variable, and a machine's limit on the tests it runs
/// at once (zeewin's, 4: the run script sets it) did not reach the plants' runs, whose baseline
/// suites ran as wide as the machine (T-777, P2-3). It goes before the `--`, where nextest takes
/// it; a variable that is no number of threads, or 0, is none.
///
/// The threads the driver was asked for (`CF_PLANTS_TEST_THREADS`) are given the same way
/// and win over the table's own, the caller's number being the one asked for.
pub(super) fn with_the_threads_of_the_environment(
    mut words: Vec<OsString>,
    context: &Context,
) -> Vec<OsString> {
    if let Some(threads) = caller_threads(context) {
        let before = words
            .iter()
            .position(|word| word == "--")
            .unwrap_or(words.len());
        // Over a table's own, either way it was written.
        take_out_threads(&mut words, before);
        let before = words
            .iter()
            .position(|word| word == "--")
            .unwrap_or(words.len());
        words.splice(
            before..before,
            [OsString::from("--test-threads"), OsString::from(threads)],
        );
        return words;
    }
    let before = words
        .iter()
        .position(|word| word == "--")
        .unwrap_or(words.len());
    if words[..before].iter().any(|word| word == "--test-threads") {
        return words;
    }
    let threads = context
        .env
        .text("RUST_TEST_THREADS")
        .filter(|threads| threads.parse::<u32>().is_ok_and(|threads| threads > 0));
    if let Some(threads) = threads {
        words.splice(
            before..before,
            [OsString::from("--test-threads"), OsString::from(threads)],
        );
    }
    words
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

/// Whether the cargo words `words` name the package `cf-e2e`, before a `--`. Each
/// process of one of its tests builds the `cf` and the pane host it runs, once for
/// the process, and one test's build took away the `cf` another test was starting
/// where they ran beside each other ("could not start …/target/release/cf: No such
/// file or directory", 2026-10-16): cargo linked it again, the build before the rig
/// having made it of other features. The run builds them once, before its tests, as
/// they would (`CF_E2E_PROGRAMS`), and tells them where (`CF_E2E_BUILT`): they
/// build nothing, and run beside each other.
pub(super) fn names_cf_e2e(words: &[OsString]) -> bool {
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
pub(super) fn nextest_config(dir: &Path) -> String {
    let store = dir.display().to_string().replace('\\', "/");
    format!(
        "[store]\ndir = \"{store}\"\n\n[profile.{NEXTEST_PROFILE}]\nretries = 0\nfail-fast = false\n\n\
         [profile.{NEXTEST_PROFILE}.junit]\npath = \"junit.xml\"\nreport-skipped = \"all\"\n"
    )
}

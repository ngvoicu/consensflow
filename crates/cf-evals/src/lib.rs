//! ConsensFlow's chief evals: a scenario is played by a chief and a staff of real
//! harnesses on the native daemon, a scripted owner answering what they ask, and
//! what each run left in the ledger is measured, by code that reads the ledger
//! file and no more. The reports of the runs are kept (`evals/reports`) and
//! summarized (`evals/RESULTS.md`).
//!
//! This crate depends on no crate of the product. The measure is SQL over the
//! ledger file as SQLite holds it, so that what a run is held to is what the
//! ledger says and not what the ledger's own code makes of it; the run drives the
//! daemon, the pane host and the windows as `crates/cf-e2e` does, by programs and
//! files. `cargo xtask eval`, `eval summary` and `watch` hand their words to the
//! `cf-evals` command of this crate ([`command`]).
//!
//! A module for each thing it does:
//!
//! - [`scenarios`] are the twelve scenarios, code and not data: what the owner
//!   says, how it answers, and the expectations, closures over the numbers.
//! - [`plan`] is the pure part of a run: the staff, the chief's model, the owner's
//!   answers to the chief's questions.
//! - [`mod@run`] plays a scenario with real harnesses (the arm without ConsensFlow
//!   is measured by [`bare`], from the harness's own record) and writes its report.
//! - [`measure`] reads what a chief did from the ledger of a finished run, and
//!   [`ledger_file`] makes a ledger file without the ledger, which the tests of the
//!   measure are made of.
//! - [`results`] and [`summarize`] say the reports kept in a table, and [`watch`]
//!   looks at a project that is running.
//!
//! What the rest leans on: the flags every command reads the same way (`cli`),
//! the stamps of a report (`clock`), the lengths, slices and white space of
//! JavaScript's strings, which the measured numbers were made in (`js`), and what
//! goes wrong, as a value (`error`). What a window and a conversation id are, and
//! how a harness is found on the machine, are `crates/cf-e2e`'s ([`cf_e2e::live`]).

#![forbid(unsafe_code)]

pub mod bare;
mod cli;
pub mod clock;
mod error;
mod js;
pub mod ledger_file;
pub mod measure;
pub mod plan;
pub mod results;
pub mod run;
pub mod scenarios;
pub mod summarize;
pub mod watch;

use std::ffi::OsString;
use std::io::Write;
use std::path::PathBuf;

use cf_e2e::checkout;

use cli::Flag;
pub use error::{Error, Result};

/// What the command says of itself.
const USAGE: &str = "\
usage: cf-evals <command> [flags]

  run      play a scenario with a chief and a staff of real harnesses, and measure it
             [--scenario NAME] [--chief HARNESS] [--staff HARNESS,...] [--model M]
             [--claude-staff-model M] [--pi-staff-model M] [--effort LEVEL]
             [--staff-effort LEVEL] [--arm card|nocard|bare] [--switch-to HARNESS]
             [--gate] [--repeat N] [--timeout-min N] [--reports DIR]
  summary  write the table of the reports kept in evals/reports (evals/RESULTS.md)
             [--reports DIR] [--results FILE]
  watch    look at a project that is running
             --project NAME-OR-ID [--minutes N] [--tokens] [--home DIR]
";

/// The flags of `summary`.
const SUMMARY_FLAGS: [Flag; 2] = [Flag::value("reports"), Flag::value("results")];

/// The `cf-evals` command: `args` are its words (without the program's name) and
/// the answer is its exit status: 0 when what it did held, 1 when it did not or
/// could not, and 2 when its words were not ones it takes.
pub fn command(args: &[OsString], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let words: Vec<String> = args
        .iter()
        .map(|word| word.to_string_lossy().into_owned())
        .collect();
    // A command that cannot say so on its error output has failed all the same.
    match dispatch(&words, out) {
        Ok(status) => status,
        Err(Error::Usage(message)) => {
            let _ = writeln!(err, "cf-evals: {message}");
            let _ = write!(err, "{USAGE}");
            2
        }
        Err(failure) => {
            let _ = writeln!(err, "cf-evals: {failure}");
            1
        }
    }
}

fn dispatch(words: &[String], out: &mut dyn Write) -> Result<i32> {
    match words.split_first() {
        None => Err(Error::Usage("a command is required".to_owned())),
        Some((verb, _)) if matches!(verb.as_str(), "help" | "--help" | "-h") => {
            write!(out, "{USAGE}")?;
            Ok(0)
        }
        Some((verb, rest)) if verb == "run" => run::command(rest, out),
        Some((verb, rest)) if verb == "summary" => summary(rest, out),
        Some((verb, rest)) if verb == "watch" => watch::command(rest, out),
        Some((verb, _)) => Err(Error::Usage(format!("unknown command: {verb}"))),
    }
}

/// `summary`: the table of the kept reports, in the checkout's `evals` unless the
/// words name other places.
fn summary(words: &[String], out: &mut dyn Write) -> Result<i32> {
    let flags = cli::parse(&SUMMARY_FLAGS, words)?;
    let evals = checkout::path("evals");
    let reports = flags
        .get("reports")
        .map_or_else(|| evals.join("reports"), PathBuf::from);
    let results = flags
        .get("results")
        .map_or_else(|| evals.join("RESULTS.md"), PathBuf::from);
    summarize::summarize(&reports, &results, out)?;
    Ok(0)
}

#[cfg(test)]
mod tests;

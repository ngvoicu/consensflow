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
//! A module for each of the seven files of `evals/`, which the landing that ports
//! them fills ([`plan`], [`bare`], [`measure`], [`results`], [`summarize`],
//! [`watch`] and [`mod@run`]), and [`ledger_file`]: a ledger file made without
//! the ledger, which the tests of the measure (and of anything that reads a ledger)
//! are made of.

#![forbid(unsafe_code)]

pub mod bare;
mod error;
pub mod ledger_file;
pub mod measure;
pub mod plan;
pub mod results;
pub mod run;
pub mod summarize;
pub mod watch;

use std::ffi::OsString;
use std::io::Write;

pub use error::{Error, Result};

/// The `cf-evals` command: `args` are its words (without the program's name) and
/// the answer is its exit status. It has no verb yet: the evals are still the
/// JavaScript tools, and the landing that ports them gives it `run`, `summary` and
/// `watch`, which `cargo xtask eval`, `eval summary` and `watch` ask for.
pub fn command(args: &[OsString], _out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let given = if args.is_empty() {
        String::new()
    } else {
        let words: Vec<_> = args.iter().map(|word| word.to_string_lossy()).collect();
        format!(" (it was given: {})", words.join(" "))
    };
    // A command that cannot say so on its error output has failed all the same.
    let _ = writeln!(
        err,
        "cf-evals: the evals are not ported yet: run `npm run eval`, `npm run eval:summary` or `npm run watch` for now{given}"
    );
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn until_the_landing_that_ports_the_evals_the_command_says_so_and_fails() {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let status = command(&[], &mut out, &mut err);
        assert_eq!(status, 1);
        assert!(out.is_empty());
        let said = String::from_utf8(err).unwrap();
        assert!(
            said.starts_with("cf-evals: the evals are not ported yet: run `npm run eval`"),
            "{said}"
        );
        assert!(!said.contains("it was given"), "{said}");

        let (mut out, mut err) = (Vec::new(), Vec::new());
        let words = [
            OsString::from("run"),
            OsString::from("--scenario"),
            OsString::from("round-trip"),
        ];
        assert_eq!(command(&words, &mut out, &mut err), 1);
        assert!(String::from_utf8(err)
            .unwrap()
            .ends_with("(it was given: run --scenario round-trip)\n"));
    }
}

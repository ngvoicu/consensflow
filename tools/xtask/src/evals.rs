//! `cargo xtask eval`, `eval summary`, `eval long` and `watch`: the chief evals, their
//! summary, the long run and the look at a project that is running. The code is
//! `crates/cf-evals` (a library and a binary), which these commands hand their words
//! to: `eval` is its `run`, `eval summary` its `summary`, `eval long` its `long` and
//! `watch` its `watch`, with the words after the command as they are. A run spends
//! real tokens on the machine's own harnesses and is no part of `check`.
//!
//! The long run is built in release: it looks at its project every few seconds for
//! hours, reading the ledger and every Claude Code record as they grow, which a
//! debug build reads ten times slower.
//!
//! `eval summary` and `eval long` come before `eval` in the table: a command is
//! found by the first row whose words begin the command line.

use std::ffi::OsString;
use std::path::Path;

use crate::context::Context;
use crate::dispatch::{Command, Console, Failure, Run};
use crate::process::{self, Invocation};

pub const COMMANDS: &[Command] = &[
    Command {
        words: &["eval", "summary"],
        about: "Write the summary of the eval reports kept in evals/reports (evals/RESULTS.md)",
        usage: "[--reports DIR] [--results FILE]",
        run: Run::Native(summary),
    },
    Command {
        words: &["eval", "long"],
        about: "Run the long run: a real project worked for hours by a chief and a staff of every harness, disrupted on a schedule, ConsensFlow held to nine invariants the whole time (it spends real quota for hours, and is no part of check)",
        usage: "[--chief HARNESS] [--staff HARNESS,...] [--models HARNESS=M+M,...] [--model M] [--effort LEVEL] [--staff-effort LEVEL] [--hours N] [--every-min N] [--look-sec N] [--idle-min N] [--dialog-min N] [--quiet-min N] [--wake-min N] [--switch-to HARNESS] [--quiet] [--pause] [--kill] [--switch] [--compact] [--restart] [--devin-cut] [--suspend] [--ask real|stand-in] [--reports DIR]",
        run: Run::Native(long),
    },
    Command {
        words: &["eval"],
        about: "Run a chief eval: a scenario, played by a chief and a staff of real harnesses on the native daemon (it spends real tokens, and is no part of check)",
        usage: "[--scenario NAME] [--chief HARNESS] [--staff HARNESS,...] [--model M] [--claude-staff-model M] [--pi-staff-model M] [--effort LEVEL] [--staff-effort LEVEL] [--arm card|nocard|bare] [--switch-to HARNESS] [--gate] [--repeat N] [--timeout-min N] [--reports DIR]",
        run: Run::Native(eval),
    },
    Command {
        words: &["watch"],
        about: "Look at a project that is running: its tasks, its messages, and the tokens its windows used",
        usage: "--project NAME-OR-ID [--minutes N] [--tokens] [--home DIR]",
        run: Run::Native(watch),
    },
];

fn summary(context: &Context, args: &[OsString], _: &mut Console) -> Result<i32, Failure> {
    hand_over(context, "summary", args)
}

fn eval(context: &Context, args: &[OsString], _: &mut Console) -> Result<i32, Failure> {
    hand_over(context, "run", args)
}

fn watch(context: &Context, args: &[OsString], _: &mut Console) -> Result<i32, Failure> {
    hand_over(context, "watch", args)
}

fn long(context: &Context, args: &[OsString], _: &mut Console) -> Result<i32, Failure> {
    hand_over(context, "long", args)
}

/// Runs the `cf-evals` command `verb` with `args`: its exit status is the answer.
fn hand_over(context: &Context, verb: &str, args: &[OsString]) -> Result<i32, Failure> {
    Ok(process::run(
        &invocation(&context.root, verb, args),
        &context.env,
    )?)
}

/// The `cargo run` of the `cf-evals` command `verb`, in the checkout at `root`: the
/// long run's in release.
fn invocation(root: &Path, verb: &str, args: &[OsString]) -> Invocation {
    let build: &[&str] = if verb == "long" {
        &["run", "--release", "--locked", "--quiet"]
    } else {
        &["run", "--locked", "--quiet"]
    };
    Invocation::new("cargo", root)
        .args(build.iter().copied())
        .args(["--package", "cf-evals", "--"])
        .arg(verb)
        .args(args)
}

#[cfg(test)]
mod tests;

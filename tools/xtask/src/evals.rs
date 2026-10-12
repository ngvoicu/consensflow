//! `cargo xtask eval`, `eval summary` and `watch`: the chief evals, their summary and
//! the look at a project that is running. The code is `crates/cf-evals` (a library
//! and a binary), which these commands hand their words to: `eval` is its `run`,
//! `eval summary` its `summary` and `watch` its `watch`, with the words after the
//! command as they are. A run spends real tokens on the machine's own harnesses and
//! is no part of `check`.
//!
//! `eval summary` comes before `eval` in the table: a command is found by the
//! first row whose words begin the command line.

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

/// Runs the `cf-evals` command `verb` with `args`: its exit status is the answer.
fn hand_over(context: &Context, verb: &str, args: &[OsString]) -> Result<i32, Failure> {
    Ok(process::run(
        &invocation(&context.root, verb, args),
        &context.env,
    )?)
}

/// The `cargo run` of the `cf-evals` command `verb`, in the checkout at `root`.
fn invocation(root: &Path, verb: &str, args: &[OsString]) -> Invocation {
    Invocation::new("cargo", root)
        .args(["run", "--locked", "--quiet", "--package", "cf-evals", "--"])
        .arg(verb)
        .args(args)
}

#[cfg(test)]
mod tests;

//! `cargo xtask check`: what a developer runs before a commit. It says each
//! step before it runs it and stops at the first that fails, with that step's
//! exit status as its own.
//!
//! The steps are rustfmt, clippy and the tests of the workspace, then the
//! linter and the Node tests (`npm run lint`, `npm test`: what `npm run check`
//! runs). The app crate is left out of the workspace's lint and tests as the
//! gates leave it out: its build script wants the resources `cargo xtask stage`
//! puts in `app/src-tauri`, which a worktree has not, and `cargo xtask app
//! clippy` and `app test` lint and test it with them left out of its
//! configuration.
//!
//! xtask's own tests run in a step of their own, in a target folder of their
//! own. The `xtask` this command is running from is `target/debug/xtask`, the
//! one `cargo xtask` built, and cargo puts there whichever build of xtask it
//! made last: a test build of it is not that one (its dev-dependencies change the
//! features of what is under `cf-release`, so the `cf-release` it links is
//! another, and so is the binary: seen on macOS, 1,441,144 bytes against
//! 1,440,936), and so is a test build of the whole workspace. A program that is
//! running was overwritten without a word where this was seen (macOS), and
//! Windows refuses to overwrite one. So the workspace's test step leaves xtask
//! out, and xtask is tested where nothing of it is running.

use std::ffi::OsString;
use std::io::Write;

use crate::context::Context;
use crate::dispatch::{Command, Console, Failure, Run};
use crate::process::{self, Invocation};

/// Where xtask's own tests are built, from the checkout's root: beside the
/// workspace's build folder, not in it.
const XTASK_TARGET: &str = "app/src-tauri/target/xtask-check";

pub const COMMANDS: &[Command] = &[Command {
    words: &["check"],
    about: "Before a commit: rustfmt, clippy, the Rust tests, the linter and the Node tests, stopping at the first that fails",
    usage: "",
    run: Run::Native(run),
}];

fn run(context: &Context, args: &[OsString], console: &mut Console) -> Result<i32, Failure> {
    if !args.is_empty() {
        return Err(Failure::Usage("check takes no arguments".into()));
    }
    run_steps(
        &steps(context),
        |step| process::run(step, &context.env),
        console.err,
    )
}

/// What `check` runs, in order.
fn steps(context: &Context) -> Vec<Invocation> {
    let at = |program: &str, words: &[&str]| {
        Invocation::new(program, &context.root).args(words.iter().copied())
    };
    vec![
        at("cargo", &["fmt", "--all", "--check"]),
        at(
            "cargo",
            &[
                "clippy",
                "--workspace",
                "--exclude",
                "app",
                "--all-targets",
                "--",
                "-D",
                "warnings",
            ],
        ),
        at(
            "cargo",
            &[
                "test",
                "--workspace",
                "--exclude",
                "app",
                "--exclude",
                "xtask",
            ],
        ),
        at("cargo", &["test", "--package", "xtask", "--target-dir"])
            .arg(context.path(XTASK_TARGET)),
        at("npm", &["run", "lint"]),
        at("npm", &["test"]),
    ]
}

/// Runs `steps` with `run`, saying each on `err` first: the status of the
/// first that does not end with 0, else 0.
fn run_steps(
    steps: &[Invocation],
    mut run: impl FnMut(&Invocation) -> Result<i32, process::Failure>,
    err: &mut dyn Write,
) -> Result<i32, Failure> {
    let total = steps.len();
    for (index, step) in steps.iter().enumerate() {
        let number = index + 1;
        writeln!(err, "xtask check [{number}/{total}]: {}", step.display())?;
        let status = run(step)?;
        if status != 0 {
            writeln!(
                err,
                "xtask check: step {number} of {total} ended with status {status}: {}",
                step.display()
            )?;
            return Ok(status);
        }
    }
    writeln!(err, "xtask check: all {total} steps passed")?;
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::path::{Path, PathBuf};

    use cf_base::env::Env;

    fn context() -> Context {
        Context {
            root: PathBuf::from("checkout"),
            env: Env::default(),
        }
    }

    #[test]
    fn the_steps_are_the_gates_in_order_all_run_from_the_root() {
        let context = context();
        let steps = steps(&context);
        let lines: Vec<_> = steps.iter().map(Invocation::display).collect();
        let own_target = context.path(XTASK_TARGET).display().to_string();
        assert_eq!(
            lines,
            [
                "cargo fmt --all --check".to_string(),
                "cargo clippy --workspace --exclude app --all-targets -- -D warnings".to_string(),
                "cargo test --workspace --exclude app --exclude xtask".to_string(),
                format!("cargo test --package xtask --target-dir {own_target}"),
                "npm run lint".to_string(),
                "npm test".to_string(),
            ]
        );
        assert!(steps
            .iter()
            .all(|step| step.cwd == Path::new("checkout") && step.vars.is_empty()));
    }

    #[test]
    fn check_takes_no_arguments() {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let mut console = Console {
            out: &mut out,
            err: &mut err,
        };
        let refused = run(&context(), &[OsString::from("--fast")], &mut console).unwrap_err();
        assert_eq!(refused.to_string(), "check takes no arguments");
        assert!(matches!(refused, Failure::Usage(_)));
        assert!(err.is_empty());
    }

    fn steps_named(names: &[&str]) -> Vec<Invocation> {
        names
            .iter()
            .map(|name| Invocation::new(*name, "."))
            .collect()
    }

    #[test]
    fn every_step_is_said_before_it_runs_and_all_passing_is_said_last() {
        let steps = steps_named(&["one", "two", "three"]);
        let (mut ran, mut said) = (Vec::new(), Vec::new());
        let status = run_steps(
            &steps,
            |step| {
                ran.push(step.display());
                Ok(0)
            },
            &mut said,
        )
        .unwrap();
        assert_eq!(status, 0);
        assert_eq!(ran, ["one", "two", "three"]);
        assert_eq!(
            String::from_utf8(said).unwrap(),
            "xtask check [1/3]: one\nxtask check [2/3]: two\nxtask check [3/3]: three\n\
             xtask check: all 3 steps passed\n"
        );
    }

    #[test]
    fn the_first_step_that_fails_ends_it_with_its_status_and_the_rest_do_not_run() {
        let steps = steps_named(&["one", "two", "three"]);
        let (mut ran, mut said) = (Vec::new(), Vec::new());
        let status = run_steps(
            &steps,
            |step| {
                ran.push(step.display());
                Ok(if step.display() == "two" { 101 } else { 0 })
            },
            &mut said,
        )
        .unwrap();
        assert_eq!(status, 101);
        assert_eq!(ran, ["one", "two"]);
        assert_eq!(
            String::from_utf8(said).unwrap(),
            "xtask check [1/3]: one\nxtask check [2/3]: two\n\
             xtask check: step 2 of 3 ended with status 101: two\n"
        );
    }

    #[test]
    fn a_step_that_cannot_be_started_is_an_error_not_a_status() {
        let steps = steps_named(&["npm"]);
        let mut said = Vec::new();
        let failed = run_steps(
            &steps,
            |step| {
                Err(process::Failure::NotFound {
                    program: step.display(),
                })
            },
            &mut said,
        )
        .unwrap_err();
        assert_eq!(
            failed.to_string(),
            "`npm` was not found: is it installed, and on the PATH?"
        );
        assert_eq!(String::from_utf8(said).unwrap(), "xtask check [1/1]: npm\n");
    }
}

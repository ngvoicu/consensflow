//! `cargo xtask check`: what a developer runs before a commit, the one full
//! local gate (`npm run check:all` is this). It says each step before it runs
//! it and stops at the first that fails, with that step's exit status as its
//! own.
//!
//! The steps are rustfmt; clippy for the workspace, for the app and for
//! Windows; the tests of the workspace, of xtask and of the app; then the
//! linter, the build of the native `cf` and the Node tests (`npm run lint` and
//! `npm test`: what `npm run check` runs, with the `cf` those tests run built
//! again from the sources as they are). The app crate is left out of the
//! workspace's lint and tests as the
//! gates leave it out: its build script wants the resources `cargo xtask stage`
//! puts in `app/src-tauri`, which a worktree has not, so it is linted and tested
//! by the steps `cargo xtask app clippy` and `app test` are, with them left out
//! of its configuration. Clippy for Windows is `cargo xtask clippy-windows`,
//! from a machine that is not Windows; on Windows the workspace's own clippy is
//! that, and there is no step of it.
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
//! out, and xtask is tested where nothing of it is running. The step that builds
//! the `cf` is `cargo xtask build-cf`, which starts xtask again through cargo: no
//! step before it builds xtask in the shared target folder (clippy only checks,
//! and the tests of xtask have a folder of their own), so cargo finds the one
//! that is running fresh and leaves it as it is.

use std::ffi::OsString;
use std::io::Write;
use std::path::Path;

use crate::context::Context;
use crate::dispatch::{Command, Console, Failure, Run};
use crate::process::{self, Invocation};
use crate::{app, clippy_windows};

/// Where xtask's own tests are built, from the checkout's root: beside the
/// workspace's build folder, not in it.
const XTASK_TARGET: &str = "app/src-tauri/target/xtask-check";

pub const COMMANDS: &[Command] = &[Command {
    words: &["check"],
    about: "The full gate before a commit: rustfmt, clippy (workspace, app, Windows), the Rust tests (workspace, xtask, app), the linter and the Node tests, stopping at the first that fails",
    usage: "",
    run: Run::Native(run),
}];

fn run(context: &Context, args: &[OsString], console: &mut Console) -> Result<i32, Failure> {
    if !args.is_empty() {
        return Err(Failure::Usage("check takes no arguments".into()));
    }
    let windows = context.env.on_windows();
    if windows {
        writeln!(
            console.err,
            "xtask check: no clippy-windows step: on Windows the clippy of the workspace is clippy for Windows"
        )?;
    }
    run_steps(
        &steps(context, &clippy_windows::archiver()?, windows),
        |step| process::run(step, &context.env),
        console.err,
    )
}

/// What `check` runs, in order. `archiver` is the program cc-rs runs as the
/// archiver in the lint for Windows, which `windows` leaves out.
fn steps(context: &Context, archiver: &Path, windows: bool) -> Vec<Invocation> {
    let at = |program: &str, words: &[&str]| {
        Invocation::new(program, &context.root).args(words.iter().copied())
    };
    let mut steps = vec![
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
        app::clippy(context, &[]),
    ];
    if !windows {
        steps.push(clippy_windows::lint(context, &[], archiver));
    }
    steps.extend([
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
        app::test(context, &[]),
        at("npm", &["run", "lint"]),
        // The Node tests run the built cf: built again from the sources as they are,
        // as the `check:all` this replaces did first of all.
        at("cargo", &["xtask", "build-cf", "--offline"]),
        at("npm", &["test"]),
    ]);
    steps
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

    use std::path::PathBuf;

    use cf_base::env::Env;

    fn context() -> Context {
        Context {
            root: PathBuf::from("checkout"),
            env: Env::default(),
        }
    }

    /// The names of the variables a step sets.
    fn variables(step: &Invocation) -> Vec<String> {
        step.vars
            .iter()
            .map(|(name, _)| name.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn the_steps_are_the_gates_in_order_and_each_runs_where_and_as_its_own_command_does() {
        let context = context();
        let steps = steps(&context, Path::new("xtask"), false);
        let lines: Vec<_> = steps.iter().map(Invocation::display).collect();
        let own_target = context.path(XTASK_TARGET).display().to_string();
        assert_eq!(
            lines,
            [
                "cargo fmt --all --check".to_string(),
                "cargo clippy --workspace --exclude app --all-targets -- -D warnings".to_string(),
                "cargo clippy --offline --all-targets -- -D warnings".to_string(),
                "cargo clippy --offline --target x86_64-pc-windows-msvc --all-targets \
                 --workspace --exclude app -- -D warnings"
                    .to_string(),
                "cargo test --workspace --exclude app --exclude xtask".to_string(),
                format!("cargo test --package xtask --target-dir {own_target}"),
                "cargo test --offline --".to_string(),
                "npm run lint".to_string(),
                "cargo xtask build-cf --offline".to_string(),
                "npm test".to_string(),
            ]
        );
        let root = PathBuf::from("checkout");
        let app: PathBuf = ["checkout", "app", "src-tauri"].iter().collect();
        let cwds: Vec<_> = steps.iter().map(|step| step.cwd.clone()).collect();
        assert_eq!(
            cwds,
            [
                root.clone(),
                root.clone(),
                app.clone(),
                root.clone(),
                root.clone(),
                root.clone(),
                app,
                root.clone(),
                root.clone(),
                root
            ]
        );
        // The app's two steps leave its resources out; the lint for Windows is told
        // what it is told by its own command; the rest start from xtask's environment.
        assert_eq!(
            steps.iter().map(variables).collect::<Vec<_>>(),
            [
                vec![],
                vec![],
                vec!["TAURI_CONFIG".to_string()],
                vec![
                    "CC_x86_64_pc_windows_msvc".to_string(),
                    "AR_x86_64_pc_windows_msvc".to_string()
                ],
                vec![],
                vec![],
                vec!["TAURI_CONFIG".to_string()],
                vec![],
                vec![],
                vec![]
            ]
        );
    }

    #[test]
    fn the_cf_the_node_tests_run_is_built_again_just_before_them() {
        let steps = steps(&context(), Path::new("xtask"), false);
        let lines: Vec<_> = steps.iter().map(Invocation::display).collect();
        let [.., lint, build, test] = &lines[..] else {
            panic!("{lines:?}");
        };
        assert_eq!(
            [lint.as_str(), build.as_str(), test.as_str()],
            ["npm run lint", "cargo xtask build-cf --offline", "npm test"]
        );
    }

    #[test]
    fn the_lint_for_windows_is_the_one_its_command_runs_with_xtask_as_its_archiver() {
        let context = context();
        let archiver = Path::new("target").join("debug").join("xtask");
        let steps = steps(&context, &archiver, false);
        assert_eq!(steps[3], clippy_windows::lint(&context, &[], &archiver));
        assert_eq!(steps[2], app::clippy(&context, &[]));
        assert_eq!(steps[6], app::test(&context, &[]));
    }

    #[test]
    fn on_windows_the_workspaces_clippy_is_the_one_for_windows_and_there_is_no_step_of_it() {
        let context = context();
        let steps = steps(&context, Path::new("xtask"), true);
        let lines: Vec<_> = steps.iter().map(Invocation::display).collect();
        assert_eq!(lines.len(), 9);
        assert!(lines
            .iter()
            .all(|line| !line.contains("x86_64-pc-windows-msvc")));
        // Everything else is where it was.
        assert_eq!(
            lines[..3],
            [
                "cargo fmt --all --check",
                "cargo clippy --workspace --exclude app --all-targets -- -D warnings",
                "cargo clippy --offline --all-targets -- -D warnings"
            ]
        );
        assert_eq!(
            lines[5..],
            [
                "cargo test --offline --",
                "npm run lint",
                "cargo xtask build-cf --offline",
                "npm test"
            ]
        );
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

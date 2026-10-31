//! `cargo xtask app test` and `app clippy`: the app crate's tests and lint where
//! the app cannot be built as it ships. A worktree may not have
//! the resources the app's Tauri configuration names (the bundled `cf`), and its
//! build script refuses to go on without them. A test or a lint needs none, so
//! both leave them out of the configuration for the run, through the variable
//! Tauri reads one from, and run cargo in the app's folder, as CI's does.
//!
//! - `app test` is `cargo test --offline`. What follows the command is handed to
//!   the test binaries as it is, after a `--` of its own: a filter such as
//!   `portable::`, `--nocapture`.
//! - `app clippy` is `cargo clippy --offline --all-targets -- -D warnings`: the
//!   tests included and the warnings denied, as the gate lints the rest of the
//!   workspace. What follows the command goes to clippy after `-D warnings`.
//!
//! Both answer cargo's exit status and say nothing of their own. `check` runs
//! them too, by [`test`] and [`clippy`], and `clippy-windows` leaves the
//! resources out of its lint of the app the same way, by [`without_resources`].

use std::ffi::OsString;
use std::fs;

use crate::context::Context;
use crate::dispatch::{Command, Console, Failure, Run};
use crate::process::{self, Invocation};

/// Where the app crate is, from the checkout's root.
const APP: &str = "app/src-tauri";

/// The variable Tauri reads a configuration from, merged over the file's, and
/// what it is told: no resources to bundle.
const NO_RESOURCES: (&str, &str) = ("TAURI_CONFIG", r#"{"bundle":{"resources":null}}"#);

pub const COMMANDS: &[Command] = &[
    Command {
        words: &["app", "test"],
        about: "Run the app crate's tests, with its bundled resources left out",
        usage: "[test binary arguments: a filter such as portable::, --nocapture]",
        run: Run::Native(run_test),
    },
    Command {
        words: &["app", "clippy"],
        about: "Lint the app crate, tests included and warnings denied, with its bundled resources left out",
        usage: "[more clippy arguments]",
        run: Run::Native(run_clippy),
    },
];

fn run_test(context: &Context, args: &[OsString], _console: &mut Console) -> Result<i32, Failure> {
    Ok(process::run(&test(context, args), &context.env)?)
}

fn run_clippy(
    context: &Context,
    args: &[OsString],
    _console: &mut Console,
) -> Result<i32, Failure> {
    Ok(process::run(&clippy(context, args), &context.env)?)
}

/// The app's tests: `cargo test --offline -- <args>`, in the app's folder. Built into the
/// folder a linked `target` leads to ([`on_the_real_target`]).
pub(crate) fn test(context: &Context, args: &[OsString]) -> Invocation {
    on_the_real_target(context, in_the_app(context))
        .args(["test", "--offline", "--"])
        .args(args.iter().cloned())
}

/// `invocation`, told to build into the folder the app's `target` leads to where that is a
/// link (a tree whose build folder is on the builds disk, `builds-disk.sh place`), unless the
/// environment names a folder of its own. A test binary run through the link has a symlink
/// in its `current_exe()`, which Tauri's updater refuses (`StartingBinary`): the app's two
/// `real_updater` tests failed so in every such tree (T-968).
fn on_the_real_target(context: &Context, invocation: Invocation) -> Invocation {
    let target = context.path(APP).join("target");
    let linked = fs::symlink_metadata(&target).is_ok_and(|found| found.file_type().is_symlink());
    match fs::canonicalize(&target) {
        Ok(real) if linked && context.env.text("CARGO_TARGET_DIR").is_none() => {
            invocation.var("CARGO_TARGET_DIR", real)
        }
        _ => invocation,
    }
}

/// The app's lint: `cargo clippy --offline --all-targets -- -D warnings <args>`,
/// in the app's folder.
pub(crate) fn clippy(context: &Context, args: &[OsString]) -> Invocation {
    in_the_app(context)
        .args([
            "clippy",
            "--offline",
            "--all-targets",
            "--",
            "-D",
            "warnings",
        ])
        .args(args.iter().cloned())
}

/// `cargo` in the app's folder, with the app's resources left out.
fn in_the_app(context: &Context) -> Invocation {
    without_resources(Invocation::new("cargo", context.path(APP)))
}

/// `invocation` with the app's resources left out of Tauri's configuration, for
/// whatever of it builds the app's crate where the resources are not.
pub(crate) fn without_resources(invocation: Invocation) -> Invocation {
    invocation.var(NO_RESOURCES.0, NO_RESOURCES.1)
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

    fn words(words: &[&str]) -> Vec<OsString> {
        words.iter().map(OsString::from).collect()
    }

    /// The app's folder in `context`, with the platform's separator.
    fn app_folder() -> PathBuf {
        ["checkout", "app", "src-tauri"].iter().collect()
    }

    /// The one variable of an invocation that leaves the resources out.
    fn resources_left_out() -> Vec<(OsString, Option<OsString>)> {
        vec![(
            OsString::from("TAURI_CONFIG"),
            Some(OsString::from(r#"{"bundle":{"resources":null}}"#)),
        )]
    }

    #[test]
    fn the_app_tests_are_cargo_test_offline_in_the_app_with_the_words_after_a_double_dash() {
        let plain = test(&context(), &[]);
        assert_eq!(plain.display(), "cargo test --offline --");
        assert_eq!(plain.cwd, app_folder());

        let filtered = test(&context(), &words(&["portable::"]));
        assert_eq!(filtered.display(), "cargo test --offline -- portable::");

        // A `--` of the caller's is theirs, and goes on to the test binaries too.
        let given = test(&context(), &words(&["--", "--nocapture"]));
        assert_eq!(given.display(), "cargo test --offline -- -- --nocapture");
    }

    #[test]
    fn the_app_lint_is_cargo_clippy_offline_over_every_target_with_warnings_denied_and_the_words_last(
    ) {
        let plain = clippy(&context(), &[]);
        assert_eq!(
            plain.display(),
            "cargo clippy --offline --all-targets -- -D warnings"
        );
        assert_eq!(plain.cwd, app_folder());

        let more = clippy(&context(), &words(&["-W", "clippy::all"]));
        assert_eq!(
            more.display(),
            "cargo clippy --offline --all-targets -- -D warnings -W clippy::all"
        );
    }

    #[test]
    fn both_leave_the_apps_resources_out_of_the_configuration_and_nothing_else_of_the_environment()
    {
        assert_eq!(test(&context(), &[]).vars, resources_left_out());
        assert_eq!(clippy(&context(), &[]).vars, resources_left_out());
    }

    #[test]
    fn a_word_is_one_argument_whatever_is_in_it() {
        let odd = words(&["a b", "", "--flag=with space"]);
        assert_eq!(test(&context(), &odd).args[3..], odd[..]);
        assert_eq!(clippy(&context(), &odd).args[6..], odd[..]);
    }

    #[test]
    fn the_resources_are_left_out_of_whatever_invocation_is_given() {
        let lint = without_resources(Invocation::new("cargo", ".").var("A", "1"));
        assert_eq!(
            lint.vars,
            [
                (OsString::from("A"), Some(OsString::from("1"))),
                resources_left_out().remove(0)
            ]
        );
    }

    /// A checkout whose app has `target` as `kind` says (a link to a folder elsewhere, or a
    /// folder of its own), and that folder's real path where it is a link.
    #[cfg(unix)]
    fn checkout_with_target(link: bool, env: Env) -> (tempfile::TempDir, Context, PathBuf) {
        let root = tempfile::tempdir().unwrap();
        let app = root.path().join("app").join("src-tauri");
        let elsewhere = root.path().join("elsewhere").join("targets").join("tree");
        std::fs::create_dir_all(&app).unwrap();
        std::fs::create_dir_all(&elsewhere).unwrap();
        if link {
            std::os::unix::fs::symlink(&elsewhere, app.join("target")).unwrap();
        } else {
            std::fs::create_dir_all(app.join("target")).unwrap();
        }
        let real = std::fs::canonicalize(&elsewhere).unwrap();
        let context = Context {
            root: root.path().to_path_buf(),
            env,
        };
        (root, context, real)
    }

    /// The app's tests are built into the folder a linked `target` leads to, not through the
    /// link (T-968): Tauri's updater refuses a `current_exe()` that has a symlink in it, so the
    /// app's two `real_updater` tests failed in every tree whose build folder is on the builds
    /// disk (`builds-disk.sh place`), and so did the gate's step for them.
    #[cfg(unix)]
    #[test]
    fn the_app_tests_are_built_where_a_linked_target_leads_so_no_test_binary_runs_through_a_link() {
        let (_root, context, real) = checkout_with_target(true, Env::default());
        let mut expected = resources_left_out();
        expected.push((
            OsString::from("CARGO_TARGET_DIR"),
            Some(real.into_os_string()),
        ));
        assert_eq!(test(&context, &[]).vars, expected);
        // The lint runs no test binary, and the link serves it as it is.
        assert_eq!(clippy(&context, &[]).vars, resources_left_out());
    }

    #[cfg(unix)]
    #[test]
    fn a_target_that_is_a_folder_or_that_the_environment_names_is_left_as_it_is() {
        let (_root, context, _) = checkout_with_target(false, Env::default());
        assert_eq!(test(&context, &[]).vars, resources_left_out());
        let named = Env::from_vars([("CARGO_TARGET_DIR", "/somewhere/else")]);
        let (_root, context, _) = checkout_with_target(true, named);
        assert_eq!(test(&context, &[]).vars, resources_left_out());
    }

    #[test]
    fn the_two_commands_run_in_rust_and_are_no_scripts() {
        let lines: Vec<_> = COMMANDS
            .iter()
            .map(|command| command.words.join(" "))
            .collect();
        assert_eq!(lines, ["app test", "app clippy"]);
        assert!(COMMANDS
            .iter()
            .all(|command| matches!(command.run, Run::Native(_))));
    }
}

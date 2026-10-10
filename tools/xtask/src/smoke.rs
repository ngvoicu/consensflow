//! The packaged smoke and the updater smoke: the app as it is built, run as a
//! user would, and the update path proven end to end between apps. The updater
//! smoke is `updater_smoke` (landing S12); the packaged smoke is the smoke test
//! of `crates/cf-e2e` (landing S11), run on the app it is given:
//!
//! ```text
//! cargo xtask smoke                       # the bundle `npm --prefix app run build -- --bundles app` leaves
//! cargo xtask smoke --app <path>          # a built ConsensFlow.app: the signed one, or a copy
//! ```
//!
//! The app is the whole of the product, and it ships one daemon, so the smoke
//! runs once. `CONSENSFLOW_SMOKE_APP` names the app as well, for a workflow's
//! environment; `--app` wins, and a relative path is from the checkout's root.
//! `CONSENSFLOW_SMOKE_TIMEOUT_MS` and `CONSENSFLOW_SMOKE_KEEP=1` are the test's,
//! and reach it as they were set. The smoke's case is the one `cargo test`
//! ignores, so the command asks for the ignored cases (of the smoke test alone),
//! and for the test's own output, which says what the daemon that ran was. The
//! packaged app is a macOS bundle: elsewhere the command refuses, in words.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::context::Context;
use crate::dispatch::{Command, Console, Failure, Run};
use crate::process::{self, Invocation};
use crate::suites::{cargo_test, SMOKE};
use crate::updater_smoke;

/// The variable the smoke test reads the app from.
const APP_VARIABLE: &str = "CONSENSFLOW_SMOKE_APP";

/// What the command says of an `--app` that has no path after it.
const NO_PATH: &str = "--app takes a value (to start one with a dash: --app=-value)";

/// What the command says where there is no bundle to run: the Windows app is held by its own script.
const NOT_A_BUNDLE: &str = "smoke: the packaged smoke runs a macOS app bundle (ConsensFlow.app); \
    the Windows app is held by `npm --prefix app run smoke:windows -- <ConsensFlow.exe>`";

pub const COMMANDS: &[Command] = &[
    Command {
        words: &["smoke"],
        about: "Run the packaged smoke on the built app, or on the one --app names",
        usage: "[--app PATH]",
        run: Run::Native(run),
    },
    Command {
        words: &["smoke-updater"],
        about:
            "Prove the update path on the packaged app, from each release it can be installed from",
        usage: updater_smoke::USAGE,
        run: Run::Native(updater_smoke::run),
    },
];

fn run(context: &Context, args: &[OsString], console: &mut Console) -> Result<i32, Failure> {
    let app = app_of(args).map_err(Failure::Usage)?;
    if !cfg!(target_os = "macos") {
        writeln!(console.err, "{NOT_A_BUNDLE}")?;
        return Ok(1);
    }
    Ok(process::run(
        &invocation(context, app.as_deref()),
        &context.env,
    )?)
}

/// The app the command line names, as it was written: none when it names none.
/// `--app PATH` or `--app=PATH`, and nothing else.
fn app_of(args: &[OsString]) -> Result<Option<PathBuf>, String> {
    let mut app = None;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        let text = arg.to_string_lossy();
        let (name, joined) = match text.split_once('=') {
            Some((name, value)) if name.starts_with("--") => (name, Some(OsString::from(value))),
            _ => (text.as_ref(), None),
        };
        if name == "--app" {
            let value = match joined {
                Some(value) => value,
                // A value may not look like an option: `--app --keep` is a value missing.
                None => match rest.next() {
                    Some(next) if !next.to_string_lossy().starts_with('-') => next.clone(),
                    _ => return Err(NO_PATH.into()),
                },
            };
            // Nor may it be nothing: `--app=` names no app, and would name the checkout.
            if value.is_empty() {
                return Err(NO_PATH.into());
            }
            app = Some(PathBuf::from(value));
        } else if name.starts_with('-') {
            return Err(format!("unknown option: {name}"));
        } else {
            return Err(format!("unexpected argument: {text}"));
        }
    }
    Ok(app)
}

/// The packaged smoke on `app` (a relative path is from the checkout's root),
/// or on the app the test finds without being told: `cargo test -p cf-e2e
/// --test smoke -- --ignored --nocapture`, from the checkout's root. `candidate`
/// runs it on the app it built.
pub(crate) fn invocation(context: &Context, app: Option<&Path>) -> Invocation {
    let invocation = cargo_test(context, SMOKE, &["--".into(), "--nocapture".into()]);
    match app {
        Some(app) => invocation.var(APP_VARIABLE, context.root.join(app)),
        None => invocation,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use cf_base::env::Env;

    fn context() -> Context {
        Context {
            root: PathBuf::from("checkout"),
            env: Env::default(),
        }
    }

    fn words(line: &str) -> Vec<OsString> {
        line.split_whitespace().map(OsString::from).collect()
    }

    #[test]
    fn the_smoke_is_the_smoke_test_of_cf_e2e_asked_for_its_ignored_case_and_its_own_output() {
        let smoke = invocation(&context(), None);
        assert_eq!(
            smoke.display(),
            "cargo test -p cf-e2e --test smoke -- --ignored --nocapture"
        );
        assert_eq!(smoke.cwd, PathBuf::from("checkout"));
        // Nothing of the environment is set that the caller's own has: the app is the test's to find.
        assert!(smoke.vars.is_empty());
    }

    #[test]
    fn the_app_it_is_given_is_the_one_variable_it_sets_and_a_relative_path_is_from_the_root() {
        let given = |app: &str| {
            invocation(&context(), Some(Path::new(app)))
                .vars
                .into_iter()
                .collect::<Vec<_>>()
        };
        assert_eq!(
            given("dist/ConsensFlow.app"),
            [(
                OsString::from("CONSENSFLOW_SMOKE_APP"),
                Some(
                    Path::new("checkout")
                        .join("dist/ConsensFlow.app")
                        .into_os_string()
                )
            )]
        );
        // An absolute path is what it says, wherever the root is.
        let absolute = std::env::temp_dir().join("ConsensFlow.app");
        assert_eq!(
            given(&absolute.to_string_lossy()),
            [(
                OsString::from("CONSENSFLOW_SMOKE_APP"),
                Some(absolute.into_os_string())
            )]
        );
        // The rest of the line is the same with an app and without.
        assert_eq!(
            invocation(&context(), Some(Path::new("x.app"))).display(),
            invocation(&context(), None).display()
        );
    }

    #[test]
    fn the_app_is_named_by_app_with_a_value_or_joined_and_by_nothing_else() {
        assert_eq!(app_of(&[]), Ok(None));
        assert_eq!(
            app_of(&words("--app dist/ConsensFlow.app")),
            Ok(Some(PathBuf::from("dist/ConsensFlow.app")))
        );
        assert_eq!(
            app_of(&words("--app=dist/ConsensFlow.app")),
            Ok(Some(PathBuf::from("dist/ConsensFlow.app")))
        );
        // A path with spaces in it is one word; the last one given counts.
        let spaced: Vec<OsString> = ["--app", "A Folder/Consens Flow.app"]
            .iter()
            .map(OsString::from)
            .collect();
        assert_eq!(
            app_of(&spaced),
            Ok(Some(PathBuf::from("A Folder/Consens Flow.app")))
        );
        assert_eq!(
            app_of(&words("--app a.app --app b.app")),
            Ok(Some(PathBuf::from("b.app")))
        );
        // A value that starts with a dash is given joined.
        assert_eq!(
            app_of(&words("--app=-odd.app")),
            Ok(Some(PathBuf::from("-odd.app")))
        );
    }

    #[test]
    fn a_word_that_is_no_option_of_the_smoke_or_has_no_value_is_refused_in_words() {
        for (line, said) in [
            ("--app", NO_PATH),
            ("--app --keep", NO_PATH),
            ("--app=", NO_PATH),
            ("--keep", "unknown option: --keep"),
            ("--nope=1", "unknown option: --nope"),
            (
                "dist/ConsensFlow.app",
                "unexpected argument: dist/ConsensFlow.app",
            ),
            ("--app a.app extra", "unexpected argument: extra"),
        ] {
            assert_eq!(app_of(&words(line)), Err(said.to_owned()), "{line}");
        }
    }

    #[test]
    fn both_smokes_run_in_rust_and_are_no_scripts() {
        for words in [["smoke"], ["smoke-updater"]] {
            let command = COMMANDS.iter().find(|command| command.words == words);
            assert!(
                matches!(command.map(|command| &command.run), Some(Run::Native(_))),
                "{words:?}"
            );
        }
    }

    #[test]
    fn a_line_that_is_refused_is_a_usage_error_and_starts_nothing() {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let mut console = Console {
            out: &mut out,
            err: &mut err,
        };
        let refused = run(&context(), &words("--app"), &mut console).unwrap_err();
        assert!(matches!(refused, Failure::Usage(_)), "{refused}");
        assert!(out.is_empty() && err.is_empty());
    }

    #[test]
    fn where_there_is_no_bundle_the_smoke_says_what_holds_the_windows_app() {
        assert!(NOT_A_BUNDLE.contains("macOS app bundle"));
        assert!(NOT_A_BUNDLE.contains("smoke:windows"));
        assert!(!NOT_A_BUNDLE.contains('\n'));
    }
}

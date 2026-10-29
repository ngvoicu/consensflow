//! `cargo xtask unreachable-pub`: a ceiling on the `pub` items of `cf-harness` that no
//! other crate can reach.
//!
//! rustc's `unreachable_pub` lint names an item declared `pub` that its crate does not
//! export: a `pub fn` in a module that is `pub(crate)`, which says more than it is. The
//! harnesses' crate has some still, mostly the channels' functions that the test kit
//! exports for the tests of other crates, and each is a promise to a reader that something
//! outside the crate may use it. Closing them is a work of its own (the audit of T-911 and
//! `consensflow-sme`'s adapter work); this keeps the rest from growing meanwhile.
//!
//! It runs `cargo clippy -p cf-harness --lib -- -W unreachable_pub`: the crate as the
//! daemon builds it, without the test kit, which hides the sites it exports. It counts the
//! places the lint names in `cf-harness` itself, once each (the workspace crates it builds
//! with are warned about too, and are not its concern), and fails when there are more
//! than [`CEILING`]. When there are fewer it passes and says so: lower the ceiling in the
//! change that closed them, so that the room they leave is not taken by others.

use std::collections::BTreeSet;
use std::ffi::OsString;

use serde_json::Value;

use crate::context::Context;
use crate::dispatch::{Command, Console, Failure, Run};
use crate::process::{self, Captured, Invocation};

/// The package held to the ceiling.
const PACKAGE: &str = "cf-harness";

/// Its library, as cargo's messages name the target.
const LIBRARY: &str = "cf_harness";

/// The most sites the crate may have, measured on 2026-10-29 on the merge of `f1500dc7c`
/// with eight of `shared/input_box.rs` and `shared/net.rs` closed (40 before; the ninth of
/// the audit's, `InputBox`, is exported by `contract` since main's chief-meeting phase 1, so
/// it is no site). 31 of the 32 were there at the audit; the 32nd is
/// `codex::channel::Channel::shown_refreshed`, which main's T-842 added as the test kit
/// exports its siblings. **Lower it when a site is closed; never raise it.**
const CEILING: usize = 32;

pub const COMMANDS: &[Command] = &[Command {
    words: &["unreachable-pub"],
    about:
        "Fail when cf-harness has more pub items its crate does not export than the ceiling holds",
    usage: "",
    run: Run::Native(run),
}];

fn run(context: &Context, args: &[OsString], console: &mut Console) -> Result<i32, Failure> {
    if !args.is_empty() {
        return Err(Failure::Usage("unreachable-pub takes no arguments".into()));
    }
    answer(
        context,
        CEILING,
        |lint| process::capture(lint, &context.env),
        console,
    )
}

/// The lint, from the checkout's root.
fn lint(context: &Context) -> Invocation {
    Invocation::new("cargo", &context.root).args([
        "clippy",
        "-p",
        PACKAGE,
        "--lib",
        "--message-format=json",
        "--",
        "-W",
        "unreachable_pub",
    ])
}

/// Runs the lint with `capture` and judges what it said against `ceiling`: 0 when the crate
/// is within it, 1 when it is over or the lint said nothing that could be read, else the
/// status the lint ended with.
fn answer(
    context: &Context,
    ceiling: usize,
    capture: impl FnOnce(&Invocation) -> Result<Captured, process::Failure>,
    console: &mut Console,
) -> Result<i32, Failure> {
    let lint = lint(context);
    let captured = capture(&lint)?;
    if captured.code != 0 {
        console.err.write_all(captured.stderr.as_bytes())?;
        writeln!(
            console.err,
            "unreachable-pub: `{}` ended with status {}",
            lint.display(),
            captured.code
        )?;
        return Ok(captured.code);
    }
    let found = sites(&captured.stdout);
    let count = found.len();
    if count == 0 && ceiling > 0 {
        writeln!(
            console.err,
            "unreachable-pub: `{}` named no site in {PACKAGE}, where the ceiling is {ceiling}: \
             the lint's messages are not read as they were",
            lint.display()
        )?;
        return Ok(1);
    }
    if count > ceiling {
        writeln!(
            console.err,
            "unreachable-pub: {PACKAGE} has {count} pub items its crate does not export, \
             where the ceiling is {ceiling}:"
        )?;
        for (file, line) in &found {
            writeln!(console.err, "  {file}:{line}")?;
        }
        writeln!(
            console.err,
            "Make the new item `pub(crate)` or private, or export it from the crate on purpose. \
             The ceiling is never raised."
        )?;
        return Ok(1);
    }
    if count < ceiling {
        writeln!(
            console.out,
            "unreachable-pub: {PACKAGE} has {count} pub items its crate does not export, \
             under the ceiling of {ceiling}: lower CEILING in tools/xtask/src/unreachable_pub.rs \
             to {count}"
        )?;
    } else {
        writeln!(
            console.out,
            "unreachable-pub: {PACKAGE} has {count} pub items its crate does not export (the ceiling)"
        )?;
    }
    Ok(0)
}

/// The places the lint names in `cf-harness`, from cargo's JSON messages: the first line of
/// the span the warning points at, once each however many times cargo said it.
fn sites(messages: &str) -> BTreeSet<(String, u64)> {
    messages
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|message| {
            message["reason"] == "compiler-message"
                && message["target"]["name"] == LIBRARY
                && message["message"]["code"]["code"] == "unreachable_pub"
        })
        .filter_map(|message| {
            let spans = message["message"]["spans"].as_array()?;
            let span = spans.iter().find(|span| span["is_primary"] == true)?;
            Some((
                span["file_name"].as_str()?.to_owned(),
                span["line_start"].as_u64()?,
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::path::PathBuf;

    use cf_base::env::Env;
    use serde_json::json;

    fn context() -> Context {
        Context {
            root: PathBuf::from("checkout"),
            env: Env::default(),
        }
    }

    /// A message of cargo's about `target`: what `lint` is said of at `line` of `file`.
    fn warning(target: &str, lint: Option<&str>, file: &str, line: u64) -> String {
        json!({
            "reason": "compiler-message",
            "package_id": format!("path+file:///checkout/crates/{target}#3.0.6"),
            "target": { "kind": ["lib"], "name": target },
            "message": {
                "code": lint.map(|code| json!({ "code": code })),
                "level": "warning",
                "spans": [
                    { "file_name": "crates/other.rs", "line_start": 1, "is_primary": false },
                    { "file_name": file, "line_start": line, "is_primary": true }
                ]
            }
        })
        .to_string()
    }

    fn captured(code: i32, lines: &[String]) -> Captured {
        Captured {
            code,
            stdout: lines.join("\n"),
            stderr: String::new(),
        }
    }

    fn answered(
        ceiling: usize,
        said: Captured,
    ) -> (Result<i32, Failure>, String, String, Vec<String>) {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let mut ran = Vec::new();
        let status = answer(
            &context(),
            ceiling,
            |lint| {
                ran.push(lint.display());
                Ok(said)
            },
            &mut Console {
                out: &mut out,
                err: &mut err,
            },
        );
        (
            status,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
            ran,
        )
    }

    #[test]
    fn the_lint_is_the_one_the_daemon_builds_the_crate_with_run_from_the_root() {
        let lint = lint(&context());
        assert_eq!(
            lint.display(),
            "cargo clippy -p cf-harness --lib --message-format=json -- -W unreachable_pub"
        );
        assert_eq!(lint.cwd, PathBuf::from("checkout"));
    }

    #[test]
    fn the_sites_are_the_places_the_lint_names_in_the_harnesses_crate_once_each() {
        let messages = [
            "   Checking cf-harness v3.0.6".to_owned(),
            json!({ "reason": "compiler-artifact", "target": { "name": LIBRARY } }).to_string(),
            warning(
                "cf_harness",
                Some("unreachable_pub"),
                "crates/cf-harness/src/a.rs",
                3,
            ),
            // Said again, as cargo does for a target it checks twice.
            warning(
                "cf_harness",
                Some("unreachable_pub"),
                "crates/cf-harness/src/a.rs",
                3,
            ),
            warning(
                "cf_harness",
                Some("unreachable_pub"),
                "crates/cf-harness/src/b.rs",
                9,
            ),
            // Another lint of the crate, another crate of the workspace, and a message with no code.
            warning(
                "cf_harness",
                Some("dead_code"),
                "crates/cf-harness/src/c.rs",
                5,
            ),
            warning(
                "cf_process",
                Some("unreachable_pub"),
                "crates/cf-process/src/d.rs",
                7,
            ),
            warning("cf_harness", None, "crates/cf-harness/src/e.rs", 1),
        ];
        assert_eq!(
            sites(&messages.join("\n")),
            BTreeSet::from([
                ("crates/cf-harness/src/a.rs".to_owned(), 3),
                ("crates/cf-harness/src/b.rs".to_owned(), 9),
            ])
        );
    }

    #[test]
    fn a_crate_at_the_ceiling_passes_and_says_so() {
        let said = captured(
            0,
            &[
                warning("cf_harness", Some("unreachable_pub"), "a.rs", 1),
                warning("cf_harness", Some("unreachable_pub"), "b.rs", 2),
            ],
        );
        let (status, out, err, ran) = answered(2, said);
        assert_eq!(status.unwrap(), 0);
        assert_eq!(
            out,
            "unreachable-pub: cf-harness has 2 pub items its crate does not export (the ceiling)\n"
        );
        assert_eq!(err, "");
        assert_eq!(ran.len(), 1);
    }

    #[test]
    fn a_crate_over_the_ceiling_fails_and_lists_every_site_in_order() {
        let said = captured(
            0,
            &[
                warning("cf_harness", Some("unreachable_pub"), "b.rs", 2),
                warning("cf_harness", Some("unreachable_pub"), "a.rs", 10),
                warning("cf_harness", Some("unreachable_pub"), "a.rs", 4),
            ],
        );
        let (status, out, err, _) = answered(2, said);
        assert_eq!(status.unwrap(), 1);
        assert_eq!(out, "");
        assert_eq!(
            err,
            "unreachable-pub: cf-harness has 3 pub items its crate does not export, where the \
             ceiling is 2:\n  a.rs:4\n  a.rs:10\n  b.rs:2\n\
             Make the new item `pub(crate)` or private, or export it from the crate on purpose. \
             The ceiling is never raised.\n"
        );
    }

    #[test]
    fn a_crate_under_the_ceiling_passes_and_says_to_lower_it() {
        let said = captured(
            0,
            &[warning("cf_harness", Some("unreachable_pub"), "a.rs", 1)],
        );
        let (status, out, _, _) = answered(2, said);
        assert_eq!(status.unwrap(), 0);
        assert_eq!(
            out,
            "unreachable-pub: cf-harness has 1 pub items its crate does not export, under the \
             ceiling of 2: lower CEILING in tools/xtask/src/unreachable_pub.rs to 1\n"
        );
    }

    #[test]
    fn a_lint_that_named_no_site_where_some_are_expected_is_not_a_pass() {
        let said = captured(0, &["   Checking cf-harness v3.0.6".to_owned()]);
        let (status, out, err, _) = answered(2, said);
        assert_eq!(status.unwrap(), 1);
        assert_eq!(out, "");
        assert!(err.contains("named no site in cf-harness"), "{err}");
    }

    #[test]
    fn a_lint_that_ends_with_a_status_answers_it_with_what_cargo_said() {
        let said = Captured {
            code: 101,
            stdout: String::new(),
            stderr: "error[E0425]: cannot find value `x`\n".to_owned(),
        };
        let (status, out, err, _) = answered(2, said);
        assert_eq!(status.unwrap(), 101);
        assert_eq!(out, "");
        assert_eq!(
            err,
            "error[E0425]: cannot find value `x`\nunreachable-pub: `cargo clippy -p cf-harness \
             --lib --message-format=json -- -W unreachable_pub` ended with status 101\n"
        );
    }

    #[test]
    fn unreachable_pub_takes_no_arguments_and_starts_nothing_without_them() {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let mut console = Console {
            out: &mut out,
            err: &mut err,
        };
        let refused = run(&context(), &[OsString::from("--fast")], &mut console).unwrap_err();
        assert_eq!(refused.to_string(), "unreachable-pub takes no arguments");
        assert!(matches!(refused, Failure::Usage(_)));
        assert!(out.is_empty() && err.is_empty());
    }

    #[test]
    fn unreachable_pub_runs_in_rust() {
        assert!(matches!(COMMANDS[0].run, Run::Native(_)));
        assert_eq!(COMMANDS[0].words, ["unreachable-pub"]);
    }
}

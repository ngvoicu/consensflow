//! `cargo xtask rust-version`: the Rust in use is the one `.github/rust-version` names.
//!
//! Every workflow that builds installs that version and makes it the default
//! (`.github/actions/setup`, `windows-build.yml`, `release.yml`), so CI and a release build
//! with one Rust whatever the runner's `stable` is. The version matters beyond the build: the
//! engine lower-cases text with Rust's `str::to_lowercase`, and the page with Node's
//! `toLowerCase`. The golden of the second (`tests/goldens/text.json`, `lowerCase`) holds the
//! first only for the Rust it was made with. Rust 1.100.0 lower-cases twenty code points
//! Node 26 leaves as they are (T-1523), and that was found only as a red golden. This says a
//! drift as a drift, in one line naming both versions, before anything builds: the first step
//! of `cargo xtask check` and of CI's workspace job.
//!
//! There is no `rust-toolchain.toml`: rustup would install the version it names on any
//! machine a cargo command runs on, the owner's included.

use std::ffi::OsString;
use std::fs;

use crate::context::Context;
use crate::dispatch::{Command, Console, Failure, Run};
use crate::process::{self, Captured, Invocation};

/// The file that names the Rust every build uses, from the checkout's root.
pub(crate) const FILE: &str = ".github/rust-version";

pub const COMMANDS: &[Command] = &[Command {
    words: &["rust-version"],
    about: "Fail when the rustc in use is not the version .github/rust-version names, which the lower-casing golden depends on",
    usage: "",
    run: Run::Native(run),
}];

fn run(context: &Context, args: &[OsString], console: &mut Console) -> Result<i32, Failure> {
    if !args.is_empty() {
        return Err(Failure::Usage("rust-version takes no arguments".into()));
    }
    let pinned = match fs::read_to_string(context.path(FILE)) {
        Ok(text) => text,
        Err(cause) => {
            writeln!(
                console.err,
                "rust-version: {FILE} could not be read: {cause}"
            )?;
            return Ok(1);
        }
    };
    answer(
        context,
        &pinned,
        |rustc| process::capture(rustc, &context.env),
        console,
    )
}

/// The rustc cargo runs: `$RUSTC` when it is set, else the one on the PATH (rustup's, which
/// answers for the default toolchain), from the checkout's root.
fn rustc(context: &Context) -> Invocation {
    Invocation::new(context.env.text("RUSTC").unwrap_or("rustc"), &context.root).arg("--version")
}

/// Asks `rustc --version` with `capture` and judges it against `pinned`, the file's text:
/// 0 when they are one version, 1 when not or when it cannot be read, else rustc's status.
fn answer(
    context: &Context,
    pinned: &str,
    capture: impl FnOnce(&Invocation) -> Result<Captured, process::Failure>,
    console: &mut Console,
) -> Result<i32, Failure> {
    let rustc = rustc(context);
    let captured = capture(&rustc)?;
    if captured.code != 0 {
        console.err.write_all(captured.stderr.as_bytes())?;
        writeln!(
            console.err,
            "rust-version: `{}` ended with status {}",
            rustc.display(),
            captured.code
        )?;
        return Ok(captured.code);
    }
    match judged(pinned, &captured.stdout) {
        Ok(line) => {
            writeln!(console.out, "{line}")?;
            Ok(0)
        }
        Err(line) => {
            writeln!(console.err, "{line}")?;
            Ok(1)
        }
    }
}

/// `said`, what `rustc --version` printed, against `pinned`: the line to say when they are one
/// version, else the one line that says both.
fn judged(pinned: &str, said: &str) -> Result<String, String> {
    let pinned = pinned.trim();
    if pinned.is_empty() {
        return Err(format!("rust-version: {FILE} names no version"));
    }
    let mut words = said.split_whitespace();
    let (Some("rustc"), Some(version)) = (words.next(), words.next()) else {
        return Err(format!(
            "rust-version: `rustc --version` said no version: {}",
            said.trim()
        ));
    };
    if version == pinned {
        Ok(format!(
            "rust-version: rustc {version}, the version {FILE} names"
        ))
    } else {
        Err(format!(
            "rust-version: rustc is {version}, but {FILE} names {pinned}: the engine's \
             lower-casing golden (cf-engine's tests/text goldens_handoff, against Node's \
             toLowerCase) holds only for the Rust that file names; build with {pinned}, or \
             change the file with the golden"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};

    use cf_base::env::Env;

    fn context(env: Env) -> Context {
        Context {
            root: PathBuf::from("checkout"),
            env,
        }
    }

    fn said(code: i32, stdout: &str) -> Captured {
        Captured {
            code,
            stdout: stdout.to_owned(),
            stderr: if code == 0 {
                String::new()
            } else {
                "error: no such toolchain\n".to_owned()
            },
        }
    }

    fn answered(env: Env, pinned: &str, rustc: Captured) -> (i32, String, String, Vec<String>) {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let mut ran = Vec::new();
        let status = answer(
            &context(env),
            pinned,
            |asked| {
                ran.push(asked.display());
                Ok(rustc)
            },
            &mut Console {
                out: &mut out,
                err: &mut err,
            },
        )
        .unwrap();
        (
            status,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
            ran,
        )
    }

    #[test]
    fn a_rustc_of_another_version_is_said_in_one_line_with_both_versions_and_the_golden() {
        let (status, out, err, ran) = answered(
            Env::default(),
            "1.99.1\n",
            said(0, "rustc 1.100.0 (0ef1bc386 2026-11-09)\n"),
        );
        assert_eq!(ran, ["rustc --version"]);
        assert_eq!(status, 1);
        assert_eq!(out, "");
        assert_eq!(
            err,
            "rust-version: rustc is 1.100.0, but .github/rust-version names 1.99.1: the \
             engine's lower-casing golden (cf-engine's tests/text goldens_handoff, against \
             Node's toLowerCase) holds only for the Rust that file names; build with 1.99.1, or \
             change the file with the golden\n"
        );
    }

    #[test]
    fn the_rustc_of_the_version_the_file_names_passes_and_says_so() {
        let (status, out, err, _) = answered(
            Env::default(),
            "1.99.1\n",
            said(0, "rustc 1.99.1 (58cf8f9c3 2026-10-13)\n"),
        );
        assert_eq!(status, 0, "{err}");
        assert_eq!(
            out,
            "rust-version: rustc 1.99.1, the version .github/rust-version names\n"
        );
        assert_eq!(err, "");
    }

    #[test]
    fn a_version_that_only_begins_as_the_pinned_one_is_another() {
        for other in [
            "rustc 1.99.10 (aaaaaaaaa 2026-12-01)",
            "rustc 1.99.1-nightly (bbbbbbbbb 2026-10-01)",
        ] {
            let (status, _, err, _) = answered(Env::default(), "1.99.1", said(0, other));
            assert_eq!(status, 1, "{other}");
            assert!(err.starts_with("rust-version: rustc is 1.99.1"), "{err}");
        }
    }

    #[test]
    fn a_file_or_an_answer_that_names_no_version_is_said_as_such() {
        let (status, _, err, _) = answered(Env::default(), "\n", said(0, "rustc 1.99.1 (x y)\n"));
        assert_eq!(status, 1);
        assert_eq!(err, "rust-version: .github/rust-version names no version\n");
        let (status, _, err, _) = answered(Env::default(), "1.99.1", said(0, "something else\n"));
        assert_eq!(status, 1);
        assert_eq!(
            err,
            "rust-version: `rustc --version` said no version: something else\n"
        );
    }

    #[test]
    fn the_rustc_asked_is_the_one_cargo_is_told_to_run_and_a_status_of_its_own_is_passed_on() {
        let (status, out, err, ran) = answered(
            Env::default().with_var("RUSTC", "/opt/rust/bin/rustc"),
            "1.99.1",
            said(1, ""),
        );
        assert_eq!(ran, ["/opt/rust/bin/rustc --version"]);
        assert_eq!(status, 1);
        assert_eq!(out, "");
        assert_eq!(
            err,
            "error: no such toolchain\nrust-version: `/opt/rust/bin/rustc --version` ended with status 1\n"
        );
    }

    /// How a step of `.github` installs Rust: the version the file names, and nothing else.
    const INSTALL: &str = "rustup toolchain install \"$(cat .github/rust-version)\"";
    const DEFAULT: &str = "rustup default \"$(cat .github/rust-version)\"";

    /// What `.github` holds, by its path under it: every workflow, and every action of its own.
    fn github() -> BTreeMap<String, String> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.github");
        let mut found = BTreeMap::new();
        for entry in fs::read_dir(root.join("workflows")).unwrap().flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            found.insert(
                format!("workflows/{name}"),
                fs::read_to_string(entry.path()).unwrap(),
            );
        }
        for entry in fs::read_dir(root.join("actions")).unwrap().flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let action = entry.path().join("action.yml");
            found.insert(
                format!("actions/{name}/action.yml"),
                fs::read_to_string(action).unwrap(),
            );
        }
        found
    }

    fn code(text: &str) -> impl Iterator<Item = &str> {
        text.lines()
            .filter(|line| !line.trim_start().starts_with('#'))
    }

    /// T-1523: a runner's `stable` moved to 1.100.0 between two runs of one day, and the
    /// lower-casing golden went red on both platforms. Every step that installs Rust installs
    /// the version the file names and makes it the default, a release's builds included (a
    /// release built with another Rust would lower-case otherwise than the page); a workflow
    /// that runs cargo installs it, by itself or through the gate's setup.
    #[test]
    fn every_workflow_builds_with_the_rust_the_file_names() {
        let pinned = fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .join(FILE),
        )
        .unwrap();
        let version = pinned.trim();
        assert!(
            judged(&pinned, &format!("rustc {version} (0 0)")).is_ok(),
            "{FILE} names one version: {pinned:?}"
        );
        let files = github();
        let mut installs = BTreeMap::new();
        for (file, text) in &files {
            for line in code(text).filter(|line| line.contains("rustup")) {
                assert!(
                    line.contains(INSTALL) && line.contains(DEFAULT) && !line.contains("stable"),
                    "{file} installs a Rust that is not the one {FILE} names:\n{line}"
                );
                *installs.entry(file.as_str()).or_insert(0) += 1;
            }
            let builds = code(text).any(|line| line.contains("cargo "));
            let set_up = text.contains("uses: ./.github/actions/setup");
            assert!(
                !builds || set_up || installs.contains_key(file.as_str()),
                "{file} runs cargo with the runner's own Rust"
            );
        }
        assert_eq!(
            installs,
            BTreeMap::from([
                ("actions/setup/action.yml", 1),
                ("workflows/release.yml", 4),
                ("workflows/windows-build.yml", 1),
            ])
        );
        assert!(
            files["actions/setup/action.yml"].contains("--component clippy,rustfmt"),
            "the gate's setup installs the lint and the formatter with it"
        );
    }

    /// The check is said before the workspace's tests in CI, on both platforms: the job the
    /// lower-casing golden runs in.
    #[test]
    fn cis_workspace_job_says_a_drift_before_its_tests() {
        let files = github();
        let job: Vec<&str> = files["workflows/ci.yml"]
            .lines()
            .skip_while(|line| *line != "  workspace:")
            .skip(1)
            .take_while(|line| !(line.starts_with("  ") && !line.starts_with("   ")))
            .collect();
        let at = |step: &str| job.iter().position(|line| *line == step);
        let check =
            at("      - run: cargo xtask rust-version").expect("the workspace job checks the Rust");
        let setup = at("      - uses: ./.github/actions/setup").unwrap();
        let tests =
            at("      - run: cargo test --workspace --exclude app --exclude cf-e2e").unwrap();
        assert!(setup < check && check < tests, "{job:#?}");
    }
}

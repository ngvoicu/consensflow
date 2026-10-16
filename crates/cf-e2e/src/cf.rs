//! The programs under test: the `cf` and the pane host (`consensflow-bridge`),
//! the release builds of this checkout, each made once for a run of the tests;
//! and the test binary `harness-ask`, which the live tests ask what they need of
//! the harness code of the product (and which depends on no crate of ours: it
//! is a program, started as the others are). They are built, not found: `bin/cf`
//! is whatever the last `cargo xtask build-cf` left, and a suite that ran that
//! could pass against sources it never saw (a change in hand not built yet, a
//! plant). `cargo build` knows when the build is up to date, and then it takes
//! a moment.
//!
//! A run that built `cf` and the pane host before it, from the sources as they
//! are, says where (`BUILT`), and they are taken from there and not built: under
//! nextest each test is a process of its own, which would build them for itself,
//! and cargo links a binary again as it builds, taking it from under a test of
//! another process that is starting it (the plants' runner, 2026-10-16).

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::process::{own_var, Run};
use crate::{checkout, Error, Result};

/// The variable that names the folder `cf` and the pane host were built in for
/// the run, before it, from the sources as they are.
pub const BUILT: &str = "CF_E2E_BUILT";

/// The folder the workspace builds a release binary in, from the checkout's
/// root: `.cargo/config.toml` puts the build folder at `app/src-tauri/target`.
const RELEASE: [&str; 4] = ["app", "src-tauri", "target", "release"];

/// What a build leaves, or why it could not be made.
type Built = OnceLock<std::result::Result<PathBuf, String>>;

/// The `cf` under test: built, or found up to date, when this is first asked
/// for, and the same one for every case of the run; or the one built before the
/// run (`BUILT`).
pub fn binary() -> Result<&'static Path> {
    static BUILT: Built = OnceLock::new();
    built(&BUILT, "cf", "cf", &[], given())
}

/// The pane host under test, `consensflow-bridge`: the program that gives each
/// window its terminal, which the app starts beside the daemon. Built as `cf`
/// is, once for a run, or the one built before it.
pub fn pane_host() -> Result<&'static Path> {
    static BUILT: Built = OnceLock::new();
    built(&BUILT, "cf-panes", "consensflow-bridge", &[], given())
}

/// The test binary `harness-ask`, which answers what a live test asks of the
/// harness code of the product: the harness crate's own, built with the feature
/// that makes it (it ships with nothing). Built as `cf` is, once for a run, and
/// the first time a test asks; never before the run.
pub fn harness_ask() -> Result<&'static Path> {
    static BUILT: Built = OnceLock::new();
    built(
        &BUILT,
        "cf-harness",
        "harness-ask",
        &["--features", "test-support"],
        None,
    )
}

/// The folder the run built `cf` and the pane host in before it, where it says one.
fn given() -> Option<PathBuf> {
    own_var(BUILT)
        .filter(|folder| !folder.is_empty())
        .map(PathBuf::from)
}

/// The build of the binary `bin` of the package `package`, with the cargo words
/// `more` after the rest, made the first time it is asked for and kept in `cell`;
/// or, where the run built it before in the folder `before`, that one.
fn built(
    cell: &'static Built,
    package: &str,
    bin: &str,
    more: &[&str],
    before: Option<PathBuf>,
) -> Result<&'static Path> {
    cell.get_or_init(|| match before {
        Some(folder) => taken(&folder, bin),
        None => build(
            Run::new(env!("CARGO")),
            &checkout::root(),
            package,
            bin,
            more,
        ),
    })
    .as_deref()
    .map_err(|message| Error::Build(message.clone()))
}

/// The binary `bin` the run built in `folder` before it: an error where it is not
/// there, which no build is made for.
fn taken(folder: &Path, bin: &str) -> std::result::Result<PathBuf, String> {
    let built = folder.join(format!("{bin}{}", std::env::consts::EXE_SUFFIX));
    if built.is_file() {
        Ok(built)
    } else {
        Err(format!(
            "{BUILT} says {bin} was built in {}, and there is none there",
            folder.display()
        ))
    }
}

/// Builds the binary `bin` of the package `package` in the checkout at `root`
/// with `cargo` (the program that builds, given its first words if it has any),
/// and the cargo words `more`, and answers where it is.
fn build(
    cargo: Run,
    root: &Path,
    package: &str,
    bin: &str,
    more: &[&str],
) -> std::result::Result<PathBuf, String> {
    let ran = cargo
        .args([
            "build",
            "--release",
            "--locked",
            "-p",
            package,
            "--bin",
            bin,
        ])
        .args(more)
        .cwd(root)
        .inheriting_env()
        .unlimited()
        .run()
        .map_err(|cause| cause.to_string())?;
    if ran.code != Some(0) {
        return Err(format!("building {bin} did not succeed: {ran}"));
    }
    let name = format!("{bin}{}", std::env::consts::EXE_SUFFIX);
    let built = RELEASE
        .iter()
        .fold(root.to_path_buf(), |folder, part| folder.join(part))
        .join(name);
    if !built.is_file() {
        return Err(format!(
            "cargo built {bin}, but there is none at {}: is the build folder somewhere \
             else than .cargo/config.toml says?",
            built.display()
        ));
    }
    Ok(built)
}

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use super::*;

    use crate::files;

    /// A `cargo` that is a shell script: run by the shell, since a script that
    /// has just been written and is started while another thread starts a
    /// program of its own may be refused as a file still open for writing.
    fn cargo(script: &str) -> (tempfile::TempDir, Run) {
        let folder = tempfile::tempdir().unwrap();
        let file = folder.path().join("cargo.sh");
        files::write(&file, script).unwrap();
        (folder, Run::new("/bin/sh").arg(file))
    }

    #[test]
    fn the_build_is_asked_for_in_the_checkout_and_the_cf_it_leaves_is_the_answer() {
        let root = tempfile::tempdir().unwrap();
        let (_folder, cargo) = cargo(
            "pwd > asked-in; echo \"$@\" > asked; \
             mkdir -p app/src-tauri/target/release && : > app/src-tauri/target/release/cf",
        );
        let built = build(cargo, root.path(), "cf", "cf", &[]).unwrap();
        assert_eq!(
            built,
            root.path()
                .join("app/src-tauri/target/release")
                .join(format!("cf{}", std::env::consts::EXE_SUFFIX))
        );
        assert_eq!(
            files::read_string(&root.path().join("asked")).unwrap(),
            "build --release --locked -p cf --bin cf\n"
        );
        let asked_in = files::read_string(&root.path().join("asked-in")).unwrap();
        assert_eq!(
            asked_in.trim_end(),
            std::fs::canonicalize(root.path())
                .unwrap()
                .to_string_lossy()
        );
    }

    #[test]
    fn the_pane_host_is_asked_for_as_a_binary_of_its_own_package() {
        let root = tempfile::tempdir().unwrap();
        let (_folder, cargo) = cargo(
            "echo \"$@\" > asked; \
             mkdir -p app/src-tauri/target/release && : > app/src-tauri/target/release/consensflow-bridge",
        );
        let built = build(cargo, root.path(), "cf-panes", "consensflow-bridge", &[]).unwrap();
        assert_eq!(
            built,
            root.path()
                .join("app/src-tauri/target/release")
                .join(format!(
                    "consensflow-bridge{}",
                    std::env::consts::EXE_SUFFIX
                ))
        );
        assert_eq!(
            files::read_string(&root.path().join("asked")).unwrap(),
            "build --release --locked -p cf-panes --bin consensflow-bridge\n"
        );
    }

    #[test]
    fn a_build_that_fails_says_what_cargo_said() {
        let root = tempfile::tempdir().unwrap();
        let (_folder, cargo) = cargo("echo 'error: no such package' >&2; exit 101");
        let failed = build(cargo, root.path(), "cf", "cf", &[]).unwrap_err();
        assert!(
            failed.starts_with("building cf did not succeed: exit code 101\n"),
            "{failed}"
        );
        assert!(failed.contains("error: no such package"), "{failed}");
    }

    #[test]
    fn a_build_that_leaves_no_binary_where_it_should_says_where() {
        let root = tempfile::tempdir().unwrap();
        let (_folder, cargo) = cargo("exit 0");
        let failed = build(cargo, root.path(), "cf", "cf", &[]).unwrap_err();
        assert!(
            failed.starts_with("cargo built cf, but there is none at "),
            "{failed}"
        );
        assert!(failed.contains("target"), "{failed}");
    }

    #[test]
    fn a_cargo_that_cannot_be_started_is_a_failed_build() {
        let root = tempfile::tempdir().unwrap();
        let failed = build(
            Run::new(root.path().join("no-cargo")),
            root.path(),
            "cf",
            "cf",
            &[],
        )
        .unwrap_err();
        assert!(failed.starts_with("could not start `"), "{failed}");
    }

    /// Run by the test below in a process of its own, given `BUILT`: says the `cf` and
    /// the pane host under test.
    #[test]
    #[ignore = "a program the tests of cf start"]
    fn helper_says_the_programs_under_test() {
        println!("cf: {}", binary().unwrap().display());
        println!("pane host: {}", pane_host().unwrap().display());
    }

    #[test]
    fn the_programs_built_before_the_run_are_those_under_test_and_none_is_built() {
        let folder = tempfile::tempdir().unwrap();
        let named = |bin: &str| {
            folder
                .path()
                .join(format!("{bin}{}", std::env::consts::EXE_SUFFIX))
        };
        files::write(&named("cf"), "").unwrap();
        files::write(&named("consensflow-bridge"), "").unwrap();
        // A build would fail at once: no compiler is there.
        let ran = Run::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "cf::tests::helper_says_the_programs_under_test",
                "--ignored",
                "--nocapture",
            ])
            .var(BUILT, folder.path())
            .var("RUSTC", folder.path().join("no-rustc"))
            .run()
            .unwrap();
        assert_eq!(ran.code, Some(0), "{}", ran.output());
        assert!(
            ran.stdout
                .contains(&format!("cf: {}\n", named("cf").display())),
            "{}",
            ran.output()
        );
        assert!(
            ran.stdout.contains(&format!(
                "pane host: {}\n",
                named("consensflow-bridge").display()
            )),
            "{}",
            ran.output()
        );
    }

    #[test]
    fn a_program_the_run_says_it_built_and_did_not_is_an_error_and_is_not_built() {
        let folder = tempfile::tempdir().unwrap();
        let failed = taken(folder.path(), "cf").unwrap_err();
        assert_eq!(
            failed,
            format!(
                "CF_E2E_BUILT says cf was built in {}, and there is none there",
                folder.path().display()
            )
        );
        static CELL: Built = OnceLock::new();
        let failed = built(&CELL, "cf", "cf", &[], Some(folder.path().to_path_buf()))
            .unwrap_err()
            .to_string();
        assert!(failed.contains("and there is none there"), "{failed}");
    }

    #[test]
    fn the_test_binary_the_live_tests_ask_is_built_with_the_feature_that_makes_it() {
        let root = tempfile::tempdir().unwrap();
        let (_folder, cargo) = cargo(
            "echo \"$@\" > asked; \
             mkdir -p app/src-tauri/target/release && : > app/src-tauri/target/release/harness-ask",
        );
        let built = build(
            cargo,
            root.path(),
            "cf-harness",
            "harness-ask",
            &["--features", "test-support"],
        )
        .unwrap();
        assert_eq!(
            built,
            root.path()
                .join("app/src-tauri/target/release")
                .join(format!("harness-ask{}", std::env::consts::EXE_SUFFIX))
        );
        assert_eq!(
            files::read_string(&root.path().join("asked")).unwrap(),
            "build --release --locked -p cf-harness --bin harness-ask --features test-support\n"
        );
    }
}

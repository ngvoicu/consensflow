//! The programs under test: the `cf` and the pane host (`consensflow-bridge`),
//! the release builds of this checkout, each made once for a run of the tests.
//! They are built, not found: `bin/cf` is whatever the last `cargo xtask
//! build-cf` left, and a suite that ran that could pass against sources it
//! never saw (a change in hand not built yet, a plant). `cargo build` knows
//! when the build is up to date, and then it takes a moment.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::process::Run;
use crate::{checkout, Error, Result};

/// The folder the workspace builds a release binary in, from the checkout's
/// root: `.cargo/config.toml` puts the build folder at `app/src-tauri/target`.
const RELEASE: [&str; 4] = ["app", "src-tauri", "target", "release"];

/// What a build leaves, or why it could not be made.
type Built = OnceLock<std::result::Result<PathBuf, String>>;

/// The `cf` under test: built, or found up to date, when this is first asked
/// for, and the same one for every case of the run.
pub fn binary() -> Result<&'static Path> {
    static BUILT: Built = OnceLock::new();
    built(&BUILT, "cf", "cf")
}

/// The pane host under test, `consensflow-bridge`: the program that gives each
/// window its terminal, which the app starts beside the daemon. Built as `cf`
/// is, once for a run.
pub fn pane_host() -> Result<&'static Path> {
    static BUILT: Built = OnceLock::new();
    built(&BUILT, "cf-panes", "consensflow-bridge")
}

/// The build of the binary `bin` of the package `package`, made the first time
/// it is asked for and kept in `cell`.
fn built(cell: &'static Built, package: &str, bin: &str) -> Result<&'static Path> {
    cell.get_or_init(|| build(Run::new(env!("CARGO")), &checkout::root(), package, bin))
        .as_deref()
        .map_err(|message| Error::Build(message.clone()))
}

/// Builds the binary `bin` of the package `package` in the checkout at `root`
/// with `cargo` (the program that builds, given its first words if it has any),
/// and answers where it is.
fn build(
    cargo: Run,
    root: &Path,
    package: &str,
    bin: &str,
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
        let built = build(cargo, root.path(), "cf", "cf").unwrap();
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
        let built = build(cargo, root.path(), "cf-panes", "consensflow-bridge").unwrap();
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
        let failed = build(cargo, root.path(), "cf", "cf").unwrap_err();
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
        let failed = build(cargo, root.path(), "cf", "cf").unwrap_err();
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
        )
        .unwrap_err();
        assert!(failed.starts_with("could not start `"), "{failed}");
    }
}

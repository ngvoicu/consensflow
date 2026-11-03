//! The cargo a test starts (`nested_cargo`): without the variables cargo gave the test, as a shell
//! starts it, and the real cargo case that shows why (`Doc-tests cf` on zeewin, T-365).

// The cases are tests: a failure in one of their helpers is the test's.
#![allow(clippy::disallowed_methods, clippy::unwrap_used)]

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::nested_cargo::{cargo, without_the_tests_own, CARGO};

#[test]
fn what_cargo_sets_for_a_test_is_taken_from_the_cargo_it_starts_and_nothing_else_is() {
    let mut command = Command::new(CARGO);
    let names = [
        // Cargo's, for the test of a crate.
        "CARGO_MANIFEST_DIR",
        "CARGO_MANIFEST_PATH",
        "CARGO_PKG_NAME",
        "CARGO_PKG_VERSION_MAJOR",
        "CARGO_CRATE_NAME",
        "CARGO_BIN_NAME",
        "CARGO_BIN_EXE_cf",
        "CARGO_PRIMARY_PACKAGE",
        "CARGO_TARGET_TMPDIR",
        // The user's, or the system's, which the build the test is part of has as well.
        "CARGO",
        "CARGO_HOME",
        "CARGO_TARGET_DIR",
        "CARGO_INCREMENTAL",
        "RUSTC_BOOTSTRAP",
        "RUSTUP_TOOLCHAIN",
        "PATH",
    ];
    without_the_tests_own(&mut command, names.map(OsString::from));
    let taken: BTreeSet<_> = command
        .get_envs()
        .filter(|(_, value)| value.is_none())
        .map(|(name, _)| name.to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        taken.iter().map(String::as_str).collect::<Vec<_>>(),
        [
            "CARGO_BIN_EXE_cf",
            "CARGO_BIN_NAME",
            "CARGO_CRATE_NAME",
            "CARGO_MANIFEST_DIR",
            "CARGO_MANIFEST_PATH",
            "CARGO_PKG_NAME",
            "CARGO_PKG_VERSION_MAJOR",
            "CARGO_PRIMARY_PACKAGE",
            "CARGO_TARGET_TMPDIR",
        ]
    );
}

/// A workspace of two packages, made in `dir`, as small as the case is: `leaf`, whose build
/// script watches `CARGO_MANIFEST_DIR` as `ring`'s does, and `top`, over it, which has a
/// feature so that a build with it is another build of `top` and the same of `leaf`.
fn write_the_pair(dir: &Path) {
    let file = |path: &str, text: &str| {
        let path = dir.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    };
    file(
        "Cargo.toml",
        "[workspace]\nmembers = [\"leaf\", \"top\"]\nresolver = \"2\"\n",
    );
    file(
        "leaf/Cargo.toml",
        "[package]\nname = \"leaf\"\nversion = \"0.1.0\"\nedition = \"2021\"\nbuild = \"build.rs\"\n",
    );
    file(
        "leaf/build.rs",
        "fn main() { println!(\"cargo:rerun-if-env-changed=CARGO_MANIFEST_DIR\"); }\n",
    );
    file("leaf/src/lib.rs", "pub fn f() -> u32 { 1 }\n");
    file(
        "top/Cargo.toml",
        "[package]\nname = \"top\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n\
         [dependencies]\nleaf = { path = \"../leaf\" }\n\n[features]\nextra = []\n",
    );
    file("top/src/lib.rs", "pub use leaf::f;\n");
}

/// `cargo build --offline` with the words `more`, in the folder of the pair, built into
/// its own `target`, with `RUSTC_BOOTSTRAP` set or not as asked.
fn build(dir: &Path, mut cargo: Command, bootstrap: bool, more: &[&str]) {
    cargo
        .current_dir(dir)
        .args(["build", "--offline"])
        .args(more)
        .env("CARGO_TARGET_DIR", dir.join("target"));
    if bootstrap {
        cargo.env("RUSTC_BOOTSTRAP", "1");
    } else {
        cargo.env_remove("RUSTC_BOOTSTRAP");
    }
    let ran = cargo.output().unwrap();
    assert!(
        ran.status.success(),
        "cargo {more:?} failed:\n{}",
        String::from_utf8_lossy(&ran.stderr)
    );
}

/// The library of `top` the folder has, which is one while one build of it was made.
fn the_one_top(dir: &Path) -> PathBuf {
    let found: Vec<_> = fs::read_dir(dir.join("target/debug/deps"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("libtop-") && name.ends_with(".rlib"))
        })
        .collect();
    assert_eq!(found.len(), 1, "{found:?}");
    found.into_iter().next().unwrap()
}

/// Whether rustc loads `top` as `rlib`, with `extern crate`, which says why when it
/// cannot (a `use` of it says only "can't find crate").
fn loads(dir: &Path, rlib: &Path) -> Result<(), String> {
    fs::write(dir.join("probe.rs"), "extern crate top;\n").unwrap();
    let rustc = Path::new(CARGO).with_file_name(format!("rustc{}", std::env::consts::EXE_SUFFIX));
    let ran = Command::new(rustc)
        .current_dir(dir)
        .args([
            "--edition=2021",
            "--crate-type=lib",
            "--emit=metadata",
            "--crate-name=probe",
            "probe.rs",
        ])
        .arg("-o")
        .arg(dir.join("probe.rmeta"))
        .arg("--extern")
        .arg(format!("top={}", rlib.display()))
        .arg("-L")
        .arg(format!(
            "dependency={}",
            dir.join("target/debug/deps").display()
        ))
        .output()
        .unwrap();
    if ran.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&ran.stderr).into_owned())
    }
}

/// The case: `leaf` is made as a shell makes it, without `RUSTC_BOOTSTRAP`. A run with it
/// set builds `top` with a feature on, over the `leaf` it finds. Then the test it runs
/// starts the cargo `nested` makes, with `RUSTC_BOOTSTRAP` set as the run has it: `top` as it
/// is without the feature, over the same `leaf`. Whether the first `top` can be loaded after.
fn the_top_of_the_run_after(nested: impl FnOnce() -> Command) -> Result<(), String> {
    let folder = tempfile::tempdir().unwrap();
    let dir = folder.path();
    write_the_pair(dir);
    build(dir, a_shells_cargo(), false, &["--package", "leaf"]);
    build(
        dir,
        a_shells_cargo(),
        true,
        &["--package", "top", "--features", "extra"],
    );
    let the_runs = the_one_top(dir);
    build(dir, nested(), true, &["--package", "top"]);
    loads(dir, &the_runs)
}

/// The cargo of the run, started from a shell: without the variable that the build script
/// of `leaf` watches, whatever this test has.
fn a_shells_cargo() -> Command {
    let mut command = Command::new(CARGO);
    command.env_remove("CARGO_MANIFEST_DIR");
    command
}

/// The cargo a test starts if nothing is taken from what cargo gave it.
fn cargo_with_the_tests_own() -> Command {
    let mut command = Command::new(CARGO);
    command.env("CARGO_MANIFEST_DIR", env!("CARGO_MANIFEST_DIR"));
    command
}

#[test]
fn a_cargo_with_the_variables_of_the_test_builds_over_the_files_of_the_run_and_they_cannot_be_loaded(
) {
    let loaded = the_top_of_the_run_after(cargo_with_the_tests_own);
    let said = loaded.expect_err(
        "this case no longer breaks the files of the run: the test below holds nothing, \
         and what the cargo a test starts is not told may no longer matter",
    );
    assert!(said.contains("E0460"), "{said}");
}

#[test]
fn the_cargo_a_test_starts_builds_nothing_over_the_files_of_the_run() {
    // This test has what cargo gave it, as the test that starts `cf` does.
    assert!(std::env::var_os("CARGO_MANIFEST_DIR").is_some());
    assert_eq!(the_top_of_the_run_after(cargo), Ok(()));
}

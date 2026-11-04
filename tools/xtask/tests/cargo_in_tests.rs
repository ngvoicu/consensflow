//! The cargo a test starts is started without the variables cargo gave the test (issue
//! `zeewin-doc-tests-cf-cannot-find-cf-daemon`, T-365, 2026-10-23). `cargo test` gives each test the
//! variables of its crate (`CARGO_MANIFEST_DIR`, `CARGO_PKG_NAME`...), a cargo the test starts takes
//! them as its own, and a build script that watches one (`ring`'s does) makes it build the package
//! and all that is over it again, over files of the build that runs the test: with
//! `RUSTC_BOOTSTRAP=1` the libraries of the running build can then no longer be loaded, and rustdoc
//! says only "can't find crate" of `cf_daemon`. T-365 mended the one place that was found,
//! `cf-daemon`'s `runs` test; this holds the rule over the whole workspace, from the sources:
//!
//! - the real cargo is named in one file, `crates/cf-process/src/nested_cargo.rs`, which says what
//!   cargo gives a test and takes it from the environment of the cargo it starts. A test that wants a
//!   cargo takes it from there: `nested_cargo::cargo()`, or `Run::cargo()` in the rig. Any other file
//!   that spells `env!("CARGO")`, or starts `cargo` by its name in a test, started one with all of
//!   them;
//! - every crate that uses that file includes it by its path (`#[path = ".../nested_cargo.rs"]`):
//!   `tools/cf-publish` and `tools/cf-release` take no dependency, and a crate that took the feature
//!   `test-support` of `cf-process` for it would be built twice (xtask's tests would replace the
//!   `xtask.exe` that `cargo xtask check` is running, which Windows does not let a build do). A copy of
//!   the list of the variables in another file would be a second list to forget;
//! - the cargo that xtask itself starts (`Invocation::new("cargo", …)` in `src/`) is the tool's, not a
//!   test's, and `cargo xtask` runs under `cargo run`, which gives its own `CARGO_*` to every cargo
//!   xtask starts. It is not held here: scrubbing it changes the environment of every build the gate
//!   starts, so that the first build of every tree after the change is a whole one. Asked of the chief
//!   (T-969).
#![allow(clippy::disallowed_methods, clippy::expect_used, clippy::unwrap_used)]

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use common::checkout;

/// The one file that names the real cargo, from the checkout's root.
const THE_PLACE: &str = "crates/cf-process/src/nested_cargo.rs";

/// The cases of that file: they name the cargo by its constant, and start it as a test would.
const ITS_CASES: &str = "crates/cf-process/src/nested_cargo_tests.rs";

/// Every Rust file under `folder`, but for build folders.
fn sources(folder: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(folder) else {
        return;
    };
    for entry in entries {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if path.is_dir() {
            if !["target", "node_modules", ".git"].contains(&name.as_str()) {
                sources(&path, found);
            }
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            found.push(path);
        }
    }
}

/// The Rust sources of the workspace: its crates, its tools and the app's.
fn workspace() -> Vec<PathBuf> {
    let root = checkout();
    let mut found = Vec::new();
    for folder in [
        "crates",
        "tools",
        "app/src-tauri/src",
        "app/src-tauri/tests",
    ] {
        sources(&root.join(folder), &mut found);
    }
    found
}

/// The path from the checkout's root, with `/` between folders.
fn relative(path: &Path) -> String {
    path.strip_prefix(checkout())
        .unwrap()
        .components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

/// A file's code without its comments and its whitespace: rustfmt may break a call anywhere, and
/// what a comment says of cargo is no start of one.
fn squeezed(path: &Path) -> String {
    fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(|line| line.split_once("//").map_or(line, |(code, _)| code))
        .flat_map(str::chars)
        .filter(|each| !each.is_whitespace())
        .collect()
}

/// Whether the file is a test's: in a `tests` folder, or a `tests.rs` or `testing.rs`.
fn is_test(path: &str) -> bool {
    path.split('/').any(|part| part == "tests")
        || path.ends_with("/tests.rs")
        || path.ends_with("/testing.rs")
}

/// The package the file is in: `crates/cf-e2e`, `tools/xtask`.
fn package(path: &str) -> String {
    path.split('/').take(2).collect::<Vec<_>>().join("/")
}

#[test]
fn the_cargo_a_test_starts_is_named_in_one_file_and_has_none_of_the_variables_cargo_gave_the_test()
{
    let mut found = Vec::new();
    for path in workspace() {
        let shown = relative(&path);
        if shown == THE_PLACE
            || shown == ITS_CASES
            || shown == "tools/xtask/tests/cargo_in_tests.rs"
        {
            continue;
        }
        let text = squeezed(&path);
        if text.contains("env!(\"CARGO\")") {
            found.push(format!("{shown}: names the real cargo, env!(\"CARGO\")"));
        }
        if is_test(&shown) {
            for builder in ["Command::new", "Run::new"] {
                if text.contains(&format!("{builder}(\"cargo\"")) {
                    found.push(format!(
                        "{shown}: starts cargo by its name, {builder}(\"cargo\")"
                    ));
                }
            }
        }
        if text.contains("fngiven_to_a_test(") {
            found.push(format!(
                "{shown}: a second list of the variables cargo gives a test"
            ));
        }
    }
    assert!(
        found.is_empty(),
        "a test that wants cargo takes it from {THE_PLACE} (nested_cargo::cargo(), Run::cargo()):\n{}",
        found.join("\n")
    );
}

#[test]
fn a_crate_that_uses_the_one_file_includes_that_file_and_not_a_copy_of_it() {
    let sources = workspace();
    let mut users: Vec<String> = Vec::new();
    let mut includers: Vec<String> = Vec::new();
    let mut found = Vec::new();
    for path in &sources {
        let shown = relative(path);
        if shown == THE_PLACE
            || shown == ITS_CASES
            || shown == "tools/xtask/tests/cargo_in_tests.rs"
        {
            continue;
        }
        // The crate that holds the file declares it as its own module, for its cases.
        if shown == "crates/cf-process/src/lib.rs" {
            continue;
        }
        let text = squeezed(path);
        if text.contains("nested_cargo::") {
            users.push(package(&shown));
        }
        // `mod nested_cargo` is the include, with the attributes right before it: its path.
        for at in text.match_indices("modnested_cargo").map(|(at, _)| at) {
            let before = &text[..at];
            let start = before.rfind([';', '}']).map_or(0, |end| end + 1);
            let attributes = &before[start..];
            if attributes.contains("#[path=\"")
                && attributes.contains("cf-process/src/nested_cargo.rs\"]")
            {
                includers.push(package(&shown));
            } else {
                found.push(format!(
                    "{shown}: a module nested_cargo that is not {THE_PLACE} included by its path"
                ));
            }
        }
    }
    users.sort();
    users.dedup();
    includers.sort();
    includers.dedup();
    for user in users {
        if !includers.contains(&user) {
            found.push(format!(
                "{user} uses nested_cargo and includes no {THE_PLACE} (#[path = \"…/nested_cargo.rs\"] mod nested_cargo;)"
            ));
        }
    }
    assert!(found.is_empty(), "\n{}", found.join("\n"));
}

#[test]
fn the_one_file_takes_what_cargo_gives_a_test_from_the_cargo_it_starts() {
    let place = checkout().join(THE_PLACE);
    let text = fs::read_to_string(&place).expect("the one file is there");
    for name in [
        "CARGO_PKG_",
        "CARGO_MANIFEST_DIR",
        "CARGO_BIN_EXE_",
        "CARGO_CRATE_NAME",
        "env_remove",
    ] {
        assert!(
            text.contains(name),
            "{THE_PLACE} does not take {name} from the cargo"
        );
    }
    // The cases hold it to cargo's list, with a real cargo.
    assert!(checkout().join(ITS_CASES).is_file());
}

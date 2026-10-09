//! xtask as a process, the way `cargo xtask` starts it: from the folders a
//! developer is in, handing its arguments to a stand-in for `node` (the fake
//! child, copied under that name into a folder that is all the PATH it has),
//! and answering that stand-in's exit status.
#![allow(clippy::disallowed_methods, clippy::expect_used, clippy::unwrap_used)]

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use common::{arg_line, arg_lines, assert_ran_in, FAKE};
use xtask::process::Invocation;

/// The line `.cargo/config.toml` has to carry for `cargo xtask` to be this.
const ALIAS: &str =
    r#"xtask = ["run", "--locked", "--quiet", "--package", "xtask", "--bin", "xtask", "--"]"#;
/// What `cargo help xtask` says that line makes of the word.
const EXPANDED: &str =
    "`xtask` is aliased to `run --locked --quiet --package xtask --bin xtask --`";

fn checkout() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .to_path_buf()
}

/// A folder holding the fake child under each of `names`, as the programs xtask starts.
fn programs(names: &[&str]) -> tempfile::TempDir {
    let folder = tempfile::tempdir().unwrap();
    for name in names {
        let file = format!("{name}{}", std::env::consts::EXE_SUFFIX);
        fs::copy(FAKE, folder.path().join(file)).unwrap();
    }
    folder
}

/// xtask run in `cwd` with `args`, finding programs in `path` alone, and the
/// fake child's instructions in `vars`.
fn xtask(cwd: &Path, path: &Path, vars: &[(&str, &str)], args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_xtask"))
        .current_dir(cwd)
        .env("PATH", path)
        .envs(vars.iter().copied())
        .args(args)
        .output()
        .unwrap()
}

fn out(ran: &Output) -> String {
    String::from_utf8(ran.stdout.clone()).unwrap()
}

fn err(ran: &Output) -> String {
    String::from_utf8(ran.stderr.clone()).unwrap()
}

#[test]
fn the_checkout_is_the_same_from_the_root_from_app_and_from_anywhere_else() {
    let nowhere = tempfile::tempdir().unwrap();
    let said = format!("checkout: {}\n", checkout().display());
    for cwd in [
        checkout(),
        checkout().join("app"),
        checkout().join("app").join("src-tauri"),
        checkout().join("tools").join("xtask"),
        nowhere.path().to_path_buf(),
    ] {
        let ran = xtask(&cwd, nowhere.path(), &[], &["--help"]);
        assert!(
            ran.status.success(),
            "from {}: {}",
            cwd.display(),
            err(&ran)
        );
        assert!(
            out(&ran).contains(&said),
            "from {}:\n{}",
            cwd.display(),
            out(&ran)
        );
    }
}

#[test]
fn a_command_runs_its_script_from_the_root_with_every_word_as_it_was_given() {
    let node = programs(&["node"]);
    let words = ["departures", "a b", "--", "c", "", "--help"];
    // From `app/`, as a developer would: it is the checkout's root that counts.
    let ran = xtask(&checkout().join("app"), node.path(), &[], &words);
    assert!(ran.status.success(), "{}", err(&ran));
    let report = out(&ran);
    assert_ran_in(&report, &checkout());
    let script = checkout().join("tests").join("departures.mjs");
    assert_eq!(
        arg_lines(&report),
        [
            script.into_os_string(),
            "a b".into(),
            "--".into(),
            "c".into(),
            "".into(),
            "--help".into()
        ]
        .map(arg_line)
    );
}

#[test]
fn the_exit_status_of_what_it_ran_is_its_own() {
    let node = programs(&["node"]);
    for status in [0, 1, 2, 3, 42, 101, 255] {
        let said = status.to_string();
        let ran = xtask(
            &checkout(),
            node.path(),
            &[("FAKE_CHILD_EXIT", &said)],
            &["test", "daemons"],
        );
        assert_eq!(ran.status.code(), Some(status), "{}", err(&ran));
        // What the child wrote is its own to write: nothing of xtask's is added on a failure.
        assert_eq!(err(&ran), "");
    }
}

/// A `node` that the system will not start: a file of that name which is no program.
/// (One that is not there would do, but Windows also looks for it on the PATH of whoever
/// runs the test, which may have a real one.)
#[test]
fn a_script_that_cannot_be_started_is_one_error_with_status_1() {
    let folder = tempfile::tempdir().unwrap();
    let name = format!("node{}", std::env::consts::EXE_SUFFIX);
    fs::write(folder.path().join(name), "not a program").unwrap();
    let ran = xtask(&checkout(), folder.path(), &[], &["app", "test"]);
    assert_eq!(ran.status.code(), Some(1));
    assert_eq!(out(&ran), "");
    let said = err(&ran);
    assert!(said.starts_with("xtask: could not run `node`: "), "{said}");
    assert!(said.ends_with('\n') && said.lines().count() == 1, "{said}");
}

#[test]
fn a_help_and_a_refusal_start_nothing() {
    // No program on the PATH: whatever is started fails, and these do not.
    let no_programs = tempfile::tempdir().unwrap();
    for (command, script) in [
        ("portable", "app/scripts/portable.mjs"),
        ("smoke", "tests/smoke.mjs"),
        ("smoke-updater", "tests/smoke-updater.mjs"),
        ("candidate", "app/scripts/candidate.mjs"),
    ] {
        let ran = xtask(&checkout(), no_programs.path(), &[], &[command, "--help"]);
        assert!(ran.status.success(), "{command}: {}", err(&ran));
        let help = out(&ran);
        assert!(
            help.starts_with(&format!("Usage: cargo xtask {command}")),
            "{help}"
        );
        assert!(help.contains(&format!("node {script}")), "{help}");
        assert_eq!(err(&ran), "");
    }
    for (args, said) in [
        (vec![], "xtask: a command is required\n"),
        (vec!["deploy"], "xtask: unknown command: deploy\n"),
        (vec!["app"], "xtask: app takes a command: test, clippy\n"),
    ] {
        let ran = xtask(&checkout(), no_programs.path(), &[], &args);
        assert_eq!(ran.status.code(), Some(2), "{args:?}");
        assert_eq!(out(&ran), "");
        assert_eq!(
            err(&ran),
            format!("{said}see `cargo xtask --help`\n"),
            "{args:?}"
        );
    }
}

/// The commands that run in Rust, asked for what they do not take, are refused
/// with the status 2 before anything is started, from `app/` as from the root.
/// (What they do when they run is for the unit tests, which give them a system of
/// their own: here it would be this checkout's `bin/` and resources.)
#[test]
fn the_commands_that_run_in_rust_refuse_the_words_they_do_not_take_and_start_nothing() {
    let no_programs = tempfile::tempdir().unwrap();
    for (args, said) in [
        (
            vec!["build-cf", "--ofline"],
            "xtask: build-cf takes --offline or nothing, not --ofline\n",
        ),
        (
            vec!["stage", "--offline"],
            "xtask: stage takes no arguments\n",
        ),
        (
            vec!["conpty"],
            "xtask: conpty takes --into DIR, the folder for the console host's files\n",
        ),
    ] {
        let ran = xtask(&checkout().join("app"), no_programs.path(), &[], &args);
        assert_eq!(ran.status.code(), Some(2), "{args:?}");
        assert_eq!(out(&ran), "", "{args:?}");
        assert_eq!(
            err(&ran),
            format!("{said}see `cargo xtask --help`\n"),
            "{args:?}"
        );
    }
}

#[test]
fn the_commands_that_run_in_rust_say_so_in_their_help() {
    let no_programs = tempfile::tempdir().unwrap();
    for command in ["build-cf", "stage", "conpty"] {
        let ran = xtask(&checkout(), no_programs.path(), &[], &[command, "--help"]);
        assert!(ran.status.success(), "{command}: {}", err(&ran));
        let help = out(&ran);
        assert!(
            help.starts_with(&format!("Usage: cargo xtask {command}")),
            "{help}"
        );
        assert!(help.contains("It runs in Rust."), "{help}");
        assert!(!help.contains("node "), "{help}");
        assert_eq!(err(&ran), "");
    }
}

#[test]
fn check_says_each_step_and_ends_when_all_have_passed() {
    let tools = programs(&["cargo", "npm"]);
    let ran = xtask(&checkout(), tools.path(), &[], &["check"]);
    assert!(ran.status.success(), "{}", err(&ran));
    // xtask's own tests are built in a folder of their own, beside the workspace's.
    let own_tests = Invocation::new("cargo", ".")
        .args(["test", "--package", "xtask", "--target-dir"])
        .arg(
            checkout()
                .join("app")
                .join("src-tauri")
                .join("target")
                .join("xtask-check"),
        )
        .display();
    assert_eq!(
        err(&ran),
        format!(
            "xtask check [1/6]: cargo fmt --all --check\n\
             xtask check [2/6]: cargo clippy --workspace --exclude app --all-targets -- -D warnings\n\
             xtask check [3/6]: cargo test --workspace --exclude app --exclude xtask\n\
             xtask check [4/6]: {own_tests}\n\
             xtask check [5/6]: npm run lint\n\
             xtask check [6/6]: npm test\n\
             xtask check: all 6 steps passed\n"
        )
    );
    // Each step ran from the checkout's root.
    let report = out(&ran);
    assert_eq!(report.matches("cwd: ").count(), 6);
    assert_ran_in(&report, &checkout());
}

#[test]
fn check_stops_at_the_first_step_that_fails_and_answers_its_status() {
    let tools = programs(&["cargo", "npm"]);
    let ran = xtask(
        &checkout(),
        tools.path(),
        &[("FAKE_CHILD_EXIT", "3")],
        &["check"],
    );
    assert_eq!(ran.status.code(), Some(3));
    assert_eq!(
        err(&ran),
        "xtask check [1/6]: cargo fmt --all --check\n\
         xtask check: step 1 of 6 ended with status 3: cargo fmt --all --check\n"
    );
    assert_eq!(out(&ran).matches("cwd: ").count(), 1);
}

/// Cargo finds `xtask` as the alias in `.cargo/config.toml` from the root and
/// from `app/`, and expands it to the command that runs this package's binary:
/// `cargo help` says so without building anything, and without a build that
/// could replace the binary the other tests here are starting.
#[test]
fn cargo_finds_the_xtask_alias_from_the_root_and_from_app() {
    let missing =
        format!(".cargo/config.toml needs, beside its [build] table:\n\n[alias]\n{ALIAS}\n");
    for cwd in [checkout(), checkout().join("app")] {
        let ran = Command::new(env!("CARGO"))
            .args(["help", "xtask"])
            .current_dir(&cwd)
            .output()
            .unwrap();
        assert_eq!(
            out(&ran).trim(),
            EXPANDED,
            "`cargo help xtask` from {} said:\n{}{}\n{missing}",
            cwd.display(),
            out(&ran),
            err(&ran)
        );
    }
}

/// Every `"name": "cargo xtask <words>"` line of a package.json, as `(name, words)`.
fn xtask_scripts(package_json: &Path) -> Vec<(String, String)> {
    let text = fs::read_to_string(package_json).unwrap();
    text.lines()
        .filter_map(|line| {
            let (name, command) = line.trim().strip_prefix('"')?.split_once("\": \"")?;
            let words = command.strip_prefix("cargo xtask ")?;
            Some((
                name.to_string(),
                words.trim_end_matches([',', '"']).to_string(),
            ))
        })
        .collect()
}

/// A typo in a script of the package.json files is found here, not by whoever runs it
/// first: each command they hand over to is one xtask has (asked for its help, which
/// starts nothing).
#[test]
fn every_npm_script_that_hands_over_to_xtask_names_a_command_it_has() {
    let nowhere = tempfile::tempdir().unwrap();
    let mut scripts = xtask_scripts(&checkout().join("package.json"));
    scripts.extend(xtask_scripts(&checkout().join("app").join("package.json")));
    // The ones that moved at step 5's first landing are there.
    assert!(scripts.len() >= 14, "{scripts:?}");
    for (name, words) in scripts {
        let mut args: Vec<&str> = words.split(' ').collect();
        args.push("--help");
        let ran = xtask(&checkout(), nowhere.path(), &[], &args);
        assert!(
            ran.status.success(),
            "npm run {name} is `cargo xtask {words}`, which is no command:\n{}",
            err(&ran)
        );
    }
}

/// The alias was added to a file that already says where the build goes, which the
/// workflows, the integration rig and the portable build read.
#[test]
fn the_cargo_config_keeps_the_target_dir_the_workflows_read() {
    let config = fs::read_to_string(checkout().join(".cargo").join("config.toml")).unwrap();
    assert!(
        config.contains(r#"target-dir = "app/src-tauri/target""#),
        "{config}"
    );
}

//! xtask's process module against a child it can be told what to do: the fake
//! child, which says where it was started and with what, and ends as the
//! environment tells it.
#![allow(clippy::disallowed_methods, clippy::expect_used, clippy::unwrap_used)]

mod common;

use std::ffi::OsString;
use std::fs;
use std::path::Path;

use cf_base::env::Env;
use common::{arg_line, arg_lines, assert_ran_in, FAKE};
use xtask::process::{capture, run, Failure, Invocation};

/// An environment of just these variables, and the one Windows wants of every
/// program it starts: nothing of this test's own is in it.
fn env_of(vars: &[(&str, &str)]) -> Env {
    let mut all: Vec<(OsString, OsString)> = vars
        .iter()
        .map(|(name, value)| (OsString::from(name), OsString::from(value)))
        .collect();
    if let Some(root) = Env::from_process().os("SYSTEMROOT") {
        all.push(("SYSTEMROOT".into(), root.to_os_string()));
    }
    Env::from_vars(all)
}

fn fake_in(folder: &Path) -> Invocation {
    Invocation::new(FAKE, folder)
}

#[test]
fn the_code_a_program_ends_with_is_the_code_run_answers() {
    let here = std::env::current_dir().unwrap();
    for code in [0, 1, 2, 3, 7, 42, 101, 255] {
        let said = code.to_string();
        let env = env_of(&[("FAKE_CHILD_QUIET", "1"), ("FAKE_CHILD_EXIT", &said)]);
        assert_eq!(run(&fake_in(&here), &env).unwrap(), code);
    }
}

#[test]
fn the_words_a_program_is_given_reach_it_one_each_as_they_are() {
    let words = [
        "plain",
        "a b",
        "",
        "--",
        "-x",
        "it's",
        "say \"x\"",
        "trailing ",
        " leading",
        "tab\there",
        "ünï",
        "C:\\a b\\c",
        "50%",
        "a=b",
        "--flag=with space",
    ];
    let here = std::env::current_dir().unwrap();
    let seen = capture(&fake_in(&here).args(words), &env_of(&[])).unwrap();
    assert_eq!(seen.code, 0);
    let expected: Vec<_> = words.iter().map(|word| arg_line(*word)).collect();
    assert_eq!(arg_lines(&seen.stdout), expected);
}

#[test]
fn a_program_runs_in_the_folder_it_is_given() {
    let folder = tempfile::tempdir().unwrap();
    let seen = capture(&fake_in(folder.path()), &env_of(&[])).unwrap();
    assert_ran_in(&seen.stdout, folder.path());
}

#[test]
fn a_program_has_the_environment_it_is_handed_with_what_the_invocation_changes_and_nothing_else() {
    let env = env_of(&[
        (
            "FAKE_CHILD_REPORT",
            "KEPT,SET,TAKEN,GONE,NEVER_THERE,CARGO_MANIFEST_DIR",
        ),
        ("KEPT", "1"),
        ("SET", "from the environment"),
        ("TAKEN", "x"),
    ]);
    let here = std::env::current_dir().unwrap();
    let invocation = fake_in(&here)
        .var("SET", "from the invocation")
        .without("TAKEN")
        .var("LATE", "y");
    let seen = capture(&invocation, &env).unwrap();
    let vars: Vec<_> = seen
        .stdout
        .lines()
        .filter(|line| line.starts_with("var "))
        .collect();
    assert_eq!(
        vars,
        [
            "var KEPT: \"1\"",
            "var SET: \"from the invocation\"",
            "var TAKEN: unset",
            "var GONE: unset",
            "var NEVER_THERE: unset",
            // Cargo gave this test the variable; the environment it handed over has none.
            "var CARGO_MANIFEST_DIR: unset",
        ]
    );
}

#[test]
fn what_a_program_wrote_is_kept_with_the_code_it_left() {
    let env = env_of(&[
        ("FAKE_CHILD_EXIT", "3"),
        ("FAKE_CHILD_STDERR", "it went wrong"),
    ]);
    let here = std::env::current_dir().unwrap();
    let seen = capture(&fake_in(&here).arg("one"), &env).unwrap();
    assert_eq!(seen.code, 3);
    assert_eq!(seen.stderr, "it went wrong\n");
    assert_eq!(arg_lines(&seen.stdout), [arg_line("one")]);
}

#[test]
fn a_bare_name_is_found_on_the_path_of_the_environment_it_is_started_with() {
    let programs = tempfile::tempdir().unwrap();
    let named = format!("a-tool{}", std::env::consts::EXE_SUFFIX);
    fs::copy(FAKE, programs.path().join(named)).unwrap();
    let env = env_of(&[
        ("PATH", &programs.path().to_string_lossy()),
        ("FAKE_CHILD_EXIT", "9"),
        ("FAKE_CHILD_QUIET", "1"),
    ]);
    let here = std::env::current_dir().unwrap();
    assert_eq!(run(&Invocation::new("a-tool", &here), &env).unwrap(), 9);
}

#[test]
fn a_program_that_is_not_there_is_told_by_its_name_whichever_way_it_is_run() {
    let empty = tempfile::tempdir().unwrap();
    let env = env_of(&[("PATH", &empty.path().to_string_lossy())]);
    let missing =
        Invocation::new("cf-xtask-no-such-program", empty.path()).arg("--password=hunter2");
    let words = "`cf-xtask-no-such-program` was not found: is it installed, and on the PATH?";
    assert!(matches!(run(&missing, &env), Err(Failure::NotFound { .. })));
    assert_eq!(run(&missing, &env).unwrap_err().to_string(), words);
    assert_eq!(capture(&missing, &env).unwrap_err().to_string(), words);
}

#[test]
fn a_folder_that_is_not_there_is_not_taken_for_a_program_that_is_not() {
    let folder = tempfile::tempdir().unwrap();
    let gone = folder.path().join("gone");
    let said = run(&fake_in(&gone), &env_of(&[])).unwrap_err();
    assert!(matches!(said, Failure::NoFolder { .. }), "{said}");
    assert_eq!(
        said.to_string(),
        format!(
            "the folder `{}` to run `{FAKE}` in is not there",
            gone.display()
        )
    );
}

#[cfg(unix)]
#[test]
fn a_program_the_system_refuses_to_start_is_told_in_the_systems_words() {
    let folder = tempfile::tempdir().unwrap();
    let not_runnable = folder.path().join("a-file");
    fs::write(&not_runnable, "#!/bin/sh\n").unwrap();
    let said = run(&Invocation::new(&not_runnable, folder.path()), &env_of(&[])).unwrap_err();
    assert!(matches!(said, Failure::Refused { .. }), "{said}");
    assert_eq!(
        said.to_string(),
        format!(
            "could not run `{}`: Permission denied (os error 13)",
            not_runnable.display()
        )
    );
}

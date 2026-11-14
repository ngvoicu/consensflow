//! A say-only live run (`CF_LIVE_AUDIT`) through `cargo xtask live`, started as a developer starts
//! it, against a stand-in for `cargo` (the fake child): whatever the run came to, it is never a pass,
//! its file is the run's own and new, and an older run's file is never told as this run's (T-1592).
#![allow(clippy::disallowed_methods, clippy::expect_used, clippy::unwrap_used)]

mod common;

use std::fs;

use common::{checkout, err, out, programs, xtask};

/// The words after `live` of a regular live run of a test the checkout has.
const RUN: [&str; 2] = ["live", "paste"];

#[test]
fn a_say_only_run_that_made_no_file_is_no_pass_and_is_told_to_have_stopped_before_it_acted() {
    let tools = programs(&["cargo"]);
    let folder = tempfile::tempdir().unwrap();
    let file = folder.path().join("said.txt");
    let ran = xtask(
        &checkout(),
        tools.path(),
        &[
            ("CF_LIVE_AUDIT", "1"),
            ("CF_LIVE_AUDIT_FILE", file.to_str().unwrap()),
        ],
        &RUN,
    );
    // The stand-in cargo came to 0 and made no file: the run is no pass, and says why.
    assert_eq!(ran.status.code(), Some(1), "{}", err(&ran));
    let said = err(&ran);
    assert!(said.contains("made no audit file"), "{said}");
    assert!(said.contains("no pass"), "{said}");
}

#[test]
fn an_audit_file_that_is_there_stops_the_run_before_cargo_starts_and_is_not_told_as_the_runs() {
    let tools = programs(&["cargo"]);
    let folder = tempfile::tempdir().unwrap();
    let file = folder.path().join("older.txt");
    fs::write(&file, "an older run's audit\nkill\tprocess 1\tground\n").unwrap();
    let ran = xtask(
        &checkout(),
        tools.path(),
        &[
            ("CF_LIVE_AUDIT", "1"),
            ("CF_LIVE_AUDIT_FILE", file.to_str().unwrap()),
        ],
        &RUN,
    );
    assert_ne!(ran.status.code(), Some(0), "{}", err(&ran));
    let said = err(&ran);
    assert!(said.contains("is there already"), "{said}");
    assert!(!said.contains("would have made"), "{said}");
    // Cargo was never started (the stand-in says where it was started), and the file is as it was.
    assert!(!out(&ran).contains("cwd:"), "{}", out(&ran));
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "an older run's audit\nkill\tprocess 1\tground\n"
    );
}

#[test]
fn a_set_under_audit_is_no_pass_with_a_file_for_each_run_and_stops_where_one_is_there() {
    let tools = programs(&["cargo"]);
    let folder = tempfile::tempdir().unwrap();
    let file = folder.path().join("said.txt");
    let vars = [
        ("CF_LIVE_AUDIT", "1"),
        ("CF_LIVE_AUDIT_FILE", file.to_str().unwrap()),
        ("FAKE_CHILD_REPORT", "CF_LIVE_AUDIT_FILE"),
    ];
    let ran = xtask(
        &checkout(),
        tools.path(),
        &vars,
        &["live", "--set", "windows"],
    );
    // Every run of the set came to 0, and the set is no pass all the same.
    assert_eq!(ran.status.code(), Some(1), "{}", err(&ran));
    assert!(err(&ran).contains("is no pass"), "{}", err(&ran));
    // The fake cargo also reports the builds before the cases. Count the file given to each
    // case, not the variable inherited by those builds.
    let output = out(&ran);
    let cases: Vec<&str> = output.split("== live ").skip(1).collect();
    assert_eq!(cases.len(), 3, "{output}");
    for (at, case) in cases.iter().enumerate() {
        let given: Vec<&str> = case
            .lines()
            .filter(|line| line.starts_with("var CF_LIVE_AUDIT_FILE:"))
            .collect();
        assert_eq!(given.len(), 1, "{case}");
        assert!(
            given[0].contains(&format!("said-{}.txt", at + 1)),
            "run {} was given {}",
            at + 1,
            given[0]
        );
    }

    // A file of an earlier run of the set stops it before any run starts.
    fs::write(folder.path().join("said-2.txt"), "an older run's\n").unwrap();
    let stopped = xtask(
        &checkout(),
        tools.path(),
        &vars,
        &["live", "--set", "windows"],
    );
    assert_ne!(stopped.status.code(), Some(0));
    assert!(
        err(&stopped).contains("is there already"),
        "{}",
        err(&stopped)
    );
    assert!(!out(&stopped).contains("== live"), "{}", out(&stopped));
}

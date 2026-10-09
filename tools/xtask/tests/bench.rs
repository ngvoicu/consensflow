//! `cargo xtask bench records-memory` as a process, started the way `cargo xtask`
//! starts it, against a stand-in for `cargo`: the fake child, which says where it
//! ran, with what and with which variables, and ends as it is told. What each
//! measure is run as, from where and with which variables, how what it printed
//! is said, and the status it ends with, are held here; how the command line is
//! read and each line of output is chosen are the unit tests' (`src/bench`).
#![allow(clippy::disallowed_methods, clippy::expect_used, clippy::unwrap_used)]

mod common;

use std::fs;
use std::path::PathBuf;

use common::{arg_line, checkout, err, out, programs, shown, with_cargo, xtask};

/// What a bench run said, as `(heading, lines indented under it)`.
fn blocks(report: &str) -> Vec<(&str, Vec<&str>)> {
    let mut blocks: Vec<(&str, Vec<&str>)> = Vec::new();
    for line in report.lines() {
        match line.strip_prefix("  ") {
            Some(inside) => blocks.last_mut().unwrap().1.push(inside),
            None => blocks.push((line, Vec::new())),
        }
    }
    blocks
}

/// The line the fake child writes for each word of one measure's cargo run.
fn measure_args(name: &str) -> Vec<String> {
    [
        "test",
        "--offline",
        "--release",
        "-p",
        "cf-harness",
        "--lib",
        &format!("claude::record::tests::memory::{name}"),
        "--",
        "--ignored",
        "--nocapture",
        "--exact",
    ]
    .map(arg_line)
    .to_vec()
}

fn arg_lines_of<'a>(block: &[&'a str]) -> Vec<&'a str> {
    block
        .iter()
        .copied()
        .filter(|line| line.starts_with("arg: "))
        .collect()
}

/// The folder the child of one block ran in, as the system names it.
fn ran_in(block: &[&str]) -> PathBuf {
    let said = block
        .iter()
        .find_map(|line| line.strip_prefix("cwd: "))
        .unwrap();
    fs::canonicalize(said).unwrap()
}

#[test]
fn it_runs_each_measure_alone_from_the_root_and_says_what_it_printed_under_its_heading() {
    let ran = with_cargo(
        "CF_RECORDS_MEMORY_LINES,CF_RECORDS_MEMORY_TRANSCRIPT",
        &["bench", "records-memory"],
    );
    assert!(ran.status.success(), "{}", err(&ran));
    let report = out(&ran);
    let blocks = blocks(&report);
    let headings: Vec<_> = blocks.iter().map(|(heading, _)| *heading).collect();
    assert_eq!(
        headings,
        [
            "first look:",
            "first look parts:",
            "unchanged look:",
            "reread:"
        ]
    );
    let root = fs::canonicalize(checkout()).unwrap();
    for ((_, lines), name) in
        blocks
            .iter()
            .zip(["first_look", "first_look_parts", "unchanged_look", "reread"])
    {
        assert_eq!(arg_lines_of(lines), measure_args(name), "{name}");
        assert_eq!(ran_in(lines), root, "{name}");
        assert!(
            lines.contains(&"var CF_RECORDS_MEMORY_LINES: unset"),
            "{name}"
        );
        assert!(
            lines.contains(&"var CF_RECORDS_MEMORY_TRANSCRIPT: unset"),
            "{name}"
        );
    }
    assert_eq!(err(&ran), "");
}

#[test]
fn its_options_repeat_the_first_looks_and_reach_the_measures_by_the_folder_and_the_variables() {
    let ran = with_cargo(
        "CF_RECORDS_MEMORY_LINES,CF_RECORDS_MEMORY_TRANSCRIPT",
        &[
            "bench",
            "records-memory",
            "--runs=2",
            "--lines",
            "5000",
            "--transcript",
            "/t/a b.jsonl",
            // From the checkout's root, as the script this replaces ran it.
            "--repo",
            "app",
        ],
    );
    assert!(ran.status.success(), "{}", err(&ran));
    let report = out(&ran);
    let blocks = blocks(&report);
    let headings: Vec<_> = blocks.iter().map(|(heading, _)| *heading).collect();
    assert_eq!(
        headings,
        [
            "first look (run 1):",
            "first look (run 2):",
            "first look parts (run 1):",
            "first look parts (run 2):",
            "unchanged look:",
            "reread:"
        ]
    );
    let names = [
        "first_look",
        "first_look",
        "first_look_parts",
        "first_look_parts",
        "unchanged_look",
        "reread",
    ];
    let here = fs::canonicalize(checkout().join("app")).unwrap();
    let lines_var = format!("var CF_RECORDS_MEMORY_LINES: {}", shown("5000"));
    let transcript_var = format!(
        "var CF_RECORDS_MEMORY_TRANSCRIPT: {}",
        shown("/t/a b.jsonl")
    );
    for ((_, lines), name) in blocks.iter().zip(names) {
        assert_eq!(arg_lines_of(lines), measure_args(name), "{name}");
        assert_eq!(ran_in(lines), here, "{name}");
        assert!(lines.contains(&lines_var.as_str()), "{name}: {lines:?}");
        assert!(
            lines.contains(&transcript_var.as_str()),
            "{name}: {lines:?}"
        );
    }
}

#[test]
fn a_measure_that_fails_ends_it_with_status_1_and_says_all_it_wrote_on_the_standard_error() {
    let tools = programs(&["cargo"]);
    let ran = xtask(
        &checkout(),
        tools.path(),
        &[
            ("FAKE_CHILD_EXIT", "101"),
            ("FAKE_CHILD_STDERR", "the test panicked"),
        ],
        &["bench", "records-memory"],
    );
    assert_eq!(ran.status.code(), Some(1));
    // Its heading was said when it began, and nothing after it ran.
    assert_eq!(out(&ran), "first look:\n");
    let said = err(&ran);
    assert!(said.contains("arg: \"--exact\"\n"), "{said}");
    assert!(
        said.ends_with(
            "the test panicked\nxtask: first_look failed: its cargo test ended with status 101\n"
        ),
        "{said}"
    );
    assert_eq!(said.matches("cwd: ").count(), 1, "{said}");
}

#[test]
fn a_cargo_that_is_not_there_ends_it_after_the_first_heading() {
    let no_programs = tempfile::tempdir().unwrap();
    let ran = xtask(
        &checkout(),
        no_programs.path(),
        &[],
        &["bench", "records-memory"],
    );
    assert_eq!(ran.status.code(), Some(1));
    assert_eq!(out(&ran), "first look:\n");
    assert_eq!(
        err(&ran),
        "xtask: `cargo` was not found: is it installed, and on the PATH?\n"
    );
}

#[test]
fn what_it_does_not_take_is_refused_with_status_2_before_anything_is_started() {
    let no_programs = tempfile::tempdir().unwrap();
    let takes =
        "xtask: bench records-memory takes [--runs N] [--lines N] [--transcript FILE] [--repo DIR]";
    for (args, said) in [
        (["--bogus"], format!("{takes}, not --bogus\n")),
        (["positional"], format!("{takes}, not positional\n")),
        (
            ["--lines"],
            "xtask: bench records-memory: --lines needs a value\n".to_string(),
        ),
        (
            ["--runs=many"],
            "xtask: bench records-memory: --runs takes a whole number, not many\n".to_string(),
        ),
    ] {
        let mut line = vec!["bench", "records-memory"];
        line.extend(args);
        let ran = xtask(&checkout().join("app"), no_programs.path(), &[], &line);
        assert_eq!(ran.status.code(), Some(2), "{args:?}");
        assert_eq!(out(&ran), "", "{args:?}");
        assert_eq!(
            err(&ran),
            format!("{said}see `cargo xtask --help`\n"),
            "{args:?}"
        );
    }
}

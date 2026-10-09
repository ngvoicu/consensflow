//! What the integration tests share: xtask as a process, the fake child
//! (`src/bin/fake_child.rs`) and how to read what it says.
// Each test file uses some of this, and not all of it.
#![allow(dead_code)]

use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// The fake child, built for these tests.
pub const FAKE: &str = env!("CARGO_BIN_EXE_fake-child");

/// The checkout these tests run in.
pub fn checkout() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .to_path_buf()
}

/// xtask run in `cwd` with `args`, finding programs in `path` alone, and the
/// fake child's instructions in `vars`.
pub fn xtask(cwd: &Path, path: &Path, vars: &[(&str, &str)], args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_xtask"))
        .current_dir(cwd)
        .env("PATH", path)
        .envs(vars.iter().copied())
        .args(args)
        .output()
        .unwrap()
}

/// What a run wrote on its standard output.
pub fn out(ran: &Output) -> String {
    String::from_utf8(ran.stdout.clone()).unwrap()
}

/// What a run wrote on its standard error.
pub fn err(ran: &Output) -> String {
    String::from_utf8(ran.stderr.clone()).unwrap()
}

/// `args` run from `app/`, as a developer would, with the fake child as the only
/// program there is, `cargo`, reporting the variables `report` names.
pub fn with_cargo(report: &str, args: &[&str]) -> Output {
    let tools = programs(&["cargo"]);
    xtask(
        &checkout().join("app"),
        tools.path(),
        &[("FAKE_CHILD_REPORT", report)],
        args,
    )
}

/// How the fake child writes the value `text` when it reports a variable.
pub fn shown(text: &str) -> String {
    format!("{:?}", OsStr::new(text))
}

/// A folder holding the fake child under each of `names`, as the programs xtask starts.
pub fn programs(names: &[&str]) -> tempfile::TempDir {
    let folder = tempfile::tempdir().unwrap();
    for name in names {
        let file = format!("{name}{}", std::env::consts::EXE_SUFFIX);
        fs::copy(FAKE, folder.path().join(file)).unwrap();
    }
    folder
}

/// The line a fake child writes for the word `word` it was given.
pub fn arg_line(word: impl Into<OsString>) -> String {
    format!("arg: {:?}", word.into())
}

/// The lines of a fake child's report that name the words it was given, as
/// written: what follows `arg: `, one for each, in order.
pub fn arg_lines(report: &str) -> Vec<&str> {
    report
        .lines()
        .filter(|line| line.starts_with("arg: "))
        .collect()
}

/// The folders the report says its children ran in, one for each `cwd:` line in
/// order, as the system names them.
pub fn cwds(report: &str) -> Vec<PathBuf> {
    report
        .lines()
        .filter_map(|line| line.strip_prefix("cwd: "))
        .map(|said| fs::canonicalize(said).unwrap())
        .collect()
}

/// What the report says of the variable `name`: `unset`, or its value as Rust
/// writes a string. None when the child was not asked to report it.
pub fn var_line<'a>(report: &'a str, name: &str) -> Option<&'a str> {
    let start = format!("var {name}: ");
    report
        .lines()
        .find_map(|line| line.strip_prefix(start.as_str()))
}

/// Asserts that the report says the child ran in `expected`: the same folder,
/// whatever links or prefixes the system names it by.
pub fn assert_ran_in(report: &str, expected: &Path) {
    let said = report
        .lines()
        .find_map(|line| line.strip_prefix("cwd: "))
        .unwrap_or_else(|| panic!("no folder in the report:\n{report}"));
    assert_eq!(
        fs::canonicalize(said).unwrap(),
        fs::canonicalize(expected).unwrap(),
        "ran in {said}, not in {}",
        expected.display()
    );
}

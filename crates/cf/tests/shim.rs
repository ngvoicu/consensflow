//! `cf-shim`, the program that stands for `cf` in the user's terminal on Windows, run as
//! the process it is: the binary the bundle carries, copied to `cf.exe` beside the launch file
//! `cf setup` writes (`cf.launch`), which it reads to find the `cf` it runs.
//!
//! This is the shim on the system the tests run on, which is not Windows here: the stand-in `cf`
//! is a script. What Windows does with the command line is the shells' (`windows_shells.rs`), run
//! on Windows against the real `cf`; what the shim does with what it was given is these.
#![cfg(unix)]
// The tests' own folders and files, and the programs they start.
#![allow(clippy::unwrap_used, clippy::disallowed_methods)]

use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const SHIM: &str = env!("CARGO_BIN_EXE_cf-shim");

/// The shim copied to `<folder>/cf.exe`, its launch file naming `cf`, and the home it pins.
fn shimmed(folder: &Path, cf: &Path, home: Option<&Path>) -> PathBuf {
    let exe = folder.join("cf.exe");
    fs::copy(SHIM, &exe).unwrap();
    let pin = home.map_or_else(String::new, |home| {
        format!(
            "setlocal\r\nset \"CONSENSFLOW_HOME={}\"\r\n",
            home.display()
        )
    });
    fs::write(
        folder.join("cf.launch"),
        format!(
            "@echo off\r\nREM Installed by ConsensFlow. Runs the app's own cf, so the terminal and the\r\nREM window never drift apart.\r\n{pin}\"{}\" %*\r\n",
            cf.display()
        ),
    )
    .unwrap();
    exe
}

/// A stand-in `cf` that writes each argument it was given, each ended by a NUL, to `args`, the home
/// it was told to `home`, copies its standard input to its standard output, and ends with 7.
fn stand_in(folder: &Path) -> (PathBuf, PathBuf, PathBuf) {
    let (args, home) = (folder.join("args"), folder.join("home"));
    let cf = folder.join("cf-stand-in");
    fs::write(
        &cf,
        format!(
            "#!/bin/sh\nprintf '%s\\0' \"$@\" > '{}'\nprintf '%s' \"$CONSENSFLOW_HOME\" > '{}'\ncat\nexit 7\n",
            args.display(),
            home.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&cf, fs::Permissions::from_mode(0o755)).unwrap();
    (cf, args, home)
}

fn run(exe: &Path, args: &[OsString], input: &str) -> Output {
    let mut child = Command::new(exe)
        .args(args)
        .env_remove("CONSENSFLOW_HOME")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn given(args: &Path) -> Vec<Vec<u8>> {
    let bytes = fs::read(args).unwrap();
    let mut each: Vec<Vec<u8>> = bytes.split(|byte| *byte == 0).map(<[u8]>::to_vec).collect();
    each.pop();
    each
}

#[test]
fn a_text_of_many_lines_reaches_the_cf_whole_with_every_other_argument_as_it_was_given() {
    let folder = tempfile::tempdir().unwrap();
    let (cf, args, _) = stand_in(folder.path());
    let exe = shimmed(folder.path(), &cf, None);
    let text = "Întrebarea 1: verific și versiunea în engleză?\nHe said \"only the Romanian one\" & left; 100% sure | %PATH% ^ !x! `echo` $(whoami)\n\nPLOP-6142\r\n";
    let words = ["ask", text, "two words", "", "-n", r"C:\dir\", "ends\\"];

    let ran = run(
        &exe,
        &words.iter().map(OsString::from).collect::<Vec<_>>(),
        "",
    );

    assert_eq!(
        ran.status.code(),
        Some(7),
        "the exit code of the cf is the command's"
    );
    let sent: Vec<Vec<u8>> = words.iter().map(|each| each.as_bytes().to_vec()).collect();
    assert_eq!(given(&args), sent);
}

#[test]
fn what_is_piped_in_reaches_the_cf_and_what_it_says_reaches_the_terminal() {
    let folder = tempfile::tempdir().unwrap();
    let (cf, _, _) = stand_in(folder.path());
    let exe = shimmed(folder.path(), &cf, None);

    let ran = run(
        &exe,
        &[OsString::from("ask"), OsString::from("-")],
        "one\ntwo\n",
    );

    assert_eq!(String::from_utf8_lossy(&ran.stdout), "one\ntwo\n");
}

#[test]
fn the_home_the_launch_file_pins_is_the_home_the_cf_is_told() {
    let folder = tempfile::tempdir().unwrap();
    let (cf, _, told) = stand_in(folder.path());
    let home = folder.path().join("a home with spaces");
    let exe = shimmed(folder.path(), &cf, Some(&home));

    run(&exe, &[OsString::from("x")], "");

    assert_eq!(fs::read(told).unwrap(), home.as_os_str().as_bytes());
}

#[test]
fn without_its_launch_file_the_shim_says_so_and_what_to_do_and_ends_with_1() {
    let folder = tempfile::tempdir().unwrap();
    let exe = folder.path().join("cf.exe");
    fs::copy(SHIM, &exe).unwrap();

    let ran = run(&exe, &[OsString::from("ask")], "");

    assert_eq!(ran.status.code(), Some(1));
    let said = String::from_utf8_lossy(&ran.stderr);
    assert!(
        said.starts_with("cf: ")
            && said.contains(&folder.path().join("cf.launch").display().to_string())
            && said.contains("cf setup"),
        "{said}"
    );
}

#[test]
fn a_launch_file_that_names_the_shim_itself_does_not_start_it_again() {
    let folder = tempfile::tempdir().unwrap();
    let exe = folder.path().join("cf.exe");
    fs::copy(SHIM, &exe).unwrap();
    shimmed(folder.path(), &exe, None);

    let ran = run(&exe, &[OsString::from("ask")], "");

    assert_eq!(ran.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&ran.stderr).contains("names this program itself"),
        "{}",
        String::from_utf8_lossy(&ran.stderr)
    );
}

#[test]
fn a_cf_that_is_gone_is_said_with_where_it_was_and_what_to_do() {
    let folder = tempfile::tempdir().unwrap();
    let gone = folder.path().join("gone").join("cf.exe");
    let exe = shimmed(folder.path(), &gone, None);

    let ran = run(&exe, &[OsString::from("ask")], "");

    assert_eq!(ran.status.code(), Some(1));
    let said = String::from_utf8_lossy(&ran.stderr);
    assert!(
        said.contains(&gone.display().to_string()) && said.contains("cf setup"),
        "{said}"
    );
}

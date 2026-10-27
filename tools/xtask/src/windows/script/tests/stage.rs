//! What a run makes sure of on the machine before its command (`PREPARE_STEPS`), tried by the
//! machine's own PowerShell and cmd on a profile made for each case: the crates, and the console
//! host beside the builds the tests run from. A stand-in `cargo`, a batch file in the tree
//! (cmd looks in the folder it is in first), says what it was asked, and fails the console host
//! where the tree holds a file that asks it to.

use std::fs;

use super::lock::{Came, Case};

/// The stand-in `cargo`: it appends its words to `stage.log`, and ends with 7 for `xtask conpty`
/// where the tree has a file named `no-console-host`.
const CARGO: &str = "@echo off\r\n\
    echo cargo %*>>stage.log\r\n\
    if \"%1 %2\"==\"xtask conpty\" if exist no-console-host (\r\n\
      echo no network for the console host's package 1>&2\r\n\
      exit /b 7\r\n\
    )\r\n\
    exit /b 0\r\n";

#[test]
fn a_run_makes_sure_of_the_crates_and_the_console_host_before_its_command_even_when_nobody_built_there(
) {
    let case = Case::new();
    let ran = Came::of(case.run(
        "0000c001-00000001",
        &[("cargo.cmd", CARGO.as_bytes())],
        None,
        "echo ran>>stage.log",
    ));
    assert_eq!(ran.status, Some(0), "{}", ran.said);
    let log = fs::read_to_string(case.copy().join("stage.log")).unwrap();
    assert_eq!(
        log.lines().collect::<Vec<_>>(),
        [
            "cargo fetch --locked --offline",
            "cargo xtask conpty --into app/src-tauri/target/debug",
            "cargo xtask conpty --into app/src-tauri/target/release",
            "ran",
        ],
        "{}",
        ran.said
    );
    // Each is said in the run's output, before the command's.
    let at = |needle: &str| {
        ran.said
            .find(needle)
            .unwrap_or_else(|| panic!("no {needle:?}: {}", ran.said))
    };
    assert!(
        at("== cargo xtask conpty --into app/src-tauri/target/debug")
            < at("== cargo xtask conpty --into app/src-tauri/target/release"),
        "{}",
        ran.said
    );
}

#[test]
fn a_run_that_cannot_have_the_console_host_ends_with_that_status_and_never_runs_its_command() {
    let case = Case::new();
    let ran = Came::of(case.run(
        "0000c002-00000002",
        &[
            ("cargo.cmd", CARGO.as_bytes()),
            ("no-console-host", b"".as_slice()),
        ],
        None,
        "echo ran>>stage.log",
    ));
    assert_eq!(ran.status, Some(7), "{}", ran.said);
    assert!(
        ran.said.contains(
            "== the run needs what 'cargo xtask conpty --into app/src-tauri/target/debug' makes there, and it could not be made (status 7): the command is not run on what happens to be on the machine"
        ),
        "{}",
        ran.said
    );
    assert!(
        ran.said
            .contains("no network for the console host's package"),
        "{}",
        ran.said
    );
    // It stopped at the first it could not make, and ran nothing after it.
    let log = fs::read_to_string(case.copy().join("stage.log")).unwrap();
    assert_eq!(
        log.lines().collect::<Vec<_>>(),
        [
            "cargo fetch --locked --offline",
            "cargo xtask conpty --into app/src-tauri/target/debug",
        ],
        "{}",
        ran.said
    );
}

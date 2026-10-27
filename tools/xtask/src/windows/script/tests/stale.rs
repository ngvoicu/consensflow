//! A run killed hard leaves its records (the owner file and the older process file) behind, and
//! nothing in the next run's way: the lock is the system's and closes with the process, the records
//! name a process that is gone, and a machine asked its state says no run goes on there (T-701
//! item 9; the Linux side of kluris's runner, which kept the copy of the runner from before T-610,
//! was refused for as long as the file stayed).

use std::fs;
use std::io::{BufRead, BufReader};
use std::time::{Duration, Instant};

use super::lock::{Came, Case};
use crate::process::{self, Invocation};
use crate::windows::machine::{state_script, State};
use crate::windows::script::{exit_said, LOCK, OWNER, RUNNING};

/// What the machine says of its copy `SHARED` ("race-copy" in the case's profile): the free disk
/// and whether a run holds it, as `Machine::state` reads it.
fn state_of(case: &Case) -> State {
    let asked = Invocation::new("powershell", case.scratch.path())
        .args(["-NoProfile", "-NonInteractive", "-Command"])
        .arg(state_script("race-copy"));
    let vars: Vec<(std::ffi::OsString, std::ffi::OsString)> = case
        .env
        .iter()
        .filter(|(name, _)| !name.eq_ignore_ascii_case("USERPROFILE"))
        .map(|(name, value)| (name.to_owned(), value.to_owned()))
        .chain([("USERPROFILE".into(), case.profile.clone().into())])
        .collect();
    let answer = process::capture(&asked, &cf_base::env::Env::from_vars(vars)).unwrap();
    assert_eq!(answer.code, 0, "{}", answer.stderr);
    State::read(&answer.stdout).unwrap_or_else(|| panic!("{:?}", answer.stdout))
}

#[test]
fn a_run_killed_hard_leaves_records_that_neither_make_the_machine_busy_nor_refuse_the_next_run() {
    let case = Case::new();
    let mut run = case.run(
        "0000d001-00000001",
        &[("keep.txt", b"kept".as_slice())],
        None,
        "ping -n 120 127.0.0.1",
    );
    let owner = case.copy().join(OWNER);
    // The run goes on: its owner file names its process, and the machine says a run holds the copy.
    let until = Instant::now() + Duration::from_secs(90);
    while !fs::read_to_string(&owner).is_ok_and(|text| text.lines().count() >= 3) {
        assert!(Instant::now() < until, "the run never began its command");
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(state_of(&case).running, "a run that goes on is a run");
    // Killed as a battery or a BSOD ends it: no `finally`, so no record is taken away.
    run.kill().unwrap();
    run.wait().unwrap();
    assert!(owner.exists(), "the owner file stays");
    assert!(case.copy().join(RUNNING).exists(), "the older file stays");
    // The machine says no run goes on there, though the records are there.
    let state = state_of(&case);
    assert!(
        !state.running,
        "a record of a process that is gone is no run: {state:?}"
    );
    assert!(
        !state.held,
        "the system closed the lock with the process: {state:?}"
    );
    // And the next run goes, takes the records for its own and takes them away.
    let next = Came::of(case.run(
        "0000d002-00000002",
        &[("keep.txt", b"kept".as_slice())],
        None,
        "cmd /c exit 0",
    ));
    assert_eq!(next.status, Some(0), "{}", next.said);
    assert!(!owner.exists() && !case.copy().join(RUNNING).exists());
}

/// A PowerShell that has ended while this test still holds its process open, as a program that
/// holds a handle to a process holds it after it is gone. The system keeps such a process in its
/// table, and the case is that the guards that ask `Get-Process` do not take it for a run: they
/// are asked, here, of one (measured on zeewin2, 2026-10-26: `Get-Process -Id` does not list it).
/// (Rust's `Child` keeps the handle until it is dropped, `wait` or not.)
fn ended_but_held(case: &Case) -> std::process::Child {
    let start = Invocation::new("powershell", case.scratch.path()).args([
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        "exit 0",
    ]);
    let mut child = process::spawn(&start, &case.env, std::process::Stdio::null(), false).unwrap();
    child.wait().unwrap();
    // The record is written after it began, as a run's is.
    std::thread::sleep(Duration::from_millis(500));
    child
}

#[test]
fn a_powershell_that_has_ended_but_is_still_held_open_by_another_program_is_no_run() {
    // T-602 saw a run refused for minutes naming a process ("A run is still going on this machine
    // (process 54196)"). One theory was a corpse the system keeps while a program holds a handle to
    // it; `Get-Process` does not list one, so the guard is right and this holds.
    let case = Case::new();
    let held = ended_but_held(&case);
    let record = case.copy().join(RUNNING);
    fs::write(&record, held.id().to_string()).unwrap();
    let owner = case.copy().join(OWNER);
    fs::write(&owner, format!("some-run\r\n{}\r\n", held.id())).unwrap();
    // The machine's state, and then a run, say it is no run.
    assert!(
        !state_of(&case).running,
        "an ended process the system still lists is no run"
    );
    let ran = Came::of(case.run(
        "0000d003-00000003",
        &[("keep.txt", b"kept".as_slice())],
        None,
        "cmd /c exit 0",
    ));
    assert_eq!(ran.status, Some(0), "{}", ran.said);
    drop(held);
}

/// What the machine holds of a run, read at once: its owner file, the older process file, and
/// whether its lock is held (it cannot be opened to share with no one).
fn held_of(case: &Case) -> Vec<&'static str> {
    use std::os::windows::fs::OpenOptionsExt;

    let mut held = Vec::new();
    if case.copy().join(OWNER).exists() {
        held.push("the owner file");
    }
    if case.copy().join(RUNNING).exists() {
        held.push("the process file");
    }
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .share_mode(0)
        .open(case.copy().join(LOCK));
    if lock.is_err() {
        held.push("the lock");
    }
    held
}

/// A run says how it ended as the last thing it does that the next run could meet: by the time
/// that line is read, the machine holds nothing of it (T-701 item 4). Before, the line was written
/// first and the lock, the records and the job's release followed in the `finally`, so that
/// whatever slowed the PowerShell's end (the tester's run, T-602, found it alive minutes after its
/// exit line, and the next run was refused for as long) kept the machine refused.
#[test]
fn a_run_holds_nothing_of_the_machine_by_the_time_its_exit_line_is_read() {
    let case = Case::new();
    let leave = b"@echo off\r\nstart \"\" /b ping -n 8 127.0.0.1 >nul\r\nexit /b 3\r\n";
    let mut holding = Vec::new();
    for round in 0..10 {
        let id = format!("0000d1{round:02x}-000000{round:02x}");
        let mut run = case.run(&id, &[("leave.cmd", leave.as_slice())], None, "leave.cmd");
        let out = run.stdout.take().unwrap();
        let mut said = String::new();
        for line in BufReader::new(out).lines() {
            let line = line.unwrap();
            said.push_str(&line);
            said.push('\n');
            if exit_said(&line).is_some() {
                // Read now, before anything else is asked of the machine.
                let held = held_of(&case);
                if !held.is_empty() {
                    holding.push(format!("round {round}: {}", held.join(", ")));
                }
                break;
            }
        }
        run.wait().unwrap();
        assert!(said.contains("== exit 3"), "round {round}: {said}");
    }
    assert!(
        holding.is_empty(),
        "the machine still held something of a run whose exit line was read: {holding:#?}"
    );
}

// A test starts a program of its own on Windows, and a failure in its own
// folders and files is the test's.
#![allow(clippy::disallowed_methods, clippy::unwrap_used)]

use std::fs;

use super::*;

fn names_in(folder: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(folder)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn the_shim_of_a_bundle_is_the_file_beside_its_cf() {
    assert_eq!(
        bundled(Path::new("/app/cli/bin/cf.exe")),
        Path::new("/app/cli/bin/cf-shim.exe")
    );
    assert_eq!(bundled(Path::new("cf")), Path::new("cf-shim.exe"));
}

#[test]
fn a_bundle_with_a_shim_gives_its_bytes_and_one_without_says_where_it_looked() {
    let folder = tempfile::tempdir().unwrap();
    let cf = folder.path().join("cf.exe");
    let said = read_bundled(&cf).unwrap_err();
    assert!(
        said.contains("is missing beside") && said.contains(&cf.display().to_string()),
        "{said}"
    );
    fs::write(folder.path().join(BUNDLED), b"shim").unwrap();
    assert_eq!(read_bundled(&cf).unwrap(), b"shim");
}

#[test]
fn a_program_that_is_not_there_is_written_and_nothing_else_is() {
    let folder = tempfile::tempdir().unwrap();
    let program = folder.path().join("cf.exe");
    place(&program, b"shim").unwrap();
    assert_eq!(fs::read(&program).unwrap(), b"shim");
    assert_eq!(names_in(folder.path()), ["cf.exe"]);
}

#[cfg(unix)]
#[test]
fn a_program_a_terminal_can_run_is_written_with_the_mode_to_run() {
    use std::os::unix::fs::PermissionsExt;
    let folder = tempfile::tempdir().unwrap();
    let program = folder.path().join("cf.exe");
    place(&program, b"shim").unwrap();
    let mode = fs::metadata(&program).unwrap().permissions().mode();
    assert!(mode & 0o111 != 0, "{mode:o}");
}

#[cfg(unix)]
#[test]
fn a_program_that_is_the_shim_is_left_as_it_is() {
    use std::os::unix::fs::MetadataExt;
    let folder = tempfile::tempdir().unwrap();
    let program = folder.path().join("cf.exe");
    place(&program, b"shim").unwrap();
    let before = fs::metadata(&program).unwrap().ino();
    place(&program, b"shim").unwrap();
    assert_eq!(fs::metadata(&program).unwrap().ino(), before);
    assert_eq!(names_in(folder.path()), ["cf.exe"]);
}

#[cfg(unix)]
#[test]
fn a_program_that_is_another_is_replaced_by_a_new_file_and_not_written_in_place() {
    use std::os::unix::fs::MetadataExt;
    let folder = tempfile::tempdir().unwrap();
    let program = folder.path().join("cf.exe");
    place(&program, b"old shim").unwrap();
    let before = fs::metadata(&program).unwrap().ino();
    // Held open, as a running program is: the copy that was there stays whole.
    let held = fs::File::open(&program).unwrap();
    place(&program, b"new shim").unwrap();
    assert_eq!(fs::read(&program).unwrap(), b"new shim");
    assert_ne!(fs::metadata(&program).unwrap().ino(), before);
    assert_eq!(held.metadata().unwrap().ino(), before);
    // Nothing runs the copy set aside, so it is taken away.
    assert_eq!(names_in(folder.path()), ["cf.exe"]);
}

#[test]
fn the_copies_set_aside_earlier_are_taken_away_and_nothing_else_is() {
    let folder = tempfile::tempdir().unwrap();
    let program = folder.path().join("cf.exe");
    for name in [
        "cf.exe.old-1",
        "cf.exe.old-7",
        "cf.exe.old",
        "other.exe.old-1",
        "cf.launch",
    ] {
        fs::write(folder.path().join(name), "").unwrap();
    }
    place(&program, b"shim").unwrap();
    assert_eq!(
        names_in(folder.path()),
        ["cf.exe", "cf.exe.old", "cf.launch", "other.exe.old-1"]
    );
}

#[test]
fn a_program_goes_aside_under_the_first_name_that_is_not_taken() {
    let folder = tempfile::tempdir().unwrap();
    let program = folder.path().join("cf.exe");
    assert_eq!(free_aside(&program), folder.path().join("cf.exe.old-1"));
    for name in ["cf.exe.old-1", "cf.exe.old-2", "cf.exe.old-4"] {
        fs::write(folder.path().join(name), "").unwrap();
    }
    assert_eq!(free_aside(&program), folder.path().join("cf.exe.old-3"));
}

#[test]
fn a_program_is_current_only_when_it_holds_the_bytes() {
    let folder = tempfile::tempdir().unwrap();
    let program = folder.path().join("cf.exe");
    assert!(!is_current(&program, b"shim"));
    fs::write(&program, b"shim").unwrap();
    assert!(is_current(&program, b"shim"));
    assert!(!is_current(&program, b"shims"));
}

/// Windows refuses to write or delete a program that runs, and lets it be
/// renamed: the shim a terminal is using is replaced all the same, and goes on.
#[cfg(windows)]
#[test]
fn a_program_that_runs_is_replaced_and_goes_on_running() {
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    let folder = tempfile::tempdir().unwrap();
    let program = folder.path().join("cf.exe");
    // cmd.exe is a program that runs a while, and is one whose file is this one's.
    let system = std::env::var_os("SystemRoot").unwrap();
    fs::copy(Path::new(&system).join(r"System32\cmd.exe"), &program).unwrap();
    let mut running = Command::new(&program)
        .args(["/d", "/c", "ping -n 8 127.0.0.1 >nul"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    assert!(
        fs::OpenOptions::new().write(true).open(&program).is_err(),
        "Windows should refuse to write a program that runs"
    );

    place(&program, b"new shim").unwrap();

    assert_eq!(fs::read(&program).unwrap(), b"new shim");
    assert!(running.try_wait().unwrap().is_none(), "it goes on running");
    running.kill().unwrap();
    running.wait().unwrap();
    // Once it ends, the next call takes the copy set aside away.
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        sweep(&program);
        if names_in(folder.path()) == ["cf.exe"] {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("left aside: {:?}", names_in(folder.path()));
}

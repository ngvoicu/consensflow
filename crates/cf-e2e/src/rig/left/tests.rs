#![cfg(unix)]

use std::fs::{self, File};
use std::time::Duration;

use super::*;
use crate::process::{Run, Spawned};

/// A program that runs a minute, started now.
fn sleeper() -> Spawned {
    Run::new("/bin/sleep")
        .arg("60")
        .closed_input()
        .spawn()
        .unwrap()
}

/// The file `pids`, with `pid` in it, last written `ago` ago.
fn noted(folder: &Path, pid: u32, ago: Duration) -> std::path::PathBuf {
    let file = folder.join("pids.jsonl");
    fs::write(&file, format!("{pid}\n")).unwrap();
    let written = fs::metadata(&file).unwrap().modified().unwrap();
    File::options()
        .write(true)
        .open(&file)
        .unwrap()
        .set_modified(written - ago)
        .unwrap();
    file
}

#[test]
fn a_window_still_there_is_said_with_what_it_is() {
    let folder = tempfile::tempdir().unwrap();
    let window = sleeper();
    let file = noted(folder.path(), window.id(), Duration::ZERO);
    let left = left_behind(&file, Duration::from_millis(200)).expect("a window was left");
    assert!(left.starts_with(&window.id().to_string()), "{left}");
    assert!(left.contains("/bin/sleep 60"), "{left}");
    assert!(left.contains("state "), "{left}");
}

#[test]
fn a_window_that_has_gone_is_none() {
    let folder = tempfile::tempdir().unwrap();
    let mut window = sleeper();
    let pid = window.id();
    window.kill();
    let file = noted(folder.path(), pid, Duration::ZERO);
    assert_eq!(left_behind(&file, Duration::from_millis(200)), None);
}

/// The number of a window that ended is held by a program that began after the last note was
/// written. It is no window's, and is not said to be one that was left behind.
#[test]
fn a_program_that_began_after_the_last_note_is_none_of_the_windows() {
    let folder = tempfile::tempdir().unwrap();
    let another = sleeper();
    let file = noted(folder.path(), another.id(), Duration::from_secs(3600));
    assert_eq!(left_behind(&file, Duration::from_millis(200)), None);
}

#[test]
fn a_number_is_looked_at_for_what_it_is_in_the_words_of_ps() {
    assert_eq!(process::described(u32::MAX), None);
    let window = sleeper();
    let said = process::described(window.id()).unwrap();
    assert!(said.contains("command /bin/sleep 60"), "{said}");
    assert!(process::age(window.id()).unwrap() < Duration::from_secs(60));
    assert_eq!(process::age(u32::MAX), None);
}

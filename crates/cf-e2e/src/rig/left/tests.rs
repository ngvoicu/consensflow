use std::fs::{self, File};
use std::time::Duration;

use super::*;
use crate::process::{Run, Spawned};

/// A program that runs a minute, started now.
#[cfg(unix)]
fn sleeper() -> Spawned {
    Run::new("/bin/sleep")
        .arg("60")
        .closed_input()
        .spawn()
        .unwrap()
}

/// A program that stays until it is ended: Windows' own `sort`, which reads its
/// input to its end, and a spawned one's stays open while the case holds it.
#[cfg(windows)]
fn sleeper() -> Spawned {
    let system = crate::process::own_var("SystemRoot").unwrap_or_else(|| r"C:\Windows".into());
    Run::new(
        std::path::Path::new(&system)
            .join("System32")
            .join("sort.exe"),
    )
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

/// `began.jsonl` beside the `pids` file, saying the process `pid` began `started`,
/// in the words `process::started` would say them.
fn began(folder: &Path, pid: u32, started: &str) {
    fs::write(folder.join("began.jsonl"), format!("{pid}\t{started}\n")).unwrap();
}

/// A process that began when the note says the window's began is the window's
/// own, still there.
#[test]
fn a_process_that_began_when_its_note_says_is_the_windows() {
    let folder = tempfile::tempdir().unwrap();
    let window = sleeper();
    let file = noted(folder.path(), window.id(), Duration::ZERO);
    began(
        folder.path(),
        window.id(),
        &process::started(window.id()).unwrap(),
    );
    let left = left_behind(&file, Duration::from_millis(200)).expect("a window was left");
    assert!(left.starts_with(&window.id().to_string()), "{left}");
}

/// A window that ended gave its number back, and the system gave it to a program
/// that began later. The note says when the window's process began, which is not
/// when this one began: it is no window's.
#[test]
fn a_program_that_took_an_ended_windows_number_is_none_of_the_windows() {
    let folder = tempfile::tempdir().unwrap();
    let squatter = sleeper();
    let file = noted(folder.path(), squatter.id(), Duration::ZERO);
    began(
        folder.path(),
        squatter.id(),
        "a start that is not this process's",
    );
    assert_eq!(left_behind(&file, Duration::from_millis(200)), None);
}

#[cfg(unix)]
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
#[cfg(unix)]
#[test]
fn a_program_that_began_after_the_last_note_is_none_of_the_windows() {
    let folder = tempfile::tempdir().unwrap();
    let another = sleeper();
    let file = noted(folder.path(), another.id(), Duration::from_secs(3600));
    assert_eq!(left_behind(&file, Duration::from_millis(200)), None);
}

#[cfg(unix)]
#[test]
fn a_number_is_looked_at_for_what_it_is_in_the_words_of_ps() {
    assert_eq!(process::described(u32::MAX), None);
    let window = sleeper();
    let said = process::described(window.id()).unwrap();
    assert!(said.contains("command /bin/sleep 60"), "{said}");
    assert!(process::age(window.id()).unwrap() < Duration::from_secs(60));
    assert_eq!(process::age(u32::MAX), None);
}

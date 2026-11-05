use std::fs::{self, File};
use std::time::Duration;

use super::*;
use crate::process::{Run, Spawned};

/// A program that runs a minute, started now, and what its command line holds.
#[cfg(unix)]
fn sleeper() -> Spawned {
    Run::new("/bin/sleep")
        .arg("60")
        .closed_input()
        .spawn()
        .unwrap()
}
#[cfg(unix)]
const SLEEPS: &str = "/bin/sleep 60";

#[cfg(windows)]
fn sleeper() -> Spawned {
    Run::new("ping")
        .args(["-n", "61", "127.0.0.1"])
        .finding_programs()
        .closed_input()
        .spawn()
        .unwrap()
}
#[cfg(windows)]
const SLEEPS: &str = "127.0.0.1";

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
    assert!(left.contains(SLEEPS), "{left}");
    #[cfg(unix)]
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

/// Windows' process table says when a process began in 100 ns since 1601: the start of 1970
/// is 116444736000000000 of them, and a process that began more than the slack after the
/// file was written began after it.
#[test]
fn windows_time_reads_from_1601_and_a_later_start_is_after_the_slack_only() {
    let unix = std::time::SystemTime::UNIX_EPOCH;
    assert_eq!(from_file_time(116_444_736_000_000_000), Some(unix));
    assert_eq!(
        from_file_time(116_444_736_000_000_000 + 15_000_000),
        Some(unix + Duration::from_millis(1500))
    );
    assert_eq!(from_file_time(1), None);
    let written = unix + Duration::from_secs(1_000);
    assert!(began_later(written + Duration::from_secs(3), written));
    assert!(!began_later(written + Duration::from_secs(1), written));
    assert!(!began_later(written - Duration::from_secs(60), written));
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

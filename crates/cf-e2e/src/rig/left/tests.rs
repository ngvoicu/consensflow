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

/// The program [`sleeper`] runs, which these tests take for the stand-in's own.
#[cfg(unix)]
const SLEEPER: &str = "sleep";
#[cfg(windows)]
const SLEEPER: &str = "ping";

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
    let left = left_behind(&file, Duration::from_millis(200), SLEEPER).expect("a window was left");
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
    assert_eq!(
        left_behind(&file, Duration::from_millis(200), SLEEPER),
        None
    );
}

/// The number of a window that ended is held by a program that began after the last note was
/// written. It is no window's, and is not said to be one that was left behind.
#[test]
fn a_program_that_began_after_the_last_note_is_none_of_the_windows() {
    let folder = tempfile::tempdir().unwrap();
    let another = sleeper();
    let file = noted(folder.path(), another.id(), Duration::from_secs(3600));
    assert_eq!(
        left_behind(&file, Duration::from_millis(200), SLEEPER),
        None
    );
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

/// CI run 41973700452 (main `addd83cfe`, Windows): a window's number, given back, was held by
/// `sort.exe` when the rig ended, and was said to be a window left behind. A program that
/// took the number and began within the slack of the last note, or before a later window's
/// note, is not told from a window by its start: it runs another program than the stand-in,
/// and is none of the windows.
#[test]
fn a_number_held_by_another_program_than_the_stand_in_is_none_of_the_windows_whenever_it_began() {
    let folder = tempfile::tempdir().unwrap();
    let another = sleeper();
    let file = noted(folder.path(), another.id(), Duration::ZERO);
    assert_eq!(
        left_behind(&file, Duration::from_millis(200), "fake-agent"),
        None
    );
}

/// A command line runs a program where a word of it is that program's file, whatever its
/// folder, its `.exe` or its case: and no other.
#[test]
fn a_command_line_names_its_program_by_its_file_alone() {
    assert!(names("/x/target/debug/fake-agent --worker", "fake-agent"));
    assert!(names(
        r#""C:\a b\fake-agent.EXE" "C:\a b\agent" --x"#,
        "fake-agent"
    ));
    assert!(names(
        "C:\\Windows\\System32\\PING.EXE -n 61 127.0.0.1",
        "ping"
    ));
    assert!(!names("C:\\Windows\\System32\\sort.exe", "fake-agent"));
    assert!(!names("/bin/sh -c fake-agent-helper", "fake-agent"));
    assert!(!names("", "fake-agent"));
}

/// The program the rig takes a noted number's for is the binary that notes it: the crate's
/// `fake-agent`, whose source writes the number down.
#[test]
fn the_program_a_window_runs_is_the_stand_in_binary_that_notes_its_number() {
    let manifest = fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml")).unwrap();
    let bin = format!("name = \"{NOTER}\"\npath = \"src/bin/fake_agent.rs\"");
    assert!(
        manifest.contains(&bin),
        "no binary {NOTER} of src/bin/fake_agent.rs"
    );
    let source = fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/bin/fake_agent.rs"
    ))
    .unwrap();
    assert!(
        source.contains("note_process(beside, pid,"),
        "fake_agent notes its number"
    );
}

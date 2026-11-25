//! A `cf ui` that refuses to start never waits on whoever reads what it says (P23-A1,
//! finding 13): the refusal line (under `--json`) and the words are said within a bound,
//! and the process ends with the refusal's status whatever became of them. Run against the
//! real `cf`, over real pipes, a home that has a marker and no ledger being the refusal.

// The tests start cf themselves, and make the folders and files they start it on.
#![allow(clippy::disallowed_methods, clippy::expect_used)]

use std::io::{PipeReader, PipeWriter, Read, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// A refusal's status for a ledger that cannot be saved to.
const CANNOT_SAVE: i32 = 74;

/// A scratch home that has had a board (its marker) and has no ledger: a start that is refused.
fn a_home_that_lost_its_ledger() -> tempfile::TempDir {
    let root = tempfile::tempdir().expect("a home");
    std::fs::write(root.path().join("consensflow.board"), "{\"board\":true}\n").expect("a marker");
    root
}

/// `cf ui` on `home`, with none of the variables a window of a board gives.
fn cf_ui(home: &Path, args: &[&str]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_cf"));
    for (name, _) in std::env::vars_os() {
        if name.to_string_lossy().starts_with("CONSENSFLOW_") {
            command.env_remove(name);
        }
    }
    command
        .arg("ui")
        .args(args)
        .env("CONSENSFLOW_HOME", home)
        .env("HOME", home)
        .env("USERPROFILE", home)
        .current_dir(home)
        .stdin(Stdio::null());
    command
}

/// A pipe that is full and that nobody drains: a writer thread is blocked on it, and whatever
/// is written to it next waits behind. Its reader is kept until the test ends.
fn a_full_pipe() -> (PipeReader, PipeWriter) {
    let (reader, writer) = std::io::pipe().expect("a pipe");
    let mut filling = writer.try_clone().expect("a clone of the writer");
    std::thread::spawn(move || {
        let _ = filling.write_all(&vec![b'x'; 4 << 20]);
    });
    std::thread::sleep(Duration::from_millis(500));
    (reader, writer)
}

/// The status `child` ends with within `within`, or none: it is ended then, being this test's.
fn ends_within(child: &mut Child, within: Duration) -> Option<i32> {
    let began = Instant::now();
    while began.elapsed() < within {
        if let Some(status) = child.try_wait().expect("the child is looked at") {
            return status.code();
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let _ = child.kill();
    let _ = child.wait();
    None
}

#[test]
fn a_refusal_does_not_wait_for_an_error_output_nobody_drains() {
    let home = a_home_that_lost_its_ledger();
    let (reader, writer) = a_full_pipe();
    let mut child = cf_ui(home.path(), &["--no-open"])
        .stdout(Stdio::null())
        .stderr(writer)
        .spawn()
        .expect("cf starts");
    assert_eq!(
        ends_within(&mut child, Duration::from_secs(10)),
        Some(CANNOT_SAVE),
        "the refused start ends with its status, the words unread"
    );
    drop(reader);
}

#[test]
fn a_refusal_under_json_does_not_wait_for_an_output_nobody_drains() {
    let home = a_home_that_lost_its_ledger();
    let (reader, writer) = a_full_pipe();
    let mut child = cf_ui(home.path(), &["--json", "--no-open"])
        .stdout(writer)
        .stderr(Stdio::null())
        .spawn()
        .expect("cf starts");
    assert_eq!(
        ends_within(&mut child, Duration::from_secs(10)),
        Some(CANNOT_SAVE),
        "the refused start ends with its status, the line unread"
    );
    drop(reader);
}

/// The first line of what the child wrote to its output, and all it wrote to its error output.
fn said(home: &Path, args: &[&str]) -> (Option<i32>, u32, String, String) {
    let mut child = cf_ui(home, args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("cf starts");
    let pid = child.id();
    let status = ends_within(&mut child, Duration::from_secs(30));
    let (mut out, mut err) = (String::new(), String::new());
    child
        .stdout
        .take()
        .expect("piped")
        .read_to_string(&mut out)
        .expect("output");
    child
        .stderr
        .take()
        .expect("piped")
        .read_to_string(&mut err)
        .expect("error output");
    (status, pid, out, err)
}

#[test]
fn under_json_a_refused_start_says_its_case_in_the_handle_lines_place() {
    let home = a_home_that_lost_its_ledger();
    let (status, pid, out, err) = said(home.path(), &["--json", "--no-open"]);
    assert_eq!(status, Some(CANNOT_SAVE), "{err}");
    let lines: Vec<&str> = out.lines().collect();
    let [line] = lines.as_slice() else {
        panic!("one line on the output: {out:?}");
    };
    let line: serde_json::Value = serde_json::from_str(line).expect("a JSON line");
    let words = cf_base::home::lost_ledger_words(&home.path().join("consensflow.db"));
    assert_eq!(
        line,
        serde_json::json!({ "refused": { "exit": 74, "pid": pid, "words": words } })
    );
    assert_eq!(err.trim_end(), format!("cf: {words}"));
}

#[test]
fn a_person_s_refused_start_says_the_words_alone() {
    let home = a_home_that_lost_its_ledger();
    let (status, _, out, err) = said(home.path(), &["--no-open"]);
    assert_eq!(status, Some(CANNOT_SAVE), "{err}");
    assert_eq!(out, "");
    assert!(err.starts_with("cf: Work has stopped"), "{err}");
}

#[test]
fn an_argument_refused_under_json_is_said_the_same_way() {
    let home = a_home_that_lost_its_ledger();
    let (status, pid, out, err) = said(home.path(), &["--json", "--nope"]);
    assert_eq!(status, Some(1), "{err}");
    let line: serde_json::Value = serde_json::from_str(out.trim_end()).expect("a JSON line");
    assert_eq!(line["refused"]["exit"], 1, "{out}");
    assert_eq!(line["refused"]["pid"], pid, "{out}");
    assert!(
        line["refused"]["words"]
            .as_str()
            .is_some_and(|words| words.starts_with("Unknown option '--nope'")),
        "{out}"
    );
}

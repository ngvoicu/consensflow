use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

use super::*;

fn identity(pid: i32, unique: u64) -> Identity {
    Identity {
        pid,
        born: (1, 0),
        unique,
    }
}

/// A program that says it is ready, and once told to go starts a child that starts a
/// process of its own (`sleep`) and says its number; the program says the child's
/// number and that one's, and waits to be told to go.
const FAMILY: &str = "import subprocess, sys; print('ready', flush=True); input(); \
    child = subprocess.Popen([sys.executable, '-c', \
        'import subprocess, time; g = subprocess.Popen([\"sleep\", \"43\"]); \
         print(g.pid, flush=True); time.sleep(43)'], stdout=subprocess.PIPE, text=True); \
    print(child.pid, child.stdout.readline().strip(), flush=True); input()";

#[allow(clippy::disallowed_methods)] // A test's own programs.
#[test]
fn what_a_program_starts_is_seen_and_what_that_starts_and_a_parent_that_has_gone_stays_seen() {
    let mut program = Command::new("/usr/bin/python3")
        .args(["-c", FAMILY])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("start the program");
    let root = i32::try_from(program.id()).expect("a process number");
    let tracker = Tracker::follow(root).expect("follow the program");
    let mut output = BufReader::new(program.stdout.take().expect("its output"));
    let mut input = program.stdin.take().expect("its input");
    let mut line = String::new();
    output.read_line(&mut line).expect("it is ready");
    input.write_all(b"\n").expect("tell it to go");
    line.clear();
    output.read_line(&mut line).expect("the numbers");
    let numbers: Vec<i32> = line
        .split_whitespace()
        .map(|number| number.parse().expect("a process number"))
        .collect();
    let (child, grandchild) = (numbers[0], numbers[1]);

    // What the system says it started is what is seen of it, told by when it began.
    let deadline = Instant::now() + Duration::from_secs(10);
    let (seen_child, seen_grandchild) = loop {
        let seen = tracker.seen();
        let by_pid = |pid: i32| seen.values().find(|identity| identity.pid == pid).copied();
        if let (Some(child), Some(grandchild)) = (by_pid(child), by_pid(grandchild)) {
            break (child, grandchild);
        }
        assert!(
            Instant::now() < deadline,
            "the processes it started were never seen: {seen:?}"
        );
        thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(Some(seen_child), process(child).map(|p| p.identity));
    assert_eq!(
        Some(seen_grandchild),
        process(grandchild).map(|p| p.identity)
    );

    // The child goes, with the program's end of it: the process it started is its to
    // name still, by the identifier no other process takes.
    // SAFETY: kill takes two integers and touches no memory of ours.
    unsafe { libc::kill(child, libc::SIGKILL) };
    let deadline = Instant::now() + Duration::from_secs(10);
    while process(child).is_some() {
        assert!(Instant::now() < deadline, "the child never ended");
        thread::sleep(Duration::from_millis(10));
    }
    assert!(tracker.seen().contains_key(&seen_child.unique));
    // SAFETY: as above.
    unsafe { libc::kill(grandchild, libc::SIGKILL) };
    input.write_all(b"\n").expect("tell it to end");
    program.wait().expect("the program ends");
}

#[test]
fn a_process_that_started_nothing_that_runs_is_let_go_of_once_it_has_gone_and_been_kept_a_while() {
    let long_ago = Instant::now()
        .checked_sub(KEPT * 2)
        .expect("a moment that long ago");
    let tracker = Tracker::having_seen([
        // Gone, long seen, and nobody's parent: let go of.
        (identity(10, 1), long_ago),
        // Long seen, and gone, but what it started still runs.
        (identity(11, 2), long_ago),
        // Gone and nobody's parent, but seen just now: the look that says so may be
        // older than the sight of it.
        (identity(12, 3), Instant::now()),
        // Long seen, and it runs.
        (identity(13, 4), long_ago),
    ]);
    tracker.forget(|seen| seen.unique == 2 || seen.unique == 4);
    let mut kept: Vec<u64> = tracker.seen().keys().copied().collect();
    kept.sort_unstable();
    assert_eq!(kept, [2, 3, 4]);
}

#[test]
fn a_process_that_does_not_exist_is_not_followed() {
    // A number no process has: the largest the system gives.
    assert!(Tracker::follow(i32::MAX).is_err());
}

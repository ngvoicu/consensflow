//! A Muse Code chief run as its launcher itself, through the native daemon
//! (`cf ui --json --no-open`, the debug `cf`): the launcher is asked `--version` at the
//! chief's launch, a supported one gets its pane asked for, 1.4.12 is refused before any
//! pane is, and the daemon answers its API after both. It is the proof on Windows, where
//! the stand-in is a native program (`Root::muse_launcher_that_says`).

// The tests start cf themselves, and make the folders and files they start it on.
#![allow(clippy::disallowed_methods, clippy::expect_used)]
// The shared helper holds what other daemon tests use.
#![allow(dead_code)]

mod daemon;

use std::time::{Duration, Instant};

use daemon::{Daemon, Frames, Root};
use serde_json::Value;

/// What the app's end sees of a chief's launch: whether a pane was asked for, within the
/// time given, and every frame of the daemon's meanwhile.
fn watch(
    root: &Root,
    daemon: &mut Daemon,
    project: i64,
    seconds: u64,
) -> (bool, Vec<Value>, Frames) {
    let frames = daemon.frames();
    daemon.resume(project);
    let mut seen = Vec::new();
    let mut opened = false;
    let until = Instant::now() + Duration::from_secs(seconds);
    while Instant::now() < until && !opened && !root.log().contains("launch failed") {
        let Some(frame) = frames.next(Duration::from_millis(200)) else {
            continue;
        };
        opened = frame["kind"] == "req" && frame["op"] == "pane.open";
        seen.push(frame);
    }
    // A pane asked for just after a refusal would show here.
    let late = Instant::now() + Duration::from_millis(500);
    while Instant::now() < late {
        if let Some(frame) = frames.next(Duration::from_millis(100)) {
            opened |= frame["kind"] == "req" && frame["op"] == "pane.open";
            seen.push(frame);
        }
    }
    (opened, seen, frames)
}

#[test]
fn a_launcher_that_says_a_supported_version_gets_its_chief_s_pane_and_the_daemon_still_answers() {
    let root = Root::new();
    root.muse_launcher_that_says("Muse Code 1.4.24 (1.4.24-R7500)");
    let project = root.open_muse_project();
    let mut daemon = Daemon::start(&root);
    let (opened, seen, frames) = watch(&root, &mut daemon, project, 30);
    assert!(
        opened,
        "a pane was asked for; frames: {seen:?}; log:\n{}",
        root.log()
    );
    daemon.ping("r-after");
    assert!(answers(&frames, "r-after"), "the daemon answers its API");
}

#[test]
fn a_launcher_that_says_1_4_12_is_refused_before_any_pane_and_the_daemon_still_answers() {
    let root = Root::new();
    root.muse_launcher_that_says("Muse Code 1.4.12 (1.4.12-R6000.1)");
    let project = root.open_muse_project();
    let mut daemon = Daemon::start(&root);
    let (opened, seen, frames) = watch(&root, &mut daemon, project, 30);
    assert!(!opened, "no pane is asked for; frames: {seen:?}");
    let said = format!("{seen:?}\n{}", root.log());
    assert!(
        said.contains("Muse Code 1.4.13 or newer is required"),
        "the refusal is in the daemon's words: {said}"
    );
    daemon.ping("r-after");
    assert!(answers(&frames, "r-after"), "the daemon answers its API");
}

/// Whether the daemon answered the request `id` on its bridge within ten seconds.
fn answers(frames: &Frames, id: &str) -> bool {
    let until = Instant::now() + Duration::from_secs(10);
    while Instant::now() < until {
        if let Some(frame) = frames.next(Duration::from_millis(200)) {
            if frame["kind"] == "res" && frame["id"] == id {
                return true;
            }
        }
    }
    false
}

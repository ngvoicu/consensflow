//! The native daemon's stop as a process (`cf ui --json --no-open` with
//! `CONSENSFLOW_DAEMON=native`), over real pipes, as the app ends it: its
//! input ended; SIGTERM with its input left open (Windows delivers none);
//! its output broken with its input left open. Each ends the daemon with exit
//! 0, and its log as `core-daemon.test.mjs:186-191` reads it.
//!
//! Each is held twice: with nothing going on, where the stop is prompt, and
//! with everything open at once that a stop has to get past, which takes the
//! one deadline of a second and no more (the app ends the daemon 2 s after it
//! asks): a pass waiting on a window that never answers, a door waiting for an
//! answer, an idle connection, and a request with its body half sent.

// The tests start cf themselves.
#![allow(clippy::disallowed_methods)]

mod daemon;

use std::time::{Duration, Instant};

use daemon::{Client, Daemon, Root};
use serde_json::{json, Value};

/// How a daemon is asked to stop.
#[derive(Debug, Clone, Copy)]
enum How {
    InputEnded,
    #[cfg(unix)]
    Sigterm,
    OutputBroken,
}

impl How {
    /// What its log says it stopped for.
    fn reason(self) -> &'static str {
        match self {
            How::InputEnded => "stdin ended",
            #[cfg(unix)]
            How::Sigterm => "SIGTERM",
            How::OutputBroken => "the bridge failed",
        }
    }
}

/// The lines of the daemon's log without the time before each.
fn said(log: &str) -> Vec<String> {
    log.lines()
        .map(|line| {
            if line.starts_with(' ') {
                line.to_owned()
            } else {
                line.split_once(' ')
                    .map_or(line, |(_, rest)| rest)
                    .to_owned()
            }
        })
        .collect()
}

/// The two lines every stop ends its log with: why, and how big it was, and
/// that it exited 0.
fn assert_ends_with_its_stop(lines: &[String], how: How) {
    let [.., stop, exit] = lines else {
        panic!("a log of fewer than two lines: {lines:?}");
    };
    let expected = format!("info stop: {}; rss ", how.reason());
    let size = stop
        .strip_prefix(&expected)
        .unwrap_or_else(|| panic!("{stop:?} is not {expected:?}…: {lines:?}"));
    assert!(
        size.strip_suffix(" MB")
            .is_some_and(|mb| mb.parse::<u64>().is_ok()),
        "{stop}"
    );
    assert_eq!(exit, "info exit 0", "{lines:?}");
}

/// Asks `daemon` to stop as `how` says, and says when it was asked.
fn stop(daemon: &mut Daemon, how: How) -> Instant {
    match how {
        How::InputEnded => {
            let asked = Instant::now();
            daemon.end_input();
            asked
        }
        #[cfg(unix)]
        How::Sigterm => {
            let asked = Instant::now();
            daemon.terminate();
            asked
        }
        How::OutputBroken => {
            // Nobody reads its output any more, and its next write finds out.
            let asked = Instant::now();
            daemon.ping("r-after-the-break");
            asked
        }
    }
}

fn hows() -> Vec<How> {
    vec![
        How::InputEnded,
        How::OutputBroken,
        #[cfg(unix)]
        How::Sigterm,
    ]
}

#[test]
fn a_quiet_daemon_starts_its_log_with_its_pid_and_ends_it_with_why_it_stopped() {
    for how in hows() {
        let root = Root::new();
        let mut daemon = Daemon::start(&root);
        let handle = daemon.handle.clone();
        assert!(handle["url"]
            .as_str()
            .is_some_and(|url| url.starts_with("http://127.0.0.1:") && url.ends_with('/')));
        assert_eq!(handle["token"].as_str().map(str::len), Some(48));
        // Its bridge answers: it is the first thing a page asks.
        daemon.ping("r-1");
        let frames = daemon.frames();
        let answer = frames
            .next(Duration::from_secs(10))
            .expect("an answer to the ping");
        assert_eq!(
            answer,
            json!({ "v": 1, "id": "r-1", "kind": "res", "op": "ping", "body": { "ok": true } })
        );
        let pid = daemon.child.id();
        if let How::OutputBroken = how {
            frames.leave_after_the_next_line();
            daemon.ping("r-2");
            // Its answer is read, and the output let go.
            let _ = frames.next(Duration::from_secs(10));
            std::thread::sleep(Duration::from_millis(100));
        }
        let asked = stop(&mut daemon, how);
        let (code, took) = daemon.exits(asked, Duration::from_secs(10));
        assert_eq!(code, Some(0), "{how:?}: {}", daemon.errors.lock().unwrap());
        assert!(
            took < Duration::from_millis(800),
            "{how:?}: a quiet daemon is prompt: {took:?}"
        );

        let lines = said(&root.log());
        assert_ends_with_its_stop(&lines, how);
        assert_eq!(
            lines[0],
            format!(
                "info start pid {pid} rust {} home {}",
                env!("CARGO_PKG_VERSION"),
                root.home().display()
            ),
            "{how:?}"
        );
        match how {
            How::OutputBroken => {
                // The failure is written, with its cause under it, before the stop.
                assert_eq!(lines.len(), 5, "{how:?}: {lines:?}");
                assert_eq!(lines[1], "error the bridge failed");
                assert!(lines[2].starts_with("    "), "{lines:?}");
            }
            _ => assert_eq!(lines.len(), 3, "{how:?}: {lines:?}"),
        }
        assert_eq!(daemon.errors.lock().unwrap().as_str(), "", "{how:?}");
    }
}

#[test]
fn a_busy_daemon_is_stopped_within_one_deadline_whatever_is_open() {
    for how in hows() {
        let root = Root::new();
        let question = root.open_project_with_a_question();
        let mut daemon = Daemon::start(&root);
        let frames = daemon.frames();

        // The app's end: each window it is asked to open opens, and every
        // other request of the daemon's is left unanswered, so that what
        // waits on a window waits for good.
        let mut chief_token: Option<String> = None;
        let mut unanswered = false;
        let until = Instant::now() + Duration::from_secs(20);
        while Instant::now() < until && !(chief_token.is_some() && unanswered) {
            let Some(frame) = frames.next(Duration::from_millis(200)) else {
                continue;
            };
            if frame["kind"] != "req" {
                continue;
            }
            if frame["op"] == "pane.open" {
                let body = &frame["body"];
                if body["id"].as_str().is_some_and(|id| id.ends_with("-chief")) {
                    chief_token = body["env"]["CONSENSFLOW_TOKEN"].as_str().map(str::to_owned);
                }
                daemon.send(&json!({
                    "v": 1, "id": frame["id"], "kind": "res", "op": "pane.open",
                    "body": { "ok": true, "id": body["id"], "generation": body["generation"] }
                }));
            } else {
                unanswered = true;
            }
        }
        let token = chief_token.expect("the chief's window was opened with its token");
        assert!(
            unanswered,
            "a pass is waiting on a window that never answers"
        );

        let url = daemon.url();
        let authorization = format!("Authorization: Bearer {token}\r\n");
        // A door waiting for the answer to a question the chief asked.
        let mut door = Client::connect(&url);
        door.write(&format!(
            "GET /api/questions/{question}?wait=25000 HTTP/1.1\r\nHost: t\r\n{authorization}\r\n"
        ));
        // A connection that has said nothing.
        let mut idle = Client::connect(&url);
        // A request whose body is half sent.
        let mut half = Client::connect(&url);
        half.write(&format!(
            "POST /api/answers HTTP/1.1\r\nHost: t\r\n{authorization}Content-Type: application/json\r\nContent-Length: 1000\r\n\r\n{{\"question\":"
        ));
        std::thread::sleep(Duration::from_millis(300));

        if let How::OutputBroken = how {
            // The output is let go after the next line the app reads.
            frames.leave_after_the_next_line();
            daemon.ping("r-last");
            std::thread::sleep(Duration::from_millis(300));
        }
        let asked = stop(&mut daemon, how);
        let (code, took) = daemon.exits(asked, Duration::from_secs(10));
        assert_eq!(code, Some(0), "{how:?}");
        // One deadline of a second for all of it, which the body half sent
        // and the pass hold to the end; the app waits 2 s.
        assert!(
            took >= Duration::from_millis(900),
            "{how:?}: the stop did not wait: {took:?}"
        );
        assert!(took < Duration::from_millis(1_600), "{how:?}: {took:?}");

        let (answered, ended) = door.read_to_the_end();
        assert!(ended, "{how:?}: the door's connection ended");
        assert!(
            answered.starts_with("HTTP/1.1 200 OK"),
            "{how:?}: {answered}"
        );
        assert!(
            answered.ends_with(r#""answer":null}"#),
            "{how:?}: {answered}"
        );
        let (nothing, ended) = idle.read_to_the_end();
        assert!(
            (nothing.as_str(), ended) == ("", true),
            "{how:?}: the idle connection ended: {nothing:?}"
        );
        let (_, ended) = half.read_to_the_end();
        assert!(ended, "{how:?}: what was open at the deadline was dropped");

        assert_ends_with_its_stop(&said(&root.log()), how);
    }
}

#[test]
fn without_the_switch_cf_ui_is_not_the_native_daemon() {
    // Not the switch's value: the daemon is asked for by `native` and nothing else.
    let root = Root::new();
    let ran = std::process::Command::new(env!("CARGO_BIN_EXE_cf"))
        .args(["ui", "--json", "--no-open"])
        .env_clear()
        .env("CONSENSFLOW_DAEMON", "yes")
        .env("CONSENSFLOW_HOME", root.home())
        .env("HOME", root.dir.path())
        .stdin(std::process::Stdio::null())
        .output()
        .expect("cf runs");
    // It goes to the CLI's Node sources, which this binary has no runtime for here.
    assert_ne!(ran.status.code(), Some(0));
    assert!(
        String::from_utf8_lossy(&ran.stderr).contains("CONSENSFLOW_NODE is not set"),
        "{}",
        String::from_utf8_lossy(&ran.stderr)
    );
    assert!(!root.home().join("daemon.log").exists(), "no daemon ran");
}

#[test]
fn a_window_s_cf_ui_is_the_board_and_never_the_daemon() {
    let root = Root::new();
    let ran = std::process::Command::new(env!("CARGO_BIN_EXE_cf"))
        .args(["ui", "--json", "--no-open"])
        .env_clear()
        .env("CONSENSFLOW_DAEMON", "native")
        .env("CONSENSFLOW_TOKEN", "a-windows-token")
        .env("CONSENSFLOW_URL", "http://127.0.0.1:9")
        .env("CONSENSFLOW_HOME", root.home())
        .env("HOME", root.dir.path())
        .stdin(std::process::Stdio::null())
        .output()
        .expect("cf runs");
    assert_ne!(ran.status.code(), Some(0), "the board has no such command");
    assert!(!root.home().join("daemon.log").exists(), "no daemon ran");
}

#[test]
fn a_daemon_that_cannot_start_says_why_in_cf_s_words_and_exits_1() {
    let root = Root::new();
    // A ledger that is held: a second daemon on the same home.
    let first = Daemon::start(&root);
    let ran = std::process::Command::new(env!("CARGO_BIN_EXE_cf"))
        .args(["ui", "--json", "--no-open"])
        .env_clear()
        .env("CONSENSFLOW_DAEMON", "native")
        .env("CONSENSFLOW_HOME", root.home())
        .env("HOME", root.dir.path())
        .stdin(std::process::Stdio::piped())
        .output()
        .expect("cf runs");
    assert_eq!(ran.status.code(), Some(1));
    let said = String::from_utf8_lossy(&ran.stderr);
    assert!(
        said.starts_with("cf: another ConsensFlow has ") && said.contains("consensflow.db open"),
        "{said}"
    );
    assert_eq!(String::from_utf8_lossy(&ran.stdout), "", "no handle line");
    drop(first);
}

#[test]
fn an_option_cf_ui_does_not_know_is_refused_as_node_refuses_it() {
    let ran = std::process::Command::new(env!("CARGO_BIN_EXE_cf"))
        .args(["ui", "--foo"])
        .env_clear()
        .env("CONSENSFLOW_DAEMON", "native")
        .stdin(std::process::Stdio::null())
        .output()
        .expect("cf runs");
    assert_eq!(ran.status.code(), Some(1));
    assert_eq!(
        String::from_utf8_lossy(&ran.stderr),
        "cf: Unknown option '--foo'. To specify a positional argument starting with a '-', place it at the end of the command after '--', as in '-- \"--foo\"\n"
    );
}

#[test]
fn the_handle_line_is_json_for_the_app() {
    let root = Root::new();
    let daemon = Daemon::start(&root);
    let Value::Object(handle) = &daemon.handle else {
        panic!("the handle line is an object");
    };
    assert_eq!(handle.keys().collect::<Vec<_>>(), ["url", "token"]);
}

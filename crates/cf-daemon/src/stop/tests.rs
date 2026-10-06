//! The stop, in process: the latch's first reason wins, the log ends as the
//! app's tests read it, and the whole of it costs no more than one deadline
//! whatever is open: a pass that never ends, a door waiting, an idle
//! connection and a body half sent. (The same as a process, with real pipes
//! and signals, is `crates/cf/tests/daemon_stop.rs`.)

use std::cell::Cell;
use std::future::pending;
use std::time::Instant;

use cf_base::env::Env;
use cf_harness::admin::Capture;
use cf_harness::seams::processes::{Limits, Processes, Program, Streams};
use cf_harness::seams::SystemProcesses;
use cf_ledger::{open_ledger, Options as LedgerOptions};
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::task::LocalSet;

use super::*;
use crate::api::answer::Answer;
use crate::api::context::Closing;
use crate::api::Handler;
use crate::console::Console;
use crate::testing::{worked, Said, Worked};

struct Rig {
    home: tempfile::TempDir,
    stopping: Stopping,
    exited: Rc<RefCell<Vec<i32>>>,
    /// What the daemon's children are started through, and ended with.
    processes: Rc<SystemProcesses>,
}

/// A daemon's parts to stop, over a home of their own: an API whose handler
/// has a door that waits for the stop and a route that reads a body, a pass
/// loop running `pass`, and a ledger. Called inside the local set the test
/// runs in.
async fn rig(pass: crate::pass::Pass) -> Rig {
    let Worked { home, spawn } = worked();
    let errors = Rc::clone(spawn.errors());
    let closing = Closing::new();
    let door = closing.clone();
    let handler: Rc<Handler> = Rc::new(move |mut request| {
        let door = door.clone();
        Box::pin(async move {
            match request.path.as_str() {
                "/door" => {
                    door.wait().await;
                    Ok(Answer::ok(json!({ "answer": null })))
                }
                "/body" => {
                    request.json().await?;
                    Ok(Answer::ok(json!({})))
                }
                _ => Ok(Answer::ok(json!({}))),
            }
        })
    });
    let api = Rc::new(
        Api::start(handler, closing, Rc::clone(&spawn))
            .await
            .unwrap(),
    );
    let console = Rc::new(Console::to(Said::default(), || {}));
    let passes = Rc::new(OnceCell::new());
    let _ = passes.set(PassLoop::start(pass, Rc::clone(&spawn), console));
    let ledger = open_ledger(
        &home.path().join("consensflow.db"),
        LedgerOptions::default(),
    )
    .unwrap();
    let exited: Rc<RefCell<Vec<i32>>> = Rc::default();
    let noted = Rc::clone(&exited);
    let processes = Rc::new(SystemProcesses::new(Env::default()));
    let ending = Rc::clone(&processes);
    let stopping = Stopping {
        errors,
        passes,
        api,
        ends_children: Rc::new(move || ending.end_all()),
        ledger: Rc::new(RefCell::new(ledger)),
        exit: Rc::new(move |code| noted.borrow_mut().push(code)),
    };
    Rig {
        home,
        stopping,
        exited,
        processes,
    }
}

fn idle_pass() -> crate::pass::Pass {
    Box::new(|| Box::pin(async { Ok(()) }))
}

fn log_lines(rig: &Rig) -> Vec<String> {
    std::fs::read_to_string(rig.home.path().join("daemon.log"))
        .unwrap_or_default()
        .lines()
        .map(|line| {
            line.split_once(' ')
                .map_or(line, |(_, rest)| rest)
                .to_owned()
        })
        .collect()
}

#[test]
fn the_first_reason_wins_and_the_daemon_stops_for_that_one_alone() {
    let latch = Latch::new();
    assert_eq!(latch.reason(), None);
    latch.trip("stdin ended");
    latch.trip("SIGTERM");
    latch.trip("the bridge failed");
    assert_eq!(latch.reason().as_deref(), Some("stdin ended"));
}

#[tokio::test]
async fn whoever_waits_for_the_latch_is_told_why_when_it_is_tripped_and_at_once_after() {
    LocalSet::new()
        .run_until(async {
            let latch = Latch::new();
            let (one, other) = (Rc::clone(&latch), Rc::clone(&latch));
            let waiting = tokio::task::spawn_local(async move { one.tripped().await });
            let also = tokio::task::spawn_local(async move { other.tripped().await });
            tokio::task::yield_now().await;
            assert!(!waiting.is_finished());
            latch.trip("SIGINT");
            assert_eq!(waiting.await.unwrap(), "SIGINT");
            assert_eq!(also.await.unwrap(), "SIGINT");
            assert_eq!(latch.tripped().await, "SIGINT");
        })
        .await;
}

#[tokio::test]
async fn a_stop_says_why_and_how_big_the_daemon_is_and_ends_the_log_with_exit_0() {
    LocalSet::new()
        .run_until(async {
            let rig = rig(idle_pass()).await;
            let started = Instant::now();
            rig.stopping.run("stdin ended").await;
            assert!(
                started.elapsed() < Duration::from_millis(300),
                "nothing waits"
            );
            let lines = log_lines(&rig);
            assert_eq!(lines.len(), 2, "{lines:?}");
            let size = lines[0]
                .strip_prefix("info stop: stdin ended; rss ")
                .unwrap();
            let megabytes: u64 = size.strip_suffix(" MB").unwrap().parse().unwrap();
            assert!(megabytes > 0);
            assert_eq!(lines[1], "info exit 0");
            assert_eq!(*rig.exited.borrow(), [0]);
        })
        .await;
}

#[tokio::test]
async fn the_ledger_is_closed_at_the_tail_so_the_next_daemon_may_hold_it() {
    LocalSet::new()
        .run_until(async {
            let rig = rig(idle_pass()).await;
            let file = rig.home.path().join("consensflow.db");
            assert!(
                open_ledger(&file, LedgerOptions::default()).is_err(),
                "held until the stop"
            );
            rig.stopping.run("SIGTERM").await;
            open_ledger(&file, LedgerOptions::default())
                .expect("the lock went with the close")
                .close()
                .unwrap();
        })
        .await;
}

async fn raw(api: &Api, request: &str) -> TcpStream {
    let address = api.url().strip_prefix("http://").unwrap().to_owned();
    let mut stream = TcpStream::connect(address).await.unwrap();
    stream.write_all(request.as_bytes()).await.unwrap();
    stream
}

#[tokio::test]
async fn a_pass_that_never_ends_a_door_an_idle_connection_and_a_body_half_sent_cost_one_deadline() {
    LocalSet::new()
        .run_until(async {
            let rig = rig(Box::new(|| Box::pin(pending()))).await;
            rig.stopping.passes.get().unwrap().kick();
            let api = &rig.stopping.api;
            let mut door = raw(api, "GET /door HTTP/1.1\r\nHost: t\r\n\r\n").await;
            let mut idle = TcpStream::connect(api.url().strip_prefix("http://").unwrap())
                .await
                .unwrap();
            let mut half = raw(
                api,
                "POST /body HTTP/1.1\r\nHost: t\r\nContent-Length: 100\r\n\r\n{\"a\":1,\"b\"",
            )
            .await;
            tokio::time::sleep(Duration::from_millis(100)).await;

            let started = Instant::now();
            rig.stopping.run("SIGTERM").await;
            let took = started.elapsed();
            // One deadline of a second for all of it, and the tail after.
            assert!(
                took >= Duration::from_secs(1),
                "the body half sent and the pass hold it: {took:?}"
            );
            assert!(took < Duration::from_millis(1_300), "{took:?}");
            assert_eq!(*rig.exited.borrow(), [0]);

            // The door was answered at once, and what was open is gone.
            let mut answered = Vec::new();
            door.read_to_end(&mut answered).await.unwrap();
            let text = String::from_utf8_lossy(&answered);
            assert!(text.starts_with("HTTP/1.1 200 OK"), "{text}");
            assert!(text.ends_with(r#"{"answer":null}"#), "{text}");
            let mut end = [0; 1];
            assert_eq!(
                idle.read(&mut end).await.unwrap(),
                0,
                "the idle connection ended"
            );
            assert!(
                matches!(half.read(&mut end).await, Ok(0) | Err(_)),
                "so was the half-sent one"
            );
            let lines = log_lines(&rig);
            assert!(lines
                .first()
                .unwrap()
                .starts_with("info stop: SIGTERM; rss "));
            assert_eq!(lines.last().map(String::as_str), Some("info exit 0"));
        })
        .await;
}

#[tokio::test]
async fn with_only_doors_and_idle_connections_open_the_stop_is_prompt() {
    LocalSet::new()
        .run_until(async {
            let rig = rig(idle_pass()).await;
            let mut door = raw(&rig.stopping.api, "GET /door HTTP/1.1\r\nHost: t\r\n\r\n").await;
            let _idle = TcpStream::connect(rig.stopping.api.url().strip_prefix("http://").unwrap())
                .await
                .unwrap();
            tokio::time::sleep(Duration::from_millis(100)).await;
            let started = Instant::now();
            rig.stopping.run("stdin ended").await;
            assert!(
                started.elapsed() < Duration::from_millis(400),
                "{:?}",
                started.elapsed()
            );
            let mut answered = Vec::new();
            door.read_to_end(&mut answered).await.unwrap();
            assert!(String::from_utf8_lossy(&answered).contains(r#"{"answer":null}"#));
        })
        .await;
}

#[tokio::test]
async fn a_pass_that_ends_within_the_deadline_is_waited_for_and_no_longer() {
    LocalSet::new()
        .run_until(async {
            let finished = Rc::new(Cell::new(false));
            let marked = Rc::clone(&finished);
            let rig = rig(Box::new(move || {
                let marked = Rc::clone(&marked);
                Box::pin(async move {
                    tokio::time::sleep(Duration::from_millis(300)).await;
                    marked.set(true);
                    Ok(())
                })
            }))
            .await;
            rig.stopping.passes.get().unwrap().kick();
            tokio::time::sleep(Duration::from_millis(50)).await;
            let started = Instant::now();
            rig.stopping.run("SIGTERM").await;
            let took = started.elapsed();
            assert!(finished.get(), "it was let end");
            assert!(
                took >= Duration::from_millis(200) && took < Duration::from_millis(700),
                "{took:?}"
            );
        })
        .await;
}

#[tokio::test]
async fn a_ledger_that_panics_at_its_close_is_written_down_and_the_process_still_ends() {
    LocalSet::new()
        .run_until(async {
            let rig = rig(idle_pass()).await;
            // Something holds the ledger as the tail takes it, and never lets
            // go: its close panics.
            std::mem::forget(rig.stopping.ledger.borrow());
            rig.stopping.run("SIGTERM").await;
            assert_eq!(*rig.exited.borrow(), [0], "it ended all the same");
            let lines = log_lines(&rig);
            assert!(
                lines.contains(&"error the ledger did not close".to_owned()),
                "{lines:?}"
            );
            assert_eq!(lines.last().map(String::as_str), Some("info exit 0"));
        })
        .await;
}

#[tokio::test]
async fn children_that_cannot_be_ended_are_written_down_and_the_ledger_is_still_closed() {
    LocalSet::new()
        .run_until(async {
            let mut rig = rig(idle_pass()).await;
            rig.stopping.ends_children = Rc::new(|| panic!("a child would not end"));
            rig.stopping.run("SIGTERM").await;
            assert_eq!(*rig.exited.borrow(), [0], "it ended all the same");
            let lines = log_lines(&rig);
            assert!(
                lines.contains(&"error the children did not end".to_owned()),
                "{lines:?}"
            );
            assert_eq!(lines.last().map(String::as_str), Some("info exit 0"));
            // The step after it was taken: the next daemon may hold the ledger.
            open_ledger(
                &rig.home.path().join("consensflow.db"),
                LedgerOptions::default(),
            )
            .expect("the lock went with the close")
            .close()
            .unwrap();
        })
        .await;
}

/// A program that runs for a minute: `sleep`, or on Windows `ping`.
#[cfg(any(unix, windows))]
fn for_a_minute() -> Program {
    let (executable, args) = if cfg!(windows) {
        (
            "C:\\Windows\\System32\\ping.exe",
            vec!["-n", "60", "127.0.0.1"],
        )
    } else {
        ("/bin/sleep", vec!["60"])
    };
    Program {
        executable: executable.into(),
        args: args.into_iter().map(str::to_owned).collect(),
        cwd: None,
        env: Env::from_vars(if cfg!(windows) {
            vec![("SystemRoot", "C:\\Windows")]
        } else {
            vec![("PATH", "/usr/bin:/bin")]
        }),
    }
}

#[cfg(any(unix, windows))]
#[tokio::test]
async fn the_children_still_running_are_ended_on_the_way_out() {
    LocalSet::new()
        .run_until(async {
            let rig = rig(idle_pass()).await;
            let child = rig.processes.spawn(for_a_minute(), Streams::Quiet).unwrap();
            assert!(!child.exited());
            rig.stopping.run("SIGTERM").await;
            tokio::time::timeout(Duration::from_secs(10), child.closed())
                .await
                .expect("it was ended with the daemon");
            assert!(child.exited());
        })
        .await;
}

#[cfg(any(unix, windows))]
#[tokio::test]
async fn the_programs_run_to_their_end_that_are_still_running_are_ended_on_the_way_out_too() {
    LocalSet::new()
        .run_until(async {
            let rig = rig(idle_pass()).await;
            // Run to their end and waited for, as the engine's probes of a CLI
            // and the harness admin's update are: `run` and `capture`.
            let limits = Limits {
                timeout: Duration::ZERO,
                max_buffer: 1024 * 1024,
            };
            let (running, capturing) = (Rc::clone(&rig.processes), Rc::clone(&rig.processes));
            let run =
                tokio::task::spawn_local(async move { running.run(for_a_minute(), limits).await });
            let capture =
                tokio::task::spawn_local(
                    async move { capturing.capture(for_a_minute(), limits).await },
                );
            tokio::time::sleep(Duration::from_millis(300)).await;
            assert!(
                !run.is_finished() && !capture.is_finished(),
                "both are running"
            );
            rig.stopping.run("SIGTERM").await;
            // Left running, they would answer in a minute: ended with the
            // daemon, they answer now, as failures.
            let (run, capture) = tokio::time::timeout(Duration::from_secs(10), async {
                (run.await, capture.await)
            })
            .await
            .expect("both were ended with the daemon");
            assert!(run.expect("the run was polled").is_err(), "run");
            assert!(capture.expect("the capture was polled").is_err(), "capture");
        })
        .await;
}

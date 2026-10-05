//! Both ends together: the local transport as the daemon runs it, over its
//! stdin and stdout, against the pane host's own threaded transport
//! (`Role::Host`), over operating system pipes.
//!
//! The clock is the real one here, and the daemon's ends are tokio's `File`
//! on the blocking pool, as `tokio::io::stdin` and `stdout` are. The host's
//! calls block, so each runs on a thread of its own: the daemon's reader runs
//! on the test's thread, and a host call made there would wait for it.

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::io::{pipe, BufRead, BufReader, Write};
use std::panic::{catch_unwind, resume_unwind, AssertUnwindSafe};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use cf_proto::bridge::Role;
use serde_json::json;
use tokio::sync::oneshot;
use tokio::task::LocalSet;

use crate::local::{Bridge, BridgeBuilder, Ended};
use crate::{Bridge as HostBridge, BridgeBuilder as HostBuilder, BridgeError};

#[cfg(unix)]
use std::os::fd::OwnedFd as Owned;
#[cfg(windows)]
use std::os::windows::io::OwnedHandle as Owned;

const PATIENCE: Duration = Duration::from_secs(20);

/// Runs a scenario on one thread with a `LocalSet`, on the real clock. A
/// runtime waits for the blocking reads of its pipes when it is dropped, so
/// one that fails does not wait for ever.
fn run_real<F: Future>(scenario: F) -> F::Output {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime");
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        LocalSet::new().block_on(&runtime, scenario)
    }));
    runtime.shutdown_timeout(Duration::from_secs(2));
    outcome.unwrap_or_else(|panic| resume_unwind(panic))
}

async fn within_real<F: Future>(future: F) -> F::Output {
    tokio::time::timeout(PATIENCE, future)
        .await
        .expect("timed out waiting")
}

async fn wait_for_real(condition: impl Fn() -> bool) {
    let until = Instant::now() + PATIENCE;
    while !condition() {
        assert!(Instant::now() < until, "timed out waiting");
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
}

/// An end of a pipe as the daemon reads and writes its own.
fn stream(end: impl Into<Owned>) -> tokio::fs::File {
    tokio::fs::File::from_std(std::fs::File::from(end.into()))
}

/// What `work` gives, once a thread of its own has done it.
fn on_a_thread<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> oneshot::Receiver<T> {
    let (done, answered) = oneshot::channel();
    std::thread::spawn(move || {
        let _ = done.send(work());
    });
    answered
}

/// The host and the daemon, joined by two pipes: what the host writes is
/// what the daemon reads and the other way round.
fn over_pipes(host: HostBuilder, daemon: BridgeBuilder) -> (HostBridge, Bridge) {
    let (daemon_input, host_output) = pipe().expect("the host's output pipe");
    let (host_input, mut daemon_output) = pipe().expect("the daemon's output pipe");
    // The handle line comes first: the daemon writes it before it speaks the
    // bridge, and the host reads it before it does.
    writeln!(
        daemon_output,
        "{}",
        json!({ "url": "http://127.0.0.1:1", "token": "t" })
    )
    .expect("the handle line");
    let connected = host
        .connect(host_input, host_output)
        .expect("the host connects");
    assert_eq!(connected.handle["token"], "t");
    let (daemon, connection) = daemon.connect(stream(daemon_input), stream(daemon_output));
    tokio::task::spawn_local(connection);
    (connected.bridge, daemon)
}

fn host_builder() -> HostBuilder {
    HostBuilder::new(Role::Host, 1024 * 1024)
}

fn daemon_builder() -> BridgeBuilder {
    BridgeBuilder::new(Role::Daemon)
}

/// What each end reports, shared with the thread the host reports on.
type Reports = Arc<Mutex<Vec<String>>>;

fn host_reports(builder: &mut HostBuilder) -> Reports {
    let reports = Reports::default();
    let sink = Arc::clone(&reports);
    builder.on_error(move |error| sink.lock().expect("reports").push(error.to_string()));
    reports
}

#[test]
fn a_daemon_and_a_host_answer_each_other_each_with_its_own_ids() {
    run_real(async {
        let mut host = host_builder();
        let reports = host_reports(&mut host);
        host.on("pane.open", |_, body| {
            Ok(json!({ "ok": true, "opened": body }))
        });
        let (host, daemon) = over_pipes(host, daemon_builder());
        daemon.on("board.get", |_, body| async move {
            Ok(json!({ "ok": true, "asked": body }))
        });

        let asked = on_a_thread({
            let host = host.clone();
            move || host.request("board.get", json!({ "project": 1 }), Some(5_000))
        });
        assert_eq!(
            within_real(asked).await.expect("the host's thread"),
            Ok(json!({ "ok": true, "asked": { "project": 1 } }))
        );
        assert_eq!(
            within_real(daemon.request("pane.open", json!({ "id": "p1" }), None)).await,
            Ok(json!({ "ok": true, "opened": { "id": "p1" } }))
        );
        assert_eq!(
            within_real(daemon.request("nobody.handles.this", json!({}), None)).await,
            Ok(json!({ "ok": false, "error": "unknown-op" }))
        );
        let unknown = on_a_thread({
            let host = host.clone();
            move || host.request("nobody.handles.this", json!({}), Some(5_000))
        });
        assert_eq!(
            within_real(unknown).await.expect("the host's thread"),
            Ok(json!({ "ok": false, "error": "unknown-op" }))
        );
        assert!(
            reports.lock().expect("reports").is_empty(),
            "nothing but frames crossed"
        );
        host.close_input();
    });
}

#[test]
fn the_hosts_events_reach_the_daemon_in_the_order_they_were_written() {
    run_real(async {
        let (host, daemon) = over_pipes(host_builder(), daemon_builder());
        let heard: Rc<RefCell<Vec<i64>>> = Rc::default();
        let sink = Rc::clone(&heard);
        daemon.on_event("tick", move |body| {
            sink.borrow_mut().extend(body["n"].as_i64());
        });
        let sent = on_a_thread({
            let host = host.clone();
            move || {
                for n in 0..300 {
                    host.stream_event("tick", json!({ "n": n }))
                        .expect("an event");
                }
            }
        });
        within_real(sent).await.expect("the host's thread");
        wait_for_real(|| heard.borrow().len() == 300).await;
        assert_eq!(*heard.borrow(), (0..300).collect::<Vec<_>>());
        host.close_input();
    });
}

#[test]
fn the_daemons_events_reach_the_host() {
    run_real(async {
        let mut host = host_builder();
        let heard: Arc<Mutex<Vec<i64>>> = Arc::default();
        let sink = Arc::clone(&heard);
        host.on_event("note", move |body| {
            sink.lock().expect("heard").extend(body["n"].as_i64());
        });
        let (host, daemon) = over_pipes(host, daemon_builder());
        for n in 0..50 {
            assert!(daemon.event("note", json!({ "n": n })));
        }
        // The host hands each event to a thread of its own, so what it hears
        // is all of them and not their order.
        wait_for_real(|| heard.lock().expect("heard").len() == 50).await;
        let mut all = heard.lock().expect("heard").clone();
        all.sort_unstable();
        assert_eq!(all, (0..50).collect::<Vec<_>>());
        host.close_input();
    });
}

#[test]
fn the_daemons_frames_cross_the_pipe_one_to_a_line_in_the_order_they_were_written() {
    run_real(async {
        let (daemon_input, host_output) = pipe().expect("the host's output pipe");
        let (host_input, daemon_output) = pipe().expect("the daemon's output pipe");
        let (daemon, connection) =
            daemon_builder().connect(stream(daemon_input), stream(daemon_output));
        tokio::task::spawn_local(connection);
        for n in 0..100 {
            daemon.event("tick", json!({ "n": n }));
        }
        let _asking = daemon.request("ask", json!({}), None);
        daemon.event("last", json!({}));

        let read = on_a_thread(move || {
            BufReader::new(host_input)
                .lines()
                .take(102)
                .collect::<Result<Vec<_>, _>>()
        });
        let lines = within_real(read)
            .await
            .expect("the reading thread")
            .expect("lines of text");
        let frames: Vec<serde_json::Value> = lines
            .iter()
            .map(|line| serde_json::from_str(line).expect("a frame"))
            .collect();
        for (at, frame) in frames.iter().enumerate() {
            assert_eq!(frame["id"], format!("n-{}", at + 1));
        }
        assert_eq!(frames[41]["body"], json!({ "n": 41 }));
        assert_eq!(frames[100]["op"], "ask");
        assert_eq!(frames[101]["op"], "last");
        drop(host_output);
    });
}

#[test]
fn an_exit_the_host_writes_before_its_answer_is_known_when_the_answer_is() {
    run_real(async {
        let mut host = host_builder();
        // The host's handler tells of the exit, then answers the open: two
        // frames, in that order, on the same pipe.
        host.on("pane.open", |bridge, _| {
            bridge
                .event("pane.exit", json!({ "id": "p1", "generation": 1 }))
                .map_err(|error| error.to_string())?;
            Ok(json!({ "ok": true }))
        });
        let (host, daemon) = over_pipes(host, daemon_builder());
        let exited = Rc::new(Cell::new(false));
        let rest_ran = Rc::new(Cell::new(false));
        let (release, gate) = oneshot::channel::<()>();
        let gate = RefCell::new(Some(gate));
        let (exit_flag, rest_flag) = (Rc::clone(&exited), Rc::clone(&rest_ran));
        daemon.on_event("pane.exit", move |_| {
            exit_flag.set(true);
            let (gate, rest_flag) = (gate.borrow_mut().take(), Rc::clone(&rest_flag));
            tokio::task::spawn_local(async move {
                if let Some(gate) = gate {
                    let _ = gate.await;
                }
                rest_flag.set(true);
            });
        });
        assert_eq!(
            within_real(daemon.request("pane.open", json!({ "id": "p1" }), Some(PATIENCE))).await,
            Ok(json!({ "ok": true }))
        );
        assert!(exited.get(), "the exit was handled before the answer");
        assert!(!rest_ran.get(), "and the work it started goes on apart");
        release.send(()).expect("the rest waits for its gate");
        wait_for_real(|| rest_ran.get()).await;
        host.close_input();
    });
}

#[test]
fn the_hosts_end_of_input_refuses_what_the_daemon_waits_for_with_eof() {
    run_real(async {
        let mut host = host_builder();
        let arrived = Arc::new(AtomicBool::new(false));
        let (release, held) = mpsc::channel::<()>();
        let held = Mutex::new(held);
        let flag = Arc::clone(&arrived);
        host.on("hangs", move |_, _| {
            flag.store(true, Ordering::SeqCst);
            let _ = held.lock().expect("held").recv();
            Ok(json!({}))
        });
        let (host, daemon) = over_pipes(host, daemon_builder());
        let waiting = daemon.request("hangs", json!({}), None);
        wait_for_real(|| arrived.load(Ordering::SeqCst)).await;
        // The host ends what the daemon reads, as the app does when it drops
        // the daemon's stdin.
        host.close_input();
        assert_eq!(within_real(waiting).await, Err(BridgeError::Eof));
        assert!(daemon.closed());
        assert_eq!(
            within_real(daemon.request("late", json!({}), None)).await,
            Err(BridgeError::Eof)
        );
        drop(release);
    });
}

#[test]
fn the_daemons_end_of_input_is_seen_by_the_host_as_the_end_of_the_bridge() {
    run_real(async {
        let host_closed = Arc::new(AtomicBool::new(false));
        let mut host = host_builder();
        let flag = Arc::clone(&host_closed);
        host.on_close(move || flag.store(true, Ordering::SeqCst));
        let (host, daemon) = over_pipes(host, daemon_builder());
        let (stay, never) = oneshot::channel::<()>();
        let never = RefCell::new(Some(never));
        let arrived = Rc::new(Cell::new(false));
        let flag = Rc::clone(&arrived);
        daemon.on("hangs", move |_, _| {
            flag.set(true);
            let never = never.borrow_mut().take();
            async move {
                if let Some(never) = never {
                    let _ = never.await;
                }
                Ok(json!({}))
            }
        });
        let waiting = on_a_thread({
            let host = host.clone();
            move || host.request("hangs", json!({}), Some(20_000))
        });
        wait_for_real(|| arrived.get()).await;
        // The daemon's end of the bridge closes, and with it the output the
        // host reads.
        daemon.close();
        assert_eq!(
            within_real(waiting).await.expect("the host's thread"),
            Err(BridgeError::Eof)
        );
        wait_for_real(|| host_closed.load(Ordering::SeqCst)).await;
        assert!(host.is_closed());
        drop(stay);
    });
}

#[test]
fn a_frame_over_the_daemons_limit_is_answered_too_large_across_the_pipe() {
    run_real(async {
        let (host, daemon) = over_pipes(host_builder(), daemon_builder().max_frame_bytes(128));
        daemon.on("big", |_, _| async { Ok(json!({ "ok": true })) });
        let asked = on_a_thread({
            let host = host.clone();
            move || host.request("big", json!({ "text": "x".repeat(100) }), Some(5_000))
        });
        assert_eq!(
            within_real(asked).await.expect("the host's thread"),
            Ok(json!({ "ok": false, "error": "too-large" }))
        );
        let small = on_a_thread({
            let host = host.clone();
            move || host.request("big", json!({}), Some(5_000))
        });
        assert_eq!(
            within_real(small).await.expect("the host's thread"),
            Ok(json!({ "ok": true })),
            "the bridge carries on"
        );
        host.close_input();
    });
}

#[test]
fn the_hosts_end_of_input_is_told_to_the_daemon_as_its_input_ended_over_real_pipes() {
    run_real(async {
        let (host, daemon) = over_pipes(host_builder(), daemon_builder());
        let heard = tokio::task::spawn_local(daemon.ended());
        // Nothing has ended while the host still holds its end.
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(!heard.is_finished());
        host.close_input();
        assert_eq!(within_real(heard).await.expect("the task"), Ended::Input);
        assert!(daemon.closed());
        // Told to whoever asks after it, too: the daemon stops on it once.
        assert_eq!(within_real(daemon.ended()).await, Ended::Input);
    });
}

#[test]
fn an_output_nobody_reads_any_more_is_told_as_a_failure_while_the_input_stays_open() {
    run_real(async {
        let (daemon_input, host_output) = pipe().expect("the host's output pipe");
        let (host_input, daemon_output) = pipe().expect("the daemon's output pipe");
        let (daemon, connection) =
            daemon_builder().connect(stream(daemon_input), stream(daemon_output));
        tokio::task::spawn_local(connection);
        // The app's end of the daemon's stdout is gone; its stdin stays open.
        drop(host_input);
        assert!(daemon.event("late", json!({})), "queued, and written later");
        let ended = within_real(daemon.ended()).await;
        let Ended::Failed(BridgeError::Io(words)) = ended else {
            panic!("not a failed transport: {ended:?}");
        };
        assert!(!words.is_empty());
        assert!(daemon.closed());
        assert!(
            !daemon.event("later", json!({})),
            "nothing more is queued once it failed"
        );
        drop(host_output);
    });
}

//! What the app runs: the window's runtime, its panes and its daemon started
//! together and stopped in order, over the pane handlers and the same drain
//! the headless helper runs.

use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;
use tauri::{AppHandle, Emitter};

use crate::daemon::{
    connect_daemon, stop_daemon, Daemon, DaemonStarter, DaemonStatus, DAEMON_READY_TIMEOUT,
    DAEMON_RESTART, DAEMON_STATUS_EVENT,
};
use crate::daemon_command::daemon_command;
use cf_bridge::BridgeBuilder;
use cf_panes::arbiter::InputArbiter;
use cf_panes::headless::{reap_all, ENTER, MAX_FRAME_BYTES};
use cf_panes::input_queue::InputQueue;
use cf_panes::output_hub::OutputHub;
use cf_panes::pane_handlers::register_pane_handlers;
use cf_panes::pty::{PaneTable, CLOSE_GRACE};
use cf_proto::bridge::Role;

/// How long quitting, or installing an update, waits for what was admitted
/// before the daemon stopped to finish.
const SHUTDOWN_DRAIN: Duration = Duration::from_secs(5);
/// How long quitting waits for every window's programs, asked to end, to have:
/// they are given [`CLOSE_GRACE`] before what is left is killed, and a moment
/// more to be. (On Windows nothing is asked and nothing waits: they are ended
/// at once.)
const SHUTDOWN_REAP: Duration = CLOSE_GRACE.saturating_add(Duration::from_secs(5));
/// The page-side name of the daemon's `state.changed`. No dot: Tauri rejects it.
pub(crate) const PAGE_STATE_EVENT: &str = "state-changed";

type PageEventSink = Arc<dyn Fn(&str, Value) + Send + Sync>;

pub struct AppRuntime {
    pub(crate) panes: Arc<PaneTable>,
    pub(crate) daemon: Arc<Daemon>,
    pub(crate) output: Arc<OutputHub>,
    pub(crate) inputs: Arc<InputQueue>,
}

impl AppRuntime {
    pub fn start(app: &AppHandle) -> Self {
        let panes = Arc::new(PaneTable::new());
        let output = Arc::new(OutputHub::new());
        let arbiter = Arc::new(InputArbiter::new(ENTER));
        let inputs = Arc::new(InputQueue::new(Arc::clone(&panes), Arc::clone(&arbiter)));

        let reporter = app.clone();
        let daemon = Daemon::new(Arc::new(move |status: &DaemonStatus| {
            if let Err(error) = reporter.emit(DAEMON_STATUS_EVENT, status) {
                eprintln!("consensflow page event {DAEMON_STATUS_EVENT}: {error}");
            }
        }));
        let starter: DaemonStarter = {
            let app = app.clone();
            let panes = Arc::clone(&panes);
            let output = Arc::clone(&output);
            let inputs = Arc::clone(&inputs);
            let login = daemon.login();
            Arc::new(move |closed| {
                let command = daemon_command(&app, &login)?;
                let mut builder = BridgeBuilder::new(Role::Host, MAX_FRAME_BYTES);
                let page_app = app.clone();
                let page_events: PageEventSink = Arc::new(move |name, body| {
                    if let Err(error) = page_app.emit(name, body) {
                        eprintln!("consensflow page event {name}: {error}");
                    }
                });
                register_page_events(&mut builder, page_events);
                register_pane_handlers(
                    &mut builder,
                    Arc::clone(&panes),
                    Arc::clone(&arbiter),
                    Arc::clone(&output),
                    Arc::clone(&inputs),
                );
                builder.on_error(|error| eprintln!("consensflow bridge: {error}"));
                builder.on_close(closed);
                connect_daemon(command, builder, DAEMON_READY_TIMEOUT)
            })
        };
        daemon.start(starter, Arc::clone(&panes), DAEMON_RESTART);
        Self {
            panes,
            daemon,
            output,
            inputs,
        }
    }

    pub fn shutdown(&self) {
        if self.begin_shutdown() && !self.finish_shutdown() {
            eprintln!("consensflow: quitting with the drain unfinished after its deadline");
        }
    }

    pub(crate) fn begin_shutdown(&self) -> bool {
        let Some((daemon, bridge)) = self.daemon.stop() else {
            return false;
        };
        if let Some(mut daemon) = daemon {
            if let Some(bridge) = &bridge {
                bridge.close_input();
            }
            stop_daemon(&mut daemon);
        }
        true
    }

    /// Waits for what was admitted before the stop (launches, pane input, the
    /// bridge's last handlers) and reaps every pane, for `SHUTDOWN_DRAIN` at
    /// most besides the time the panes take: quitting had no limit, and a
    /// launch that never finished kept the app from quitting. The reap, which
    /// asks each window's programs to end and gives them their grace (on Windows
    /// it ends them at once), has a time of its own (`SHUTDOWN_REAP`) that a
    /// launch that never finished does not spend. `false` when the drain did not finish in time; the quit or
    /// the update's restart goes on without it.
    pub(crate) fn finish_shutdown(&self) -> bool {
        let bridge = self.daemon.bridge();
        let admitted = bridge.clone();
        let waited = Instant::now();
        let launches = finish_before_deadline(SHUTDOWN_DRAIN, move || {
            if let Some(bridge) = &admitted {
                let _ = bridge.wait_launches_closed();
            }
        });
        let spent = waited.elapsed();
        let panes = Arc::clone(&self.panes);
        let reaped = finish_before_deadline(SHUTDOWN_REAP, move || reap_all(&panes));
        let inputs = Arc::clone(&self.inputs);
        let drained = finish_before_deadline(SHUTDOWN_DRAIN.saturating_sub(spent), move || {
            inputs.close_and_drain();
            if let Some(bridge) = &bridge {
                let _ = bridge.wait_closed();
            }
        });
        launches && reaped && drained
    }
}

impl Drop for AppRuntime {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Runs `finish` on a thread of its own and waits for it until `timeout`:
/// `false` when it failed or was not done by then.
fn finish_before_deadline(timeout: Duration, finish: impl FnOnce() + Send + 'static) -> bool {
    let (sent, done) = mpsc::sync_channel(1);
    if thread::Builder::new()
        .name("consensflow-drain".into())
        .spawn(move || {
            finish();
            let _ = sent.send(());
        })
        .is_err()
    {
        return false;
    }
    done.recv_timeout(timeout).is_ok()
}

/// The daemon's `state.changed` becomes the page's `state-changed`.
///
/// The two names are not the same namespace and cannot be. Tauri 2 accepts
/// only alphanumerics, `-`, `/`, `:` and `_` in an event name, so the dotted
/// bridge name is REFUSED on the page side — `listen` rejects, and the
/// rejection took the page's whole start-up with it. The bridge keeps its
/// name; only the hop into the webview is renamed.
fn register_page_events(builder: &mut BridgeBuilder, sink: PageEventSink) {
    builder.on_event("state.changed", move |body| sink(PAGE_STATE_EVENT, body));
}

/// A runtime around what a test stands up: its panes and input queue, and
/// a daemon and its bridge when the test has them. The tests that stand
/// one up run real shells, so they are Unix's.
#[cfg(all(test, unix))]
pub(crate) fn test_runtime(
    panes: Arc<PaneTable>,
    inputs: Arc<InputQueue>,
    daemon: Option<std::process::Child>,
    bridge: Option<cf_bridge::Bridge>,
) -> AppRuntime {
    AppRuntime {
        panes,
        daemon: Daemon::settled(daemon, bridge),
        output: Arc::new(OutputHub::new()),
        inputs,
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::Path;
    use std::process::{Command, Stdio};
    use std::time::Instant;

    use portable_pty::PtySize;
    use serde_json::json;

    use crate::daemon::DAEMON_STOP_GRACE;
    use cf_panes::arbiter::EnterTiming;
    use cf_panes::pty::{process_exists, PaneKey, Sentinel};

    #[test]
    fn node_state_changed_event_is_forwarded_to_the_page_sink() {
        use std::io::Write;
        use std::os::unix::net::UnixStream;

        let (rust_stream, mut node_stream) = UnixStream::pair().expect("bridge socket pair");
        node_stream
            .write_all(b"{\"url\":\"http://localhost:1/\",\"token\":\"test\"}\n")
            .expect("write bridge handle");
        node_stream.flush().expect("flush bridge handle");
        let (events, received) = mpsc::channel();
        let sink: PageEventSink = Arc::new(move |name, body| {
            events
                .send((name.to_string(), body))
                .expect("record page event");
        });
        let mut builder = BridgeBuilder::new(Role::Host, 1024);
        register_page_events(&mut builder, sink);
        let connected = builder
            .connect(
                rust_stream.try_clone().expect("clone bridge socket"),
                rust_stream,
            )
            .expect("connect bridge");

        node_stream
            .write_all(
                b"{\"v\":1,\"id\":\"n-state\",\"kind\":\"evt\",\"op\":\"state.changed\",\"body\":{\"reason\":\"pane.open\"}}\n",
            )
            .expect("write state event");
        node_stream.flush().expect("flush state event");

        assert_eq!(
            received
                .recv_timeout(Duration::from_secs(1))
                .expect("page event"),
            ("state-changed".to_string(), json!({"reason":"pane.open"}))
        );
        drop(node_stream);
        connected.bridge.wait_closed().expect("bridge closes");
    }

    #[test]
    fn gui_shutdown_drains_an_admitted_launch_before_reaping_panes() {
        use std::io::Write;
        use std::os::unix::net::UnixStream;
        use std::sync::Barrier;

        let _pty_guard = cf_panes::pty::serial_pty_test();
        let panes = Arc::new(PaneTable::new());
        let arbiter = Arc::new(InputArbiter::new(EnterTiming::fixed(0)));
        let inputs = Arc::new(InputQueue::new(Arc::clone(&panes), arbiter));
        let release = Arc::new(Barrier::new(2));
        let handler_release = Arc::clone(&release);
        let handler_panes = Arc::clone(&panes);
        let (admitted_sender, admitted_receiver) = mpsc::channel();
        let mut builder = BridgeBuilder::new(Role::Host, 1024);
        builder.on_launch("pane.open", move |_bridge, _body| {
            admitted_sender.send(()).expect("announce admitted launch");
            handler_release.wait();
            let key = PaneKey::new("late-gui-pane", 1);
            let _reader = handler_panes
                .open_at(
                    key,
                    Path::new("/tmp"),
                    &[
                        "/bin/sh".to_string(),
                        "-c".to_string(),
                        "sleep 30".to_string(),
                    ],
                    &HashMap::new(),
                    PtySize {
                        rows: 24,
                        cols: 80,
                        pixel_width: 0,
                        pixel_height: 0,
                    },
                )
                .map_err(|error| error.to_string())?;
            Ok(json!({"ok":true}))
        });

        let (rust_stream, mut node_stream) = UnixStream::pair().expect("bridge socket pair");
        node_stream
            .write_all(b"{\"url\":\"http://localhost:1/\",\"token\":\"test\"}\n")
            .expect("write bridge handle");
        node_stream.flush().expect("flush bridge handle");
        let connected = builder
            .connect(
                rust_stream.try_clone().expect("clone bridge socket"),
                rust_stream,
            )
            .expect("connect bridge");
        let runtime = Arc::new(test_runtime(
            Arc::clone(&panes),
            inputs,
            None,
            Some(connected.bridge),
        ));
        node_stream
            .write_all(
                b"{\"v\":1,\"id\":\"n-open\",\"kind\":\"req\",\"op\":\"pane.open\",\"body\":{}}\n",
            )
            .expect("write pane.open");
        node_stream.flush().expect("flush pane.open");
        admitted_receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("launch admitted");
        drop(node_stream);

        let shutdown_runtime = Arc::clone(&runtime);
        let (shutdown_sender, shutdown_receiver) = mpsc::channel();
        let shutdown = thread::spawn(move || {
            shutdown_runtime.shutdown();
            shutdown_sender.send(()).expect("announce shutdown");
        });
        let returned_before_launch = shutdown_receiver
            .recv_timeout(Duration::from_millis(100))
            .is_ok();
        release.wait();
        if !returned_before_launch {
            shutdown_receiver
                .recv_timeout(Duration::from_secs(2))
                .expect("shutdown after launch");
        }
        shutdown.join().expect("shutdown thread");

        assert!(
            !returned_before_launch,
            "GUI shutdown returned before its admitted launch finished"
        );
        assert!(panes.list().expect("pane list after shutdown").is_empty());
    }

    #[test]
    fn a_stalled_drain_is_left_behind_at_its_deadline() {
        use std::io::Read;
        use std::os::unix::net::UnixStream;
        let (mut reader, writer) = UnixStream::pair().unwrap();
        let started = Instant::now();
        assert!(!finish_before_deadline(
            Duration::from_millis(30),
            move || {
                let _ = reader.read(&mut [0_u8; 1]);
            }
        ));
        assert!(started.elapsed() < Duration::from_secs(1));
        drop(writer);
        assert!(finish_before_deadline(Duration::from_secs(1), || {}));
        assert!(!finish_before_deadline(Duration::from_secs(1), || panic!(
            "drain failed"
        )));
    }

    /// Quitting closes each window the way a terminal does: its programs are
    /// asked to end and given their grace, which is longer than the drain waits
    /// on a launch, and what ignores the request is killed once it is over.
    #[test]
    fn quitting_gives_the_windows_programs_their_grace_before_it_kills_them() {
        use std::io::Read;

        let _pty_guard = cf_panes::pty::serial_pty_test();
        let mut outsider = Sentinel::start();
        // Longer than the drain, shorter than a real grace.
        let grace = SHUTDOWN_DRAIN + Duration::from_secs(1);
        let panes = Arc::new(PaneTable::closing_within(grace));
        let arbiter = Arc::new(InputArbiter::new(EnterTiming::fixed(0)));
        let inputs = Arc::new(InputQueue::new(Arc::clone(&panes), arbiter));
        let mut reader = panes
            .open_at(
                PaneKey::new("deaf-pane", 1),
                Path::new("/tmp"),
                &[
                    "/bin/sh".to_string(),
                    "-c".to_string(),
                    cf_base::fixture::deaf("TERM HUP", "printf ready"),
                ],
                &HashMap::new(),
                PtySize {
                    rows: 24,
                    cols: 80,
                    pixel_width: 0,
                    pixel_height: 0,
                },
            )
            .expect("open a pane whose program ignores the request to end");
        let mut ready = [0_u8; 5];
        reader.read_exact(&mut ready).expect("its program is up");
        let runtime = test_runtime(Arc::clone(&panes), inputs, None, None);

        let started = Instant::now();
        runtime.shutdown();

        assert!(
            started.elapsed() >= grace,
            "its program was given its grace: {:?}",
            started.elapsed()
        );
        assert!(panes.list().expect("pane list after shutdown").is_empty());
        assert!(outsider.runs(), "a process outside the window");
    }

    /// Quitting waits for the drain no longer than installing an update does:
    /// a launch that never finishes cannot keep the app from quitting.
    #[test]
    fn quitting_waits_for_the_drain_no_longer_than_an_update_does() {
        use std::io::Write;
        use std::os::unix::net::UnixStream;
        use std::sync::Barrier;

        let release = Arc::new(Barrier::new(2));
        let handler_release = Arc::clone(&release);
        let (admitted_sender, admitted_receiver) = mpsc::channel();
        let mut builder = BridgeBuilder::new(Role::Host, 1024);
        builder.on_launch("pane.open", move |_bridge, _body| {
            admitted_sender.send(()).expect("announce admitted launch");
            handler_release.wait();
            Ok(json!({"ok":true}))
        });
        let (rust_stream, mut node_stream) = UnixStream::pair().expect("bridge socket pair");
        node_stream
            .write_all(b"{\"url\":\"http://localhost:1/\",\"token\":\"test\"}\n")
            .expect("write bridge handle");
        let connected = builder
            .connect(rust_stream.try_clone().expect("clone socket"), rust_stream)
            .expect("connect bridge");
        node_stream
            .write_all(
                b"{\"v\":1,\"id\":\"n-open\",\"kind\":\"req\",\"op\":\"pane.open\",\"body\":{}}\n",
            )
            .expect("write pane.open");
        admitted_receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("launch admitted");
        drop(node_stream);

        let panes = Arc::new(PaneTable::new());
        let arbiter = Arc::new(InputArbiter::new(EnterTiming::fixed(0)));
        let inputs = Arc::new(InputQueue::new(Arc::clone(&panes), arbiter));
        let runtime = Arc::new(test_runtime(panes, inputs, None, Some(connected.bridge)));
        let quitting = Arc::clone(&runtime);
        let (quit, quitted) = mpsc::channel();
        let started = Instant::now();
        thread::spawn(move || {
            quitting.shutdown();
            let _ = quit.send(started.elapsed());
        });
        let took = quitted.recv_timeout(Duration::from_secs(8));
        release.wait();
        let took = took.expect("the quit waited on a launch that never finished");
        assert!(took >= Duration::from_secs(5), "{took:?}");
    }

    /// The case the daemon-absent test above could not reach: a REAL daemon
    /// child, its own pipes carrying the bridge, and a `pane.open` admitted
    /// and still spawning when the app is told to quit.
    ///
    /// This is the ordering proof. `Bridge::admit_handler` refuses every new
    /// handler once the transport is `closed`, and only EOF from the peer
    /// closes it — so ending the daemon (its input closed, and the kill for
    /// one that does not stop on that) IS the act that shuts admission, and
    /// nothing else in `shutdown()` can do it. What follows is a drain of what
    /// was ALREADY admitted, and only then the reap, so a pane whose spawn
    /// was in flight is in the table before anything reaps it.
    #[test]
    fn gui_shutdown_kills_a_present_daemon_then_drains_its_admitted_launch() {
        use std::sync::Barrier;

        let _pty_guard = cf_panes::pty::serial_pty_test();
        let panes = Arc::new(PaneTable::new());
        let arbiter = Arc::new(InputArbiter::new(EnterTiming::fixed(0)));
        let inputs = Arc::new(InputQueue::new(Arc::clone(&panes), arbiter));
        let release = Arc::new(Barrier::new(2));
        let handler_release = Arc::clone(&release);
        let handler_panes = Arc::clone(&panes);
        let (admitted_sender, admitted_receiver) = mpsc::channel();
        let mut builder = BridgeBuilder::new(Role::Host, MAX_FRAME_BYTES);
        builder.on_launch("pane.open", move |_bridge, _body| {
            admitted_sender.send(()).expect("announce admitted launch");
            handler_release.wait();
            let key = PaneKey::new("daemon-present-pane", 1);
            let _reader = handler_panes
                .open_at(
                    key,
                    Path::new("/tmp"),
                    &[
                        "/bin/sh".to_string(),
                        "-c".to_string(),
                        "sleep 30".to_string(),
                    ],
                    &HashMap::new(),
                    PtySize {
                        rows: 24,
                        cols: 80,
                        pixel_width: 0,
                        pixel_height: 0,
                    },
                )
                .map_err(|error| error.to_string())?;
            Ok(json!({"ok":true}))
        });

        // Stands in for `cf ui --json`: the handshake line, one launch request,
        // then a process that holds both pipes open until something kills it.
        let mut daemon = Command::new("/bin/sh")
            .arg("-c")
            .arg(concat!(
                r#"printf '%s\n' '{"url":"http://localhost:1/","token":"test"}'; "#,
                r#"printf '%s\n' '{"v":1,"id":"n-open","kind":"req","op":"pane.open","body":{}}'; "#,
                "exec sleep 60",
            ))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("spawn the stand-in daemon");
        let daemon_pid = daemon.id() as i32;
        let reader = daemon.stdout.take().expect("daemon stdout");
        let writer = daemon.stdin.take().expect("daemon stdin");
        let connected = builder.connect(reader, writer).expect("connect bridge");

        let runtime = Arc::new(test_runtime(
            Arc::clone(&panes),
            inputs,
            Some(daemon),
            Some(connected.bridge),
        ));
        admitted_receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("launch admitted over the real daemon pipe");

        let shutdown_runtime = Arc::clone(&runtime);
        let (shutdown_sender, shutdown_receiver) = mpsc::channel();
        let shutdown = thread::spawn(move || {
            shutdown_runtime.shutdown();
            shutdown_sender.send(()).expect("announce shutdown");
        });
        let returned_before_launch = shutdown_receiver
            .recv_timeout(Duration::from_millis(250))
            .is_ok();
        release.wait();
        if !returned_before_launch {
            shutdown_receiver
                .recv_timeout(Duration::from_secs(5))
                .expect("shutdown returns once the admitted launch has finished");
        }
        shutdown.join().expect("shutdown thread");

        assert!(
            !returned_before_launch,
            "shutdown returned before its admitted launch finished"
        );
        assert!(panes.list().expect("pane list after shutdown").is_empty());
        assert!(
            !process_exists(daemon_pid),
            "the daemon child outlived shutdown"
        );
    }

    /// The daemon is asked before it is killed: one that stops when its
    /// input ends gets to write its last lines, and one that ignores it is
    /// killed once the grace is over.
    #[test]
    fn gui_shutdown_asks_the_daemon_first_and_kills_only_what_stays() {
        let mark = std::env::temp_dir().join(format!("consensflow-stop-{}", std::process::id()));
        let _ = std::fs::remove_file(&mark);
        let runtime_for = |script: String| {
            use std::io::{BufRead, BufReader};
            let mut daemon = Command::new("/bin/sh")
                .arg("-c")
                .arg(script)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .expect("spawn the stand-in daemon");
            let pid = daemon.id() as i32;
            let mut ready = String::new();
            BufReader::new(daemon.stdout.take().expect("daemon stdout"))
                .read_line(&mut ready)
                .expect("the stand-in says it is ready");
            assert_eq!(ready, "ready\n");
            let arbiter = Arc::new(InputArbiter::new(EnterTiming::fixed(0)));
            let panes = Arc::new(PaneTable::new());
            let runtime = test_runtime(
                Arc::clone(&panes),
                Arc::new(InputQueue::new(panes, arbiter)),
                Some(daemon),
                None,
            );
            (runtime, pid)
        };

        let (polite, polite_pid) = runtime_for(format!(
            "echo ready; cat >/dev/null; echo asked > {}; exit 0",
            mark.display()
        ));
        let started = Instant::now();
        polite.shutdown();
        assert!(started.elapsed() < DAEMON_STOP_GRACE, "it went on its own");
        assert_eq!(
            std::fs::read_to_string(&mark).expect("the daemon wrote its last line"),
            "asked\n"
        );
        assert!(!process_exists(polite_pid));
        let _ = std::fs::remove_file(&mark);

        let (deaf, deaf_pid) = runtime_for("echo ready; exec sleep 60".to_string());
        let started = Instant::now();
        deaf.shutdown();
        assert!(started.elapsed() >= DAEMON_STOP_GRACE, "it had its grace");
        assert!(!process_exists(deaf_pid), "and was killed after it");
    }

    /// Quitting while a start asks the login shell for the daemon's PATH ends
    /// the shell at once: one that never answered outlived the app. Only the
    /// shell: what its login files started is the user's, an `ssh-agent -D`
    /// other programs use, say (the reviews of 2026-10-16).
    #[test]
    fn quitting_ends_a_login_shell_a_start_is_still_asking_and_leaves_what_it_started() {
        use std::os::unix::fs::PermissionsExt;

        let home = tempfile::tempdir().expect("home");
        let pids = home.path().join("pids");
        let shell = home.path().join("login-shell");
        std::fs::write(
            &shell,
            format!(
                "#!/bin/sh\n/bin/sleep 60 &\necho $$ $! > '{}'\nwait\n",
                pids.display()
            ),
        )
        .expect("write the shell");
        std::fs::set_permissions(&shell, std::fs::Permissions::from_mode(0o755))
            .expect("make the shell executable");
        let panes = Arc::new(PaneTable::new());
        let arbiter = Arc::new(InputArbiter::new(EnterTiming::fixed(0)));
        let inputs = Arc::new(InputQueue::new(Arc::clone(&panes), arbiter));
        let runtime = test_runtime(panes, inputs, None, None);
        let login = runtime.daemon.login();
        let (sent, asked) = mpsc::channel();
        thread::spawn(move || {
            let _ = sent.send(crate::daemon_command::login_path_in(
                &shell,
                Duration::from_secs(60),
                &login,
            ));
        });
        let started = Instant::now();
        let (shell, agent) = loop {
            let said = std::fs::read_to_string(&pids).unwrap_or_default();
            let read: Vec<i32> = said
                .split_whitespace()
                .filter_map(|pid| pid.parse().ok())
                .collect();
            if let [shell, agent] = read[..] {
                break (shell, agent);
            }
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "the shell never ran"
            );
            thread::sleep(Duration::from_millis(20));
        };
        runtime.shutdown();
        let ended = asked.recv_timeout(Duration::from_secs(5));
        // Given a moment to be ended, as it would have been with the shell.
        thread::sleep(Duration::from_millis(200));
        let left = process_exists(agent);
        let _ = Command::new("/bin/kill")
            .args(["-9", &agent.to_string()])
            .status();
        assert_eq!(ended.expect("the lookup ended with the quit"), None);
        assert!(!process_exists(shell), "the shell outlived the quit");
        assert!(left, "what the login file started was ended with the shell");
    }

    /// Why the daemon is killed FIRST, stated as a test rather than a comment.
    ///
    /// `wait_launches_closed` waits for `closed` AND an empty launch count,
    /// and only the peer's EOF sets `closed`. Draining before the kill would
    /// therefore wait on a peer that is still writing — every app exit would
    /// hang. This is the shape a reordered `shutdown()` would take.
    #[test]
    fn draining_launches_before_the_daemon_closes_never_returns() {
        use std::io::Write;
        use std::os::unix::net::UnixStream;

        let mut builder = BridgeBuilder::new(Role::Host, 1024);
        builder.on("noop", |_bridge, _body| Ok(json!({"ok":true})));
        let (rust_stream, mut node_stream) = UnixStream::pair().expect("bridge socket pair");
        node_stream
            .write_all(b"{\"url\":\"http://localhost:1/\",\"token\":\"test\"}\n")
            .expect("write bridge handle");
        node_stream.flush().expect("flush bridge handle");
        let connected = builder
            .connect(
                rust_stream.try_clone().expect("clone bridge socket"),
                rust_stream,
            )
            .expect("connect bridge");

        let waiting = connected.bridge.clone();
        let (done_sender, done_receiver) = mpsc::channel();
        let wait = thread::spawn(move || {
            let outcome = waiting.wait_launches_closed();
            let _ = done_sender.send(outcome.is_ok());
        });
        assert!(
            done_receiver
                .recv_timeout(Duration::from_millis(300))
                .is_err(),
            "wait_launches_closed returned while the daemon peer was still open"
        );

        drop(node_stream);
        assert!(
            done_receiver
                .recv_timeout(Duration::from_secs(5))
                .expect("wait_launches_closed returns once the peer closes"),
            "wait_launches_closed failed after the peer closed"
        );
        wait.join().expect("wait thread");
    }
}

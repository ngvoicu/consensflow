//! The daemon started in process, as the app starts it: over its bridge with
//! the test as the app's end, on a home of its own. What it says first, what
//! its start does and in what order, how each way of stopping it ends its log,
//! and that a restart brings back what was open, opening the window with its
//! environment. (The same as a process is `crates/cf/tests/daemon_stop.rs`
//! and the black-box cases of `tests/core-daemon.test.mjs`.)

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::time::Instant;

use cf_bridge::local::{Bridge, BridgeBuilder};
use cf_harness::contract::LaunchId;
use cf_harness::testing::fake_window_executable;
use cf_ledger::{open_ledger, Options as LedgerOptions};
use cf_proto::bridge::Role;
use serde_json::{json, Value};
use tokio::io::{duplex, AsyncWriteExt};
use tokio::sync::Notify;
use tokio::task::LocalSet;

use super::*;
use crate::testing::Said;

mod restart;
mod screens;

const UI_TOKEN_DIGITS: usize = 48;

/// A daemon running over a home, and the app's end of its bridge.
struct Rig {
    daemon: Daemon,
    app: Bridge,
    exited: Rc<RefCell<Vec<i32>>>,
    said: Said,
    handles: Rc<RefCell<Vec<HandleLine>>>,
    home: PathBuf,
}

/// The environment the app gives a daemon on `root`: the home inside it, a
/// `claude` to be found on PATH and only it, and on Windows what finds and
/// starts a `.cmd` there (`PATHEXT`, `SystemRoot`, `ComSpec`).
fn environment(root: &Path) -> Env {
    let home = root.join("home");
    let bin = root.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    fake_window_executable(&bin.join("claude"));
    let process = Env::from_process();
    let windows = ["SystemRoot", "ComSpec", "PATHEXT"]
        .into_iter()
        .filter(|_| cfg!(windows))
        .filter_map(|name| Some((name.to_owned(), process.text(name)?.to_owned())));
    Env::from_vars(
        [
            (
                "CONSENSFLOW_HOME",
                root.join("consensflow").to_string_lossy().into_owned(),
            ),
            ("HOME", home.to_string_lossy().into_owned()),
            ("USERPROFILE", home.to_string_lossy().into_owned()),
            (
                "CLAUDE_CONFIG_DIR",
                home.join(".claude").to_string_lossy().into_owned(),
            ),
            (
                "CODEX_HOME",
                home.join(".codex").to_string_lossy().into_owned(),
            ),
            (
                "XDG_CONFIG_HOME",
                home.join(".config").to_string_lossy().into_owned(),
            ),
            ("PATH", bin.to_string_lossy().into_owned()),
            ("CONSENSFLOW_NODE", "/the/node/the/app/named".to_owned()),
        ]
        .into_iter()
        .map(|(name, value)| (name.to_owned(), value))
        .chain(windows),
    )
}

fn options(
    root: &Path,
    said: &Said,
    exited: &Rc<RefCell<Vec<i32>>>,
    handles: &Rc<RefCell<Vec<HandleLine>>>,
) -> (Options, Bridge) {
    let (daemon_input, app_output) = duplex(256 * 1024);
    let (app_input, daemon_output) = duplex(256 * 1024);
    let (app, connection) = BridgeBuilder::new(Role::Host).connect(app_input, app_output);
    drop(tokio::task::spawn_local(connection));
    let (noted, told) = (Rc::clone(exited), Rc::clone(handles));
    let options = Options {
        input: Box::new(daemon_input),
        output: Box::new(daemon_output),
        stderr: Box::new(said.clone()),
        on_out: Box::new(move |handle| {
            told.borrow_mut().push(handle.clone());
            Ok(())
        }),
        exit: Rc::new(move |code| noted.borrow_mut().push(code)),
        bundle: machine::bundle_of(&root.join("bundle").join("bin").join("cf")),
        signals: false,
    };
    (options, app)
}

async fn started(root: &Path) -> Rig {
    let (said, exited, handles) = (Said::default(), Rc::default(), Rc::default());
    let (options, app) = options(root, &said, &exited, &handles);
    let daemon = start(environment(root), options).await.expect("it starts");
    let home = daemon.parts.home.clone();
    Rig {
        daemon,
        app,
        exited,
        said,
        handles,
        home,
    }
}

async fn ask(app: &Bridge, op: &str, body: Value) -> Value {
    tokio::time::timeout(
        Duration::from_secs(10),
        app.request(op, body, Some(Duration::from_secs(10))),
    )
    .await
    .expect("answered")
    .expect("the bridge is up")
}

fn log_lines(home: &Path) -> Vec<String> {
    std::fs::read_to_string(home.join("daemon.log"))
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
}

/// A log line without its time.
fn said_of(line: &str) -> &str {
    line.split_once(' ').map_or(line, |(_, rest)| rest)
}

async fn get(url: &str, path: &str, token: Option<&str>) -> (u16, Value) {
    use tokio::io::AsyncReadExt;
    let address = url
        .strip_prefix("http://")
        .unwrap()
        .trim_end_matches('/')
        .to_owned();
    let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
    let auth = token.map_or_else(String::new, |token| {
        format!("Authorization: Bearer {token}\r\n")
    });
    stream
        .write_all(
            format!("GET {path} HTTP/1.1\r\nHost: t\r\n{auth}Connection: close\r\n\r\n").as_bytes(),
        )
        .await
        .unwrap();
    let mut answered = Vec::new();
    stream.read_to_end(&mut answered).await.unwrap();
    let text = String::from_utf8_lossy(&answered).into_owned();
    let (head, body) = text.split_once("\r\n\r\n").unwrap();
    let status = head.split(' ').nth(1).unwrap().parse().unwrap();
    (status, serde_json::from_str(body).unwrap_or(Value::Null))
}

#[tokio::test]
async fn it_says_its_handle_line_logs_its_start_and_serves_its_api() {
    LocalSet::new()
        .run_until(async {
            let root = tempfile::tempdir().unwrap();
            let rig = started(root.path()).await;
            let handle = rig.handles.borrow()[0].clone();
            assert_eq!(rig.handles.borrow().len(), 1);
            assert_eq!(&handle, rig.daemon.handle());
            assert!(
                handle.url.starts_with("http://127.0.0.1:") && handle.url.ends_with('/'),
                "{}",
                handle.url
            );
            assert_eq!(handle.token.len(), UI_TOKEN_DIGITS);
            assert!(handle.token.bytes().all(|byte| byte.is_ascii_hexdigit()));

            let lines = log_lines(&rig.home);
            assert_eq!(lines.len(), 1, "{lines:?}");
            assert_eq!(
                said_of(&lines[0]),
                format!(
                    "info start pid {} rust {VERSION} home {}",
                    std::process::id(),
                    rig.home.display()
                )
            );
            // The API is listening: an agents' request with no token is 401.
            let (status, body) = get(&handle.url, "/api/whoami", None).await;
            assert_eq!(status, 401);
            assert_eq!(body["error"], "unauthorized");
            // The UI token opens none of the agents' routes.
            let (status, _) = get(&handle.url, "/api/whoami", Some(&handle.token)).await;
            assert_eq!(status, 401);
        })
        .await;
}

#[tokio::test]
async fn a_ping_over_the_bridge_is_answered_and_every_page_operation_is_there() {
    LocalSet::new()
        .run_until(async {
            let root = tempfile::tempdir().unwrap();
            let rig = started(root.path()).await;
            assert_eq!(
                ask(&rig.app, "ping", json!({})).await,
                json!({ "ok": true })
            );
            // The page's operations are served, on the daemon's own ledger and
            // saved agents: one that reads answers its fields, and one for a
            // project the ledger has not is refused in the ledger's words.
            assert_eq!(
                ask(&rig.app, "projects.list", json!({})).await,
                json!({ "ok": true, "projects": [] })
            );
            assert_eq!(
                ask(&rig.app, "board.get", json!({ "project": 1 })).await,
                json!({ "ok": false, "error": "no project 1" })
            );
            assert_eq!(
                ask(&rig.app, "no.such.operation", json!({})).await,
                json!({ "ok": false, "error": "unknown-op" })
            );
        })
        .await;
}

fn assert_log_ends_as_the_app_reads_it(home: &Path, reason: &str) {
    let lines = log_lines(home);
    assert_eq!(lines.len(), 3, "{lines:?}");
    assert!(said_of(&lines[0]).starts_with("info start pid "));
    let stop = said_of(&lines[1]);
    let size = stop
        .strip_prefix(&format!("info stop: {reason}; rss "))
        .unwrap_or_else(|| panic!("{stop}"));
    assert!(
        size.strip_suffix(" MB").unwrap().parse::<u64>().is_ok(),
        "{stop}"
    );
    assert_eq!(said_of(&lines[2]), "info exit 0");
}

#[tokio::test]
async fn its_input_ending_stops_it_within_the_apps_two_seconds_and_ends_its_log_as_the_tests_read_it(
) {
    LocalSet::new()
        .run_until(async {
            let root = tempfile::tempdir().unwrap();
            let rig = started(root.path()).await;
            ask(&rig.app, "ping", json!({})).await;
            let started = Instant::now();
            // The app closes the daemon's input.
            rig.app.close();
            tokio::time::timeout(Duration::from_secs(5), rig.daemon.finished())
                .await
                .expect("it stopped");
            assert!(
                started.elapsed() < Duration::from_millis(1_300),
                "{:?}",
                started.elapsed()
            );
            assert_eq!(*rig.exited.borrow(), [0]);
            assert_log_ends_as_the_app_reads_it(&rig.home, "stdin ended");
            assert_eq!(rig.said.text(), "");
        })
        .await;
}

#[tokio::test]
async fn a_signal_stops_it_for_the_signal_s_name() {
    LocalSet::new()
        .run_until(async {
            let root = tempfile::tempdir().unwrap();
            let rig = started(root.path()).await;
            // Its start is over (its resume of nothing included) before it is asked to stop:
            // the exit of a test returns, where the process's does not.
            ask(&rig.app, "ping", json!({})).await;
            tokio::time::sleep(Duration::from_millis(50)).await;
            rig.daemon.stop("SIGTERM");
            // Asked again, it is asked once: the first reason stands.
            rig.daemon.stop("SIGINT");
            tokio::time::timeout(Duration::from_secs(5), rig.daemon.finished())
                .await
                .expect("it stopped");
            assert_eq!(*rig.exited.borrow(), [0]);
            assert_log_ends_as_the_app_reads_it(&rig.home, "SIGTERM");
        })
        .await;
}

#[tokio::test]
async fn an_output_nobody_reads_stops_it_as_the_bridge_failing_while_its_input_stays_open() {
    LocalSet::new()
        .run_until(async {
            let root = tempfile::tempdir().unwrap();
            let (said, exited, handles) = (Said::default(), Rc::default(), Rc::default());
            // The app's end is made by hand: its input is dropped, its output stays.
            let (daemon_input, mut app_output) = duplex(64 * 1024);
            let (app_input, daemon_output) = duplex(64 * 1024);
            let (mut options, _unused) = options(root.path(), &said, &exited, &handles);
            options.input = Box::new(daemon_input);
            options.output = Box::new(daemon_output);
            let daemon = start(environment(root.path()), options).await.unwrap();
            drop(app_input);
            app_output
                .write_all(
                    format!(
                        "{}\n",
                        json!({ "v": 1, "id": "r-1", "kind": "req", "op": "ping", "body": {} })
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
            tokio::time::timeout(Duration::from_secs(5), daemon.finished())
                .await
                .expect("it stopped itself");
            assert_eq!(*exited.borrow(), [0]);
            let lines = log_lines(&daemon.parts.home);
            let failed = lines
                .iter()
                .position(|line| said_of(line) == "error the bridge failed")
                .expect("the failure was written");
            assert!(
                lines[failed + 1].starts_with("    "),
                "its cause is underneath: {lines:?}"
            );
            let stop = lines.iter().rev().nth(1).unwrap();
            assert!(
                said_of(stop).starts_with("info stop: the bridge failed; rss "),
                "{lines:?}"
            );
            assert_eq!(said_of(lines.last().unwrap()), "info exit 0");
        })
        .await;
}

#[tokio::test]
async fn a_second_daemon_on_the_same_home_is_refused_and_touches_no_window_of_the_first() {
    LocalSet::new()
        .run_until(async {
            let root = tempfile::tempdir().unwrap();
            let rig = started(root.path()).await;
            let launch = LaunchId::new("0b9f2c1e-5d4a-4c3b-9a8f-7e6d5c4b3a21").unwrap();
            let folder = rig
                .home
                .join("integrations")
                .join("claude")
                .join(launch.as_str());
            std::fs::create_dir_all(&folder).unwrap();
            std::fs::write(folder.join("settings.json"), "{}\n").unwrap();

            let (said, exited, handles) = (Said::default(), Rc::default(), Rc::default());
            let (second, _app) = options(root.path(), &said, &exited, &handles);
            let refused = start(environment(root.path()), second)
                .await
                .err()
                .expect("it is refused");
            assert_eq!(
                refused.to_string(),
                format!(
                    "another ConsensFlow has {} open",
                    rig.home.join("consensflow.db").display()
                )
            );
            assert!(
                handles.borrow().is_empty(),
                "no handle line for a daemon that did not start"
            );
            assert_eq!(
                std::fs::read_to_string(folder.join("settings.json")).unwrap(),
                "{}\n",
                "the running daemon's window keeps its files"
            );
            assert!(exited.borrow().is_empty());
            // The log says how the second ended, as Node's exit logger did:
            // its start line and, under it, `exit 1`.
            let lines = log_lines(&rig.home);
            assert_eq!(lines.len(), 3, "{lines:?}");
            assert_eq!(said_of(&lines[1]), said_of(&lines[0]));
            assert_eq!(said_of(&lines[2]), "info exit 1");
            // Nothing of the first was touched: it still answers.
            assert_eq!(
                ask(&rig.app, "ping", json!({})).await,
                json!({ "ok": true })
            );
        })
        .await;
}

#[tokio::test]
async fn a_ledger_that_cannot_be_opened_fails_the_start_and_the_log_ends_with_exit_1() {
    LocalSet::new()
        .run_until(async {
            let root = tempfile::tempdir().unwrap();
            let home = root.path().join("consensflow");
            std::fs::create_dir_all(&home).unwrap();
            std::fs::write(
                home.join("consensflow.db"),
                "this is no ledger\n".repeat(200),
            )
            .unwrap();
            let (said, exited, handles) = (Said::default(), Rc::default(), Rc::default());
            let (options, _app) = options(root.path(), &said, &exited, &handles);
            let refused = start(environment(root.path()), options).await;
            assert!(refused.is_err(), "no daemon on a corrupt ledger");
            assert!(handles.borrow().is_empty(), "no handle line");
            let lines = log_lines(&home);
            assert_eq!(lines.len(), 2, "{lines:?}");
            assert!(
                said_of(&lines[0]).starts_with("info start pid "),
                "{lines:?}"
            );
            assert_eq!(said_of(&lines[1]), "info exit 1");
        })
        .await;
}

#[tokio::test]
async fn an_agents_file_that_cannot_be_used_stops_no_start_and_the_log_says_why() {
    LocalSet::new()
        .run_until(async {
            let root = tempfile::tempdir().unwrap();
            let file = root.path().join("consensflow").join("agents.json");
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            // A hand edit's trailing comma: the roster refuses the file and saves nothing over it.
            let broken =
                "{\"schemaVersion\": 1, \"agents\": [{\"id\": \"mine\", \"kind\": \"codex\"},]}\n";
            std::fs::write(&file, broken).unwrap();
            let rig = started(root.path()).await;
            assert_eq!(
                ask(&rig.app, "ping", json!({})).await,
                json!({ "ok": true })
            );
            let lines = log_lines(&rig.home);
            let at = lines
                .iter()
                .position(|line| said_of(line) == "error the agents file could not be used")
                .expect("it said so");
            assert!(
                lines[at + 1].starts_with("    Your agents file "),
                "{lines:?}"
            );
            assert!(lines[at + 1].contains("is not valid JSON"), "{lines:?}");
            assert_eq!(
                std::fs::read_to_string(&file).unwrap(),
                broken,
                "left as the human wrote it"
            );
        })
        .await;
}

#[tokio::test]
async fn the_engine_s_work_is_driven_from_the_start_so_what_is_woken_outside_a_drain_runs() {
    LocalSet::new()
        .run_until(async {
            let root = tempfile::tempdir().unwrap();
            let rig = started(root.path()).await;
            // Its start is over, and the pass its resume woke has run and drained:
            // the next one is a second away, so only the driver is left to run
            // what is woken.
            ask(&rig.app, "ping", json!({})).await;
            tokio::time::sleep(Duration::from_millis(50)).await;
            let (gate, ran) = (Rc::new(Notify::new()), Rc::new(Cell::new(false)));
            let (held, marked) = (Rc::clone(&gate), Rc::clone(&ran));
            let spawn = &rig.daemon.parts.spawn;
            spawn.apart("a task failed", async move {
                held.notified().await;
                marked.set(true);
            });
            spawn.drain();
            assert!(!ran.get(), "it waits for the gate");
            // What a timer or a socket does: wakes it, and nobody drains.
            gate.notify_one();
            tokio::time::sleep(Duration::from_millis(20)).await;
            assert!(ran.get(), "the executor's driver ran it");
        })
        .await;
}

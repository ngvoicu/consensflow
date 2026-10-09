//! The Codex window's supervisor, `cf codex-session <codex> <args…>`, as a
//! window runs it, on a stand-in Codex: Codex's server on a private socket (on
//! Windows on loopback, for the window's own token), its TUI through the
//! broker, and nothing left behind once the window ends. The native `cf` runs
//! here; the broker's own tests are its crate's.

use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use cf_e2e::http::server::{Answer, Response, Server};
use cf_e2e::process::{is_alive, Run, Signal, Spawned};
use cf_e2e::{cf, files, serial, wire, Error, Result};
use regex::Regex;
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::{secs, Outcome};

/// The thread the stand-in Codex's server starts.
const A: &str = "01a09094-938f-7fd1-a2d3-315cf92b4559";

/// The private token the window's launch was given.
const TOKEN: &str = "private-launch-token-1234567890";

/// The stand-in `codex` the supervisor starts.
const FAKE_CODEX: &str = env!("CARGO_BIN_EXE_fake-codex");

/// How long a window is given to end.
const EXIT: Duration = Duration::from_secs(60);

/// A supervisor, running: `cf codex-session` on a stand-in Codex, in a home of
/// its own that goes with it.
struct Supervised {
    /// The folder of the window's home: ConsensFlow's home too.
    root: TempDir,
    /// The port the broker listens on.
    port: u16,
    child: Spawned,
    log: PathBuf,
}

/// A port nobody listens on.
fn free_port() -> std::io::Result<u16> {
    Ok(TcpListener::bind("127.0.0.1:0")?.local_addr()?.port())
}

impl Supervised {
    /// The supervisor as a Codex window runs it: `cf codex-session <codex>
    /// <args>`, with the session bridge the channel configured, in a window's
    /// environment. `vars` are added to it. `codex` is the program it runs,
    /// the stand-in itself unless the case gives another.
    fn start(
        args: &[&str],
        vars: &[(&str, String)],
        codex: Option<&dyn Fn(&Path) -> PathBuf>,
    ) -> Result<Self> {
        let root = tempfile::Builder::new()
            .prefix("cf-cx-")
            .tempdir()
            .map_err(|source| Error::File {
                action: "make a folder in",
                path: std::env::temp_dir(),
                source,
            })?;
        let program = codex.map_or_else(|| PathBuf::from(FAKE_CODEX), |named| named(root.path()));
        let port = free_port().map_err(|source| Error::File {
            action: "find a port on",
            path: PathBuf::from("127.0.0.1"),
            source,
        })?;
        let log = root.path().join("codex.jsonl");
        let bridge = json!({ "launchId": "launch-1", "port": port, "token": TOKEN });
        let mut child = Run::new(cf::binary()?)
            .arg("codex-session")
            .arg(&program)
            .args(args)
            .var("CONSENSFLOW_HOME", root.path())
            .var("CONSENSFLOW_URL", "http://127.0.0.1:1")
            .var("CONSENSFLOW_TOKEN", "window-token")
            .var("CF_CODEX_SESSION_BRIDGE", bridge.to_string())
            .var("CF_TEST_CODEX_LOG", &log)
            .var("OPENAI_API_KEY", "sk-not-for-codex")
            .vars(vars.iter().map(|(name, value)| (name, value)))
            .finding_programs()
            .closed_input()
            .spawn()?;
        // What it prints is of no matter, and a pipe nobody reads would stop it.
        if let Some(output) = child.take_output() {
            wire::read_each(output, |_| true, || {});
        }
        Ok(Self {
            root,
            port,
            child,
            log,
        })
    }

    /// Every run the stand-in Codex wrote down, in order.
    fn runs(&self) -> Vec<Value> {
        files::read_string(&self.log)
            .unwrap_or_default()
            .lines()
            .filter(|line| !line.is_empty())
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect()
    }

    /// The runs, once `found` says they have got there: looked at every 25 ms,
    /// for up to ten seconds.
    fn until(&self, found: impl Fn(&[Value]) -> bool) -> Result<Vec<Value>> {
        for _ in 0..400 {
            let seen = self.runs();
            if found(&seen) {
                return Ok(seen);
            }
            thread::sleep(Duration::from_millis(25));
        }
        Err(Error::Timeout(format!(
            "the supervised Codex never got there: {}",
            self.child.errors()
        )))
    }

    /// Waits for the window to end: the code it exited with (none for a signal).
    fn exit(&mut self) -> Result<Option<i32>> {
        match self.child.wait(EXIT)? {
            Some(status) => Ok(status.code()),
            None => Err(Error::Timeout(format!(
                "the window did not end in {} s: {}",
                EXIT.as_secs(),
                self.child.errors()
            ))),
        }
    }

    /// What it said on its error output.
    fn stderr(&self) -> String {
        self.child.errors()
    }
}

/// The runs of the stand-in Codex's `kind`, in order.
fn of(runs: &[Value], kind: &str) -> Vec<Value> {
    runs.iter()
        .filter(|run| run["run"] == kind)
        .cloned()
        .collect()
}

/// The private folder of the socket Codex's server was told to listen on: Unix
/// only.
fn socket_folder(server: &Value) -> PathBuf {
    let args = server["args"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default();
    let at = args.iter().position(|arg| arg == "--listen");
    let listen = at
        .and_then(|at| args.get(at + 1))
        .and_then(Value::as_str)
        .unwrap_or_default();
    Path::new(listen.trim_start_matches("unix://"))
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default()
}

fn pid(run: &Value) -> u32 {
    run["pid"]
        .as_u64()
        .and_then(|pid| u32::try_from(pid).ok())
        .unwrap_or_default()
}

#[test]
fn opens_codex_as_its_server_on_a_private_socket_and_its_tui_through_the_broker_and_leaves_nothing_behind(
) -> Outcome {
    let _turn = serial::turn();
    let mut s = Supervised::start(
        &[
            "--model",
            "native-model",
            "--dangerously-bypass-approvals-and-sandbox",
        ],
        &[],
        None,
    )?;
    assert_eq!(s.exit()?, Some(7), "{}", s.stderr());
    let runs = s.runs();
    let (server_runs, tui_runs) = (of(&runs, "server"), of(&runs, "tui"));
    // On Unix a socket in a private folder of the home; on Windows loopback, on
    // a port the server picks, for a token of the window's own, which the
    // server is given as its SHA-256.
    let args: Vec<&str> = server_runs[0]["args"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    let hash = args.last().copied().unwrap_or_default();
    let socket = socket_folder(&server_runs[0]).join("native.sock");
    if cfg!(windows) {
        assert!(Regex::new(r"^[0-9a-f]{64}$")?.is_match(hash), "{hash}");
    } else {
        let private = format!(
            "{}{}codex-",
            s.root.path().join("tmp").display(),
            std::path::MAIN_SEPARATOR
        );
        assert!(
            socket.to_string_lossy().starts_with(&private),
            "{}",
            socket.display()
        );
    }
    // The server gets the model and the bypass as configuration, and
    // ConsensFlow's own variables set in its shell policy.
    let root = serde_json::to_string(&s.root.path().to_string_lossy())?;
    let mut expected: Vec<String> = [
        "-c",
        "model=\"native-model\"",
        "-c",
        "approval_policy=\"never\"",
        "-c",
        "sandbox_mode=\"danger-full-access\"",
        "-c",
    ]
    .iter()
    .map(|word| (*word).to_owned())
    .collect();
    expected.extend([
        format!("shell_environment_policy.set.CONSENSFLOW_HOME={root}"),
        "-c".to_owned(),
        "shell_environment_policy.set.CONSENSFLOW_TOKEN=\"window-token\"".to_owned(),
        "-c".to_owned(),
        "shell_environment_policy.set.CONSENSFLOW_URL=\"http://127.0.0.1:1\"".to_owned(),
        "app-server".to_owned(),
    ]);
    if cfg!(windows) {
        expected.extend(
            [
                "--listen",
                "ws://127.0.0.1:0",
                "--ws-auth",
                "capability-token",
                "--ws-token-sha256",
                hash,
            ]
            .map(str::to_owned),
        );
    } else {
        expected.extend([
            "--listen".to_owned(),
            format!("unix://{}", socket.display()),
        ]);
    }
    assert_eq!(server_runs[0]["args"], json!(expected));
    // The TUI reaches the server only through the broker, its token in its
    // environment and never on its command line.
    assert_eq!(
        tui_runs[0]["args"],
        json!([
            "--remote",
            format!("ws://127.0.0.1:{}", s.port),
            "--remote-auth-token-env",
            "CF_CODEX_TUI_TOKEN",
            "--model",
            "native-model"
        ])
    );
    assert_eq!(tui_runs[0]["token"], TOKEN);
    assert_eq!(
        [
            server_runs[0]["apiKey"].clone(),
            tui_runs[0]["apiKey"].clone()
        ],
        [Value::Null, Value::Null],
        "an OpenAI API key never reaches Codex"
    );
    // The fresh thread the TUI started is the broker's to deliver to, with the bypass.
    assert_eq!(
        [
            server_runs[1]["started"]["approvalPolicy"].clone(),
            server_runs[1]["started"]["sandbox"].clone()
        ],
        [json!("never"), json!("danger-full-access")]
    );
    assert_eq!(
        tui_runs[1]["session"],
        json!({
            "launchId": "launch-1",
            "sessionId": A,
            "revision": 1,
            "empty": true,
            "available": true,
        })
    );
    assert!(
        !is_alive(pid(&server_runs[0])),
        "the server went with the window"
    );
    if !cfg!(windows) {
        assert!(
            !socket_folder(&server_runs[0]).exists(),
            "the socket folder went with the session"
        );
    }
    Ok(())
}

#[test]
fn says_why_codexs_server_could_not_start_in_the_window_and_leaves_nothing_behind() -> Outcome {
    let _turn = serial::turn();
    let mut s = Supervised::start(&[], &[("CF_TEST_CODEX_BACKEND", "fail".to_owned())], None)?;
    assert_eq!(s.exit()?, Some(1), "{}", s.stderr());
    assert_eq!(
        s.stderr(),
        "ConsensFlow could not open Codex: Codex server could not start: not logged in\n\n"
    );
    let runs = s.runs();
    let kinds: Vec<&str> = runs.iter().filter_map(|run| run["run"].as_str()).collect();
    assert_eq!(kinds, ["server"], "no TUI was opened");
    if !cfg!(windows) {
        assert!(!socket_folder(&runs[0]).exists());
    }
    // A Codex gone from where the launch found it fails the same way, in its
    // own words: Rust's, deliberately, where Node said `spawn <path> ENOENT`.
    // Its folder is still there, so Windows too says the file is missing (os
    // error 2), not its path (3).
    let mut missing = Supervised::start(&[], &[], Some(&|root| root.join("codex")))?;
    assert_eq!(missing.exit()?, Some(1), "{}", missing.stderr());
    let gone = regex::escape(&missing.root.path().join("codex").to_string_lossy());
    let said = missing.stderr();
    assert!(
        Regex::new(&format!(
            r"^ConsensFlow could not open Codex: Codex server could not start: {gone}: .*\(os error 2\)\n$"
        ))?
        .is_match(&said),
        "{said}"
    );
    if !cfg!(windows) {
        let tmp = std::fs::read_dir(missing.root.path().join("tmp"))?.count();
        assert_eq!(tmp, 0);
    }
    Ok(())
}

/// A window closed by `signal`: both Codex processes end with it.
fn ends_both_processes_when_its_window_is_closed(signal: Signal) -> Outcome {
    let _turn = serial::turn();
    let mut s = Supervised::start(&[], &[("CF_TEST_CODEX_TUI", "wait".to_owned())], None)?;
    let seen = s.until(|runs| runs.iter().any(|run| run.get("session").is_some()))?;
    let server = of(&seen, "server")[0].clone();
    let tui = of(&seen, "tui")[0].clone();
    s.child.signal(signal);
    assert_eq!(s.exit()?, Some(0), "{}", s.stderr());
    assert_eq!(
        [is_alive(pid(&server)), is_alive(pid(&tui))],
        [false, false]
    );
    assert!(!socket_folder(&server).exists());
    Ok(())
}

#[test]
#[cfg_attr(windows, ignore = "no SIGTERM on Windows")]
fn ends_both_codex_processes_when_its_window_is_closed_sigterm() -> Outcome {
    ends_both_processes_when_its_window_is_closed(Signal::Terminate)
}

#[test]
#[cfg_attr(windows, ignore = "no SIGTERM on Windows")]
fn ends_both_codex_processes_when_its_window_is_closed_sigint() -> Outcome {
    ends_both_processes_when_its_window_is_closed(Signal::Interrupt)
}

/// The board's API, with a question it is asked and never answers: the polls
/// it saw.
fn silent_board() -> std::io::Result<(Server, Arc<Mutex<Vec<String>>>)> {
    let polls = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&polls);
    let server = Server::start(
        move |request| match (request.method.as_str(), request.path()) {
            ("POST", "/api/questions") => {
                Answer::Respond(Response::json(201, &json!({ "message": { "id": 61 } })))
            }
            ("GET", path) if path.starts_with("/api/questions/61") => {
                seen.lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push(request.target.clone());
                Answer::Hold
            }
            _ => Answer::Respond(Response::status(404)),
        },
    )?;
    Ok((server, polls))
}

#[test]
fn ends_with_its_tui_even_while_a_question_of_codexs_is_still_waiting_at_the_board() -> Outcome {
    let _turn = serial::turn();
    // The board's door waits nearly an hour for the answer to a question; the
    // window, whose TUI went, must not wait with it.
    let (board, polls) = silent_board()?;
    let mut s = Supervised::start(
        &[],
        &[
            ("CF_TEST_CODEX_QUESTION", "1".to_owned()),
            ("CF_TEST_CODEX_TUI", "wait".to_owned()),
            ("CONSENSFLOW_URL", board.origin()),
        ],
        None,
    )?;
    let seen = s.until(|runs| runs.iter().any(|run| run.get("session").is_some()))?;
    let held = |polls: &Arc<Mutex<Vec<String>>>| {
        !polls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_empty()
    };
    for _ in 0..400 {
        if held(&polls) {
            break;
        }
        thread::sleep(Duration::from_millis(25));
    }
    assert!(
        held(&polls),
        "the question is held at the board: {}",
        s.stderr()
    );
    let tui = of(&seen, "tui")[0].clone();
    cf_e2e::process::signal(pid(&tui), Signal::Terminate)?;
    let started = Instant::now();
    let waited = s.child.wait(secs(15))?;
    assert!(waited.is_some(), "the window did not end with its TUI");
    assert!(started.elapsed() < secs(15));
    let server = of(&seen, "server")[0].clone();
    assert!(!is_alive(pid(&server)));
    if !cfg!(windows) {
        assert!(!socket_folder(&server).exists());
    }
    Ok(())
}

#[test]
fn ends_the_tui_when_codexs_server_dies_under_it_so_the_window_does_not_hang() -> Outcome {
    let _turn = serial::turn();
    let mut s = Supervised::start(
        &[],
        &[
            ("CF_TEST_CODEX_BACKEND", "die".to_owned()),
            ("CF_TEST_CODEX_TUI", "wait".to_owned()),
        ],
        None,
    )?;
    let code = s.exit()?;
    // The TUI's code is the window's: 0 when a signal ended it. Windows has no
    // signal, and a TUI ended there (taskkill /F) has the code that gave it.
    if !cfg!(windows) {
        assert_eq!(code, Some(0), "{}", s.stderr());
    }
    let runs = s.runs();
    assert!(!is_alive(pid(&of(&runs, "tui")[0])));
    if !cfg!(windows) {
        assert!(!socket_folder(&of(&runs, "server")[0]).exists());
    }
    Ok(())
}

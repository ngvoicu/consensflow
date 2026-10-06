//! `cf ui` as a process, as the app starts it: the native daemon behind its
//! switch, its standard streams real pipes, the app's end of the bridge a
//! few lines of this file, and a home and a ledger of its own.

// The tests' own helper: a failure in it is the test's, and it starts cf
// itself.
#![allow(clippy::expect_used, clippy::disallowed_methods)]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use cf_ledger::{open_ledger, NewChief, NewMember, NewProject, NewQuestion, Options};
use serde_json::{json, Value};

/// A home for a daemon: its folder, its fake `claude`, its workspace.
pub struct Root {
    pub dir: tempfile::TempDir,
}

impl Root {
    pub fn new() -> Self {
        let root = Self {
            dir: tempfile::tempdir().expect("a temporary folder"),
        };
        std::fs::create_dir_all(root.bin()).expect("a bin folder");
        cf_harness::testing::fake_window_executable(&root.bin().join("claude"));
        root
    }

    pub fn home(&self) -> PathBuf {
        self.dir.path().join("consensflow")
    }

    pub fn bin(&self) -> PathBuf {
        self.dir.path().join("bin")
    }

    pub fn log(&self) -> String {
        std::fs::read_to_string(self.home().join("daemon.log")).unwrap_or_default()
    }

    /// A `claude` that never answers: it writes its pid to a file beside it
    /// (the file is what this gives) and sleeps, two minutes at most. The
    /// daemon runs a CLI in the environment's `HOME`, which is made here: a
    /// program whose folder is not there does not start. And a run that never
    /// gets its version never asks a release feed.
    #[cfg(unix)]
    pub fn claude_that_never_answers(&self) -> PathBuf {
        std::fs::create_dir_all(self.dir.path().join("home")).expect("a home folder");
        std::fs::write(
            self.bin().join("claude"),
            "#!/bin/sh\necho $$ > \"${0%/*}/asked\"\nexec /bin/sleep 120\n",
        )
        .expect("a stand-in claude");
        self.bin().join("asked")
    }

    /// A project that was open when the last daemon ended, with its chief on
    /// a saved agent, a worker `zeus`, and a question the chief put to zeus:
    /// a door's wait. The question's number.
    pub fn open_project_with_a_question(&self) -> i64 {
        std::fs::create_dir_all(self.home()).expect("the home");
        std::fs::write(
            self.home().join("agents.json"),
            json!({
                "schemaVersion": 1,
                "agents": [{ "id": "mybuilder", "kind": "claude-code", "model": "fake", "workTier": "standard" }]
            })
            .to_string(),
        )
        .expect("the agents file");
        let workspace = self.dir.path().join("workspace");
        std::fs::create_dir_all(&workspace).expect("a workspace");
        let mut ledger =
            open_ledger(&self.home().join("consensflow.db"), Options::default()).expect("a ledger");
        let project = ledger
            .create_project(&NewProject {
                directory: workspace.to_string_lossy().into_owned(),
                name: "app".to_owned(),
                chief: NewChief {
                    harness: "claude-code".to_owned(),
                    agent: Some("mybuilder".to_owned()),
                },
                staff: vec![NewMember {
                    agent: "zeus".to_owned(),
                    harness: "claude-code".to_owned(),
                    designer: false,
                    roles: vec!["worker".to_owned()],
                    tier: "standard".to_owned(),
                }],
                gate: false,
            })
            .expect("a project");
        let question = ledger
            .ask(
                project.id,
                &NewQuestion {
                    from: Some("chief".to_owned()),
                    to: "zeus".to_owned(),
                    body: Some("Which?".to_owned()),
                    ..NewQuestion::default()
                },
            )
            .expect("a question");
        ledger.close().expect("the ledger closed");
        question.id
    }
}

/// A running `cf ui --json --no-open` on the native daemon.
pub struct Daemon {
    pub child: Child,
    pub stdin: Option<ChildStdin>,
    stdout: Option<BufReader<ChildStdout>>,
    pub handle: Value,
    pub errors: Arc<Mutex<String>>,
}

impl Daemon {
    /// Starts it over `root`, and reads its handle line.
    pub fn start(root: &Root) -> Self {
        let mut command = Command::new(env!("CARGO_BIN_EXE_cf"));
        command
            .args(["ui", "--json", "--no-open"])
            .env_clear()
            .env("CONSENSFLOW_DAEMON", "native")
            .env("CONSENSFLOW_HOME", root.home())
            .env("HOME", root.dir.path().join("home"))
            .env("USERPROFILE", root.dir.path().join("home"))
            .env(
                "CLAUDE_CONFIG_DIR",
                root.dir.path().join("home").join(".claude"),
            )
            .env("CODEX_HOME", root.dir.path().join("home").join(".codex"))
            .env(
                "XDG_CONFIG_HOME",
                root.dir.path().join("home").join(".config"),
            )
            .env("CONSENSFLOW_NODE", "/the/app/named/this/node")
            .env("PATH", root.bin())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if cfg!(windows) {
            for name in ["SystemRoot", "ComSpec", "PATHEXT"] {
                if let Some(value) = std::env::var_os(name) {
                    command.env(name, value);
                }
            }
        }
        let mut child = command.spawn().expect("cf ui starts");
        let errors = Arc::new(Mutex::new(String::new()));
        let mut stderr = child.stderr.take().expect("its error output");
        let said = Arc::clone(&errors);
        std::thread::spawn(move || {
            let mut text = String::new();
            let _ = stderr.read_to_string(&mut text);
            said.lock().expect("the errors").push_str(&text);
        });
        let stdin = child.stdin.take();
        let mut stdout = BufReader::new(child.stdout.take().expect("its output"));
        let mut line = String::new();
        stdout.read_line(&mut line).expect("a handle line");
        let handle: Value = serde_json::from_str(line.trim()).unwrap_or_else(|failed| {
            panic!(
                "no handle line ({failed}): {line:?}; errors: {}",
                errors.lock().expect("the errors")
            )
        });
        Self {
            child,
            stdin,
            stdout: Some(stdout),
            handle,
            errors,
        }
    }

    pub fn url(&self) -> String {
        self.handle["url"]
            .as_str()
            .expect("a url")
            .trim_end_matches('/')
            .to_owned()
    }

    /// The bridge's frames from here on, read on a thread of their own until
    /// the output is done with: what the app's end reads.
    pub fn frames(&mut self) -> Frames {
        let stdout = self.stdout.take().expect("the output is not read yet");
        let (sender, frames) = mpsc::channel();
        let done = Arc::new(Mutex::new(false));
        let finished = Arc::clone(&done);
        let reader = std::thread::spawn(move || {
            for line in stdout.lines() {
                let Ok(line) = line else { break };
                if let Ok(frame) = serde_json::from_str::<Value>(&line) {
                    if sender.send(frame).is_err() {
                        break;
                    }
                }
                if *finished.lock().expect("done") {
                    break;
                }
            }
        });
        Frames {
            frames,
            done,
            _reader: reader,
        }
    }

    /// Writes one frame to the daemon's input.
    pub fn send(&mut self, frame: &Value) {
        let stdin = self.stdin.as_mut().expect("its input is open");
        let _ = writeln!(stdin, "{frame}");
        let _ = stdin.flush();
    }

    pub fn ping(&mut self, id: &str) {
        self.send(&json!({ "v": 1, "id": id, "kind": "req", "op": "ping", "body": {} }));
    }

    /// Its input ended, as the app ends it.
    pub fn end_input(&mut self) {
        self.stdin = None;
    }

    /// SIGTERM, with its input left open.
    #[cfg(unix)]
    pub fn terminate(&self) {
        let pid = self.child.id().to_string();
        let sent = Command::new("kill").args(["-TERM", &pid]).status();
        assert!(
            sent.is_ok_and(|status| status.success()),
            "kill -TERM {pid}"
        );
    }

    /// Waits for it to exit: its code and how long it took from `since`.
    pub fn exits(&mut self, since: Instant, within: Duration) -> (Option<i32>, Duration) {
        loop {
            if let Some(status) = self.child.try_wait().expect("waiting for it") {
                return (status.code(), since.elapsed());
            }
            assert!(
                since.elapsed() < within,
                "the daemon did not stop within {within:?}; errors: {}; log:\n{}",
                self.errors.lock().expect("the errors"),
                "(see the home's daemon.log)"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The bridge's frames, as they come.
pub struct Frames {
    frames: Receiver<Value>,
    done: Arc<Mutex<bool>>,
    _reader: JoinHandle<()>,
}

impl Frames {
    /// The next frame within `within`.
    pub fn next(&self, within: Duration) -> Option<Value> {
        self.frames.recv_timeout(within).ok()
    }

    /// The reader gives up the output after the next line it reads, so that
    /// the daemon's next write finds nobody there.
    pub fn leave_after_the_next_line(&self) {
        *self.done.lock().expect("done") = true;
    }
}

/// A raw HTTP exchange on a connection the test keeps: written as given,
/// read to the end of what comes.
pub struct Client {
    stream: TcpStream,
}

impl Client {
    pub fn connect(url: &str) -> Self {
        let address = url.strip_prefix("http://").expect("an http url");
        let stream = TcpStream::connect(address).expect("a connection");
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .expect("a read timeout");
        Self { stream }
    }

    pub fn write(&mut self, text: &str) {
        self.stream.write_all(text.as_bytes()).expect("written");
    }

    /// Everything until the connection ends: whether it did, within the read timeout.
    pub fn read_to_the_end(&mut self) -> (String, bool) {
        let mut said = Vec::new();
        let ended = match self.stream.read_to_end(&mut said) {
            Ok(_) => true,
            // A connection reset is an end too.
            Err(failed) => {
                failed.kind() != std::io::ErrorKind::WouldBlock
                    && failed.kind() != std::io::ErrorKind::TimedOut
            }
        };
        (String::from_utf8_lossy(&said).into_owned(), ended)
    }
}

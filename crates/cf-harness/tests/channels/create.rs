//! The conversation a fresh window opens on, made as the system makes it
//! (Node's `describe('opencode createSession')`): one empty session on a
//! throwaway `opencode serve`, a child process of the machine's own
//! (`fake-opencode`, which the environment tells how to behave), stopped before
//! the id returns and on every failure. The child is checked to be gone, which
//! `tests/launch/opencode/creates.rs`, where the child is scripted, cannot.

use std::fs;
use std::net::TcpListener;
use std::time::{Duration, Instant};

use cf_base::env::Env;
use cf_harness::opencode::{create_session, Bridge, Channel, Launched, Serve, TIMEOUT_MS};
use cf_harness::seams::SystemProcesses;
use serde_json::{json, Value};
use tempfile::TempDir;
use uuid::Uuid;

use crate::support::{matches, real_folder, run, wires};

/// The stand-in `opencode`.
const FAKE: &str = env!("CARGO_BIN_EXE_fake-opencode");

/// What a window's launch gives OpenCode to serve with, as the stand-in takes
/// it: a free loopback port, the server's arguments, and a password of its own
/// that the channel asks it with; the folder the window works in; and the
/// stand-in's environment, which tells it how to behave and where to leave
/// what it saw.
struct Setup {
    /// Where the stand-in leaves what it saw.
    dir: TempDir,
    /// The folder the window works in, as the system names it.
    folder: String,
    _workspace: TempDir,
    port: u16,
    launched: Launched,
    env: Env,
}

impl Setup {
    /// A setup whose stand-in behaves as `mode` says.
    fn new(mode: &str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let port = TcpListener::bind(("127.0.0.1", 0))
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let password = Uuid::new_v4().simple().to_string();
        let state = dir.path().join("state");
        Self {
            folder: real_folder(workspace.path()),
            _workspace: workspace,
            port,
            launched: Launched {
                args: ["--port", &port.to_string(), "--hostname", "127.0.0.1"]
                    .map(str::to_owned)
                    .to_vec(),
                env: vec![
                    ("OPENCODE_SERVER_PASSWORD".to_owned(), password.clone()),
                    ("OPENCODE_SERVER_USERNAME".to_owned(), "opencode".to_owned()),
                ],
                channel: Channel {
                    launch_id: "t-create".to_owned(),
                    endpoint: format!("http://127.0.0.1:{port}"),
                    password,
                    bridge: Bridge {
                        endpoint: String::new(),
                        token: String::new(),
                    },
                },
            },
            env: Env::from_vars([
                ("CF_FIXTURE_STATE", state.to_string_lossy().into_owned()),
                ("CF_FIXTURE_MODE", mode.to_owned()),
                ("CF_FIXTURE_PING", "pong".to_owned()),
            ]),
            dir,
        }
    }

    /// A session made in the folder by `executable`, within `timeout_ms` (a
    /// window's, where there is none).
    async fn create(&self, executable: &str, timeout_ms: Option<i64>) -> Result<String, String> {
        let processes = SystemProcesses::new(Env::from_process());
        let serve = Serve {
            executable,
            directory: &self.folder,
            env: &self.env,
            launched: &self.launched,
            timeout_ms: timeout_ms.unwrap_or(TIMEOUT_MS),
        };
        create_session(wires(&processes), &serve).await
    }

    /// What the stand-in left in the file `name`, if it did.
    fn left(&self, name: &str) -> Option<String> {
        fs::read_to_string(self.dir.path().join(format!("state.{name}"))).ok()
    }

    /// What the stand-in said of itself when it started, waited for a while:
    /// none where it never did.
    async fn started(&self) -> Option<Value> {
        let until = Instant::now() + Duration::from_secs(5);
        loop {
            let said = self.left("startup");
            if let Some(started) = said.and_then(|text| serde_json::from_str(&text).ok()) {
                return Some(started);
            }
            if Instant::now() >= until {
                return None;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    /// Whether the server that started as `started` says is gone: its process
    /// is, where there are processes to ask, and its port is free for another to
    /// take. Waited for a while.
    async fn gone(&self, started: &Value) -> bool {
        let pid = started["pid"].as_u64().unwrap();
        let until = Instant::now() + Duration::from_secs(3);
        loop {
            if !running(pid) && TcpListener::bind(("127.0.0.1", self.port)).is_ok() {
                return true;
            }
            if Instant::now() >= until {
                return false;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
}

/// Whether the process `pid` is there (a signal that is none asks).
#[cfg(unix)]
fn running(pid: u64) -> bool {
    use nix::sys::signal::kill;
    use nix::unistd::Pid;
    i32::try_from(pid).is_ok_and(|pid| kill(Pid::from_raw(pid), None).is_ok())
}

/// Windows is not asked for a process by its number here: its port says.
#[cfg(not(unix))]
fn running(_pid: u64) -> bool {
    false
}

/// An id of a conversation: `ses_` and letters and digits.
fn is_id(id: &str) -> bool {
    matches(id, "^ses_[A-Za-z0-9]+$")
}

#[test]
fn creates_exactly_one_empty_session_and_frees_the_port() {
    run(async {
        let setup = Setup::new("ok");
        let id = setup.create(FAKE, None).await.unwrap();
        assert!(is_id(&id), "{id}");
        assert_eq!(setup.left("creates").as_deref(), Some("1"));
        assert_eq!(setup.left("body").as_deref(), Some("{}"));
        assert_eq!(setup.left("query").as_deref(), Some(setup.folder.as_str()));
        let started = setup.started().await.unwrap();
        let mut argv = vec!["serve".to_owned()];
        argv.extend(setup.launched.args.clone());
        assert_eq!(started["argv"], json!(argv));
        assert_eq!(started["cwd"], setup.folder);
        assert_eq!(started["username"], "opencode");
        assert_eq!(started["hasPassword"], true);
        assert_eq!(started["ping"], "pong");
        assert!(
            setup.gone(&started).await,
            "temporary server is reaped before return"
        );
    });
}

#[test]
fn mints_distinct_ids_on_two_calls() {
    run(async {
        let (a, b) = (Setup::new("ok"), Setup::new("ok"));
        let first = a.create(FAKE, None).await.unwrap();
        let second = b.create(FAKE, None).await.unwrap();
        assert_ne!(first, second);
    });
}

#[test]
fn refuses_a_missing_executable_without_a_child() {
    run(async {
        let setup = Setup::new("ok");
        let missing = setup.dir.path().join("does-not-exist");
        let failed = setup
            .create(missing.to_str().unwrap(), Some(3000))
            .await
            .unwrap_err();
        assert_eq!(failed, "opencode serve failed to start");
        assert_eq!(setup.started().await, None);
    });
}

#[test]
fn surfaces_early_exit_and_unauthorized_without_leaking_secrets() {
    run(async {
        let dead = Setup::new("early-exit");
        assert!(dead.create(FAKE, Some(3000)).await.is_err());
        let mut setup = Setup::new("ok");
        let password = setup.launched.channel.password.clone();
        setup.launched.channel.password = "wrong".to_owned();
        let error = setup.create(FAKE, Some(4000)).await.unwrap_err();
        assert!(!matches(&error, "(?i)wrong|hunter2|SECRET"), "{error}");
        assert!(!error.contains(&password), "{error}");
    });
}

#[test]
fn rejects_malformed_invalid_id_wrong_dir_and_oversized_responses() {
    run(async {
        for mode in ["bad-json", "invalid-id", "wrong-dir", "oversized"] {
            let setup = Setup::new(mode);
            let made = setup.create(FAKE, Some(5000)).await;
            assert!(made.is_err(), "{mode}: {made:?}");
            if let Some(started) = setup.started().await {
                assert!(setup.gone(&started).await, "{mode}: child reaped");
            }
        }
    });
}

#[test]
fn times_out_on_hanging_endpoints_and_reaps_an_ignored_sigterm() {
    run(async {
        let hanging = Setup::new("hang-health");
        assert!(hanging.create(FAKE, Some(5000)).await.is_err());
        let started = hanging.started().await.unwrap();
        assert!(hanging.gone(&started).await, "hanging child reaped");
        let stubborn = Setup::new("ignore-sigterm");
        let began = Instant::now();
        let id = stubborn.create(FAKE, Some(8000)).await.unwrap();
        let took = began.elapsed();
        assert!(is_id(&id), "{id}");
        // Where there are signals, the stand-in ignored the first, and it was
        // the second that ended it, after the two seconds a child has to close.
        assert!(!cfg!(unix) || took >= Duration::from_secs(2), "{took:?}");
        let started = stubborn.started().await.unwrap();
        assert!(
            stubborn.gone(&started).await,
            "SIGKILL fallback reaps the child"
        );
    });
}

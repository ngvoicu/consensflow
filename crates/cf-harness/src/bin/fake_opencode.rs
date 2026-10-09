//! A stand-in for the `opencode` program, for the tests of OpenCode's channel
//! (`tests/channels/create.rs`): the part of `opencode serve` that the channel
//! asks of it to make a window's first conversation. It is no adapter, and
//! ships with nothing: it is built only with the `test-support` feature.
//!
//! Started as `fake-opencode serve --port <port> --hostname <host>`, it
//! listens on that port of loopback and answers `GET /global/health` and
//! `POST /session`, both only to the password in `OPENCODE_SERVER_PASSWORD`
//! (basic auth, the user `opencode`). A session it makes is named for its
//! process, and for the folder the request names (the folder it was started
//! in where it names none). Its environment tells it the rest:
//! - `CF_FIXTURE_STATE`: a path beside which it leaves what it saw. `.startup`,
//!   when it starts: JSON of its process id (`pid`), arguments (`argv`), folder
//!   (`cwd`), `OPENCODE_SERVER_USERNAME` (`username`), whether it was given a
//!   password (`hasPassword`) and `CF_FIXTURE_PING` (`ping`). `.creates`, `.body`
//!   and `.query`, for the last session it was asked for: how many it was asked
//!   for so far, what the request said, and the `directory` it named.
//! - `CF_FIXTURE_MODE`, how it fails, `ok` where it does not: `early-exit` (it
//!   ends with 1 before it listens), `ignore-sigterm` (where there are signals,
//!   it ignores SIGTERM), `hang-health` and `hang-session` (it takes the
//!   request and never answers), and for a session it makes: `bad-json`
//!   (an answer that is no JSON), `oversized` (an id of two mebibytes),
//!   `invalid-id` (one the channel refuses) and `wrong-dir` (a folder that is
//!   not the one asked for).

#![forbid(unsafe_code)]

use std::cell::Cell;
use std::fs;
use std::io::Write;
use std::process::ExitCode;

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use cf_base::env::Env;
use cf_base::time::{Clock, SystemClock};
use cf_harness::testing::server::{serve, Reply, Request};
use serde_json::json;
use tokio::net::TcpListener;
use tokio::task::LocalSet;

/// What the environment tells the process to do and where to leave what it saw.
struct Fixture {
    state: String,
    mode: String,
    /// The `authorization` that gets an answer.
    want: String,
}

impl Fixture {
    fn read(env: &Env) -> Self {
        let password = env.text("OPENCODE_SERVER_PASSWORD").unwrap_or_default();
        Self {
            state: env.text("CF_FIXTURE_STATE").unwrap_or_default().to_owned(),
            mode: env.text("CF_FIXTURE_MODE").unwrap_or("ok").to_owned(),
            want: format!("Basic {}", STANDARD.encode(format!("opencode:{password}"))),
        }
    }

    /// Leaves `text` in the file `name` beside the state path. A file that
    /// cannot be written is one the test then finds missing.
    fn leave(&self, name: &str, text: &str) {
        let _ = fs::write(format!("{}.{name}", self.state), text);
    }

    fn authorized(&self, request: &Request) -> bool {
        request.header("authorization") == Some(self.want.as_str())
    }

    /// `GET /global/health`.
    fn health(&self, request: &Request) -> Reply {
        if self.mode == "hang-health" {
            Reply::Hang
        } else if self.authorized(request) {
            Reply::json(200, &json!({ "healthy": true }))
        } else {
            Reply::text(401, "unauthorized")
        }
    }

    /// `POST /session`; `asked` counts the sessions asked for by those who may.
    fn session(&self, request: &Request, asked: &Cell<u32>) -> Reply {
        if self.mode == "hang-session" {
            return Reply::Hang;
        }
        if !self.authorized(request) {
            return Reply::text(401, "unauthorized");
        }
        asked.set(asked.get() + 1);
        let creates = asked.get();
        let named = request.query("directory").unwrap_or_default();
        self.leave("creates", &creates.to_string());
        self.leave("body", &String::from_utf8_lossy(&request.body));
        self.leave("query", &named);
        let here = if named.is_empty() {
            std::env::current_dir().map_or_else(|_| String::new(), |dir| dir.display().to_string())
        } else {
            named
        };
        let made = |id: &str, directory: &str| {
            Reply::json(200, &json!({ "id": id, "directory": directory }))
        };
        match self.mode.as_str() {
            "bad-json" => Reply::text(200, "not json{"),
            "oversized" => made(&format!("ses_{}", "x".repeat(2 * 1024 * 1024)), &here),
            "invalid-id" => made("bad", &here),
            "wrong-dir" => made("ses_abc123", "/elsewhere"),
            _ => {
                let now = SystemClock.now_ms();
                made(
                    &format!("ses_{:x}{creates:x}{now:x}", std::process::id()),
                    &here,
                )
            }
        }
    }
}

/// Starts again under a shell that ignores SIGTERM, which a signal ignored
/// stays across `exec`: std has no way to ignore one, and this crate no unsafe
/// code. The process id stays the one the test was given. Only where there is
/// a signal to ignore; it returns where it need not, or has failed.
#[cfg(unix)]
fn deafen(env: &Env, args: &[String]) -> Option<ExitCode> {
    use std::os::unix::process::CommandExt;
    if env.text("CF_FIXTURE_MODE") != Some("ignore-sigterm")
        || env.text("CF_FIXTURE_DEAF").is_some()
    {
        return None;
    }
    let this = std::env::current_exe().ok()?;
    #[allow(clippy::disallowed_methods)] // The stand-in starts itself.
    let failed = std::process::Command::new("/bin/sh")
        .args(["-c", "trap '' TERM; exec \"$0\" \"$@\""])
        .arg(this)
        .args(args)
        .env("CF_FIXTURE_DEAF", "1")
        .exec();
    let _ = writeln!(std::io::stderr(), "could not ignore SIGTERM: {failed}");
    Some(ExitCode::FAILURE)
}

/// Windows has no signals: a process that is asked to end is ended.
#[cfg(not(unix))]
fn deafen(_env: &Env, _args: &[String]) -> Option<ExitCode> {
    None
}

fn main() -> ExitCode {
    let env = Env::from_process();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let port = args
        .iter()
        .position(|arg| arg == "--port")
        .and_then(|at| args.get(at + 1))
        .and_then(|port| port.parse::<u16>().ok());
    let (Some("serve"), Some(port)) = (args.first().map(String::as_str), port) else {
        return ExitCode::from(2);
    };
    let fixture = Fixture::read(&env);
    if fixture.mode == "early-exit" {
        return ExitCode::FAILURE;
    }
    if let Some(ended) = deafen(&env, &args) {
        return ended;
    }
    let pass = env.text("OPENCODE_SERVER_PASSWORD").unwrap_or_default();
    fixture.leave(
        "startup",
        &json!({
            "pid": std::process::id(),
            "argv": args,
            "cwd": std::env::current_dir().ok().map(|dir| dir.display().to_string()),
            "username": env.text("OPENCODE_SERVER_USERNAME"),
            "hasPassword": !pass.is_empty(),
            "ping": env.text("CF_FIXTURE_PING"),
        })
        .to_string(),
    );
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        return ExitCode::FAILURE;
    };
    let asked = Cell::new(0);
    LocalSet::new().block_on(&runtime, async {
        let Ok(listener) = TcpListener::bind(("127.0.0.1", port)).await else {
            let _ = writeln!(std::io::stderr(), "could not listen on port {port}");
            return ExitCode::FAILURE;
        };
        serve(listener, move |request: &Request| {
            match (request.method.as_str(), request.path()) {
                ("GET", "/global/health") => fixture.health(request),
                ("POST", "/session") => fixture.session(request, &asked),
                _ => Reply::text(404, "nope"),
            }
        })
        .await;
        ExitCode::SUCCESS
    })
}

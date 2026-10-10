//! A stand-in for the `codex` that `cf codex-session` starts, for the supervisor
//! on a window: given `app-server --listen …` it is Codex's server, starting
//! thread A when asked; given `--remote …` it is the TUI, which starts a thread
//! through the broker, reads the broker's `/session` with its token, and exits 7
//! (or waits to be ended). Each run writes what it was given, and what it saw,
//! to `CF_TEST_CODEX_LOG`. `CF_TEST_CODEX_BACKEND` says how the server goes:
//! `fail` at once, `die` once a thread starts; with `CF_TEST_CODEX_QUESTION` it
//! asks its client a question when a thread starts. `CF_TEST_CODEX_TUI=wait`
//! has the TUI wait for its window to end it.
//!
//! The server listens where it is told, as Codex's does: on a Unix socket
//! (`unix://<path>`), or on loopback (`ws://127.0.0.1:0`, Windows'), where it
//! takes a port, says which on its stderr, and lets in only a connection that
//! brings the token whose SHA-256 it was given.

use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::ExitCode;
use std::thread;

use cf_e2e::http::Http;
use cf_e2e::process::own_var;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tungstenite::client::IntoClientRequest;
use tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tungstenite::http::{HeaderValue, StatusCode};
use tungstenite::{Message, WebSocket};

/// The thread the server starts, and the TUI sees.
const THREAD: &str = "01a09094-938f-7fd1-a2d3-315cf92b4559";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "app-server") {
        server(&args)
    } else {
        tui(&args)
    }
}

/// The word after the option `name`.
fn option<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    let at = args.iter().position(|arg| arg == name)?;
    args.get(at + 1).map(String::as_str)
}

/// Writes what this run was given, or saw, to the log the case reads, a JSON
/// line with the process id first. A log that cannot be written is no run.
fn record(entry: Value) {
    let Some(log) = own_var("CF_TEST_CODEX_LOG") else {
        fail("CF_TEST_CODEX_LOG is not set");
    };
    let mut line = json!({ "pid": std::process::id() });
    if let (Some(line), Some(entry)) = (line.as_object_mut(), entry.as_object()) {
        line.extend(
            entry
                .iter()
                .map(|(name, value)| (name.clone(), value.clone())),
        );
    }
    let wrote = OpenOptions::new()
        .append(true)
        .create(true)
        .open(&log)
        .and_then(|mut file| writeln!(file, "{line}"));
    if let Err(failed) = wrote {
        fail(&format!("{log}: {failed}"));
    }
}

/// Says why the run cannot go on, and ends it.
fn fail(why: &str) -> ! {
    let _ = writeln!(std::io::stderr(), "fake-codex: {why}");
    std::process::exit(2)
}

/// The OpenAI API key this run was given, or null: one never reaches Codex.
fn api_key() -> Value {
    own_var("OPENAI_API_KEY").map_or(Value::Null, Value::from)
}

/// Codex's server.
fn server(args: &[String]) -> ExitCode {
    record(json!({ "run": "server", "args": args, "apiKey": api_key() }));
    let how = own_var("CF_TEST_CODEX_BACKEND").unwrap_or_default();
    if how == "fail" {
        let _ = writeln!(std::io::stderr(), "not logged in");
        return ExitCode::FAILURE;
    }
    let behavior = Behavior {
        die: how == "die",
        asks: own_var("CF_TEST_CODEX_QUESTION").is_some_and(|asks| !asks.is_empty()),
    };
    let listen = option(args, "--listen").unwrap_or_default().to_owned();
    match listen.strip_prefix("unix://") {
        Some(path) => on_a_socket(path, behavior),
        None => on_loopback(option(args, "--ws-token-sha256"), behavior),
    }
}

/// How the server goes once a thread starts.
#[derive(Clone, Copy)]
struct Behavior {
    /// It ends.
    die: bool,
    /// It asks its client a question.
    asks: bool,
}

/// Listens on the Unix socket `path`, and serves every connection.
#[cfg(unix)]
fn on_a_socket(path: &str, behavior: Behavior) -> ExitCode {
    let listener = match std::os::unix::net::UnixListener::bind(path) {
        Ok(listener) => listener,
        Err(failed) => fail(&format!("{path}: {failed}")),
    };
    for stream in listener.incoming().flatten() {
        thread::spawn(move || {
            if let Ok(socket) = tungstenite::accept(stream) {
                serve(socket, behavior);
            }
        });
    }
    ExitCode::SUCCESS
}

/// There is no Unix socket to listen on.
#[cfg(not(unix))]
fn on_a_socket(path: &str, _: Behavior) -> ExitCode {
    fail(&format!("no Unix socket here to listen on: {path}"))
}

/// Listens on loopback, on a port it picks, which it says on its stderr, and
/// lets in only a connection whose bearer token has the SHA-256 it was given.
fn on_loopback(hash: Option<&str>, behavior: Behavior) -> ExitCode {
    let listener = match TcpListener::bind("127.0.0.1:0") {
        Ok(listener) => listener,
        Err(failed) => fail(&format!("127.0.0.1:0: {failed}")),
    };
    let port = listener.local_addr().map_or(0, |address| address.port());
    let _ = write!(
        std::io::stderr(),
        "codex app-server (WebSockets)\n  listening on: ws://127.0.0.1:{port}\n"
    );
    let hash = hash.unwrap_or_default().to_owned();
    for stream in listener.incoming().flatten() {
        let hash = hash.clone();
        thread::spawn(move || {
            // The size of the refusal is tungstenite's: its handshake callback answers with it.
            #[allow(clippy::result_large_err)]
            let only_the_token =
                |request: &Request, response: Response| -> Result<Response, ErrorResponse> {
                    let token = request
                        .headers()
                        .get("authorization")
                        .and_then(|value| value.to_str().ok())
                        .and_then(|value| value.strip_prefix("Bearer "));
                    match token {
                        Some(token) if sha256_hex(token) == hash => Ok(response),
                        _ => {
                            let mut refusal = ErrorResponse::new(None);
                            *refusal.status_mut() = StatusCode::UNAUTHORIZED;
                            Err(refusal)
                        }
                    }
                };
            if let Ok(socket) = tungstenite::accept_hdr(stream, only_the_token) {
                serve(socket, behavior);
            }
        });
    }
    ExitCode::SUCCESS
}

/// The SHA-256 of `text` as lowercase hexadecimal.
fn sha256_hex(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Answers a connection's requests: every request with a result, the start of a
/// thread with a thread.
fn serve<S: Read + Write>(mut socket: WebSocket<S>, behavior: Behavior) {
    while let Ok(message) = socket.read() {
        let Message::Text(text) = message else {
            continue;
        };
        let Ok(request) = serde_json::from_str::<Value>(text.as_str()) else {
            continue;
        };
        // A notification has no id, and no answer.
        let Some(id) = request.get("id").filter(|id| !id.is_null()) else {
            continue;
        };
        let start = request["method"] == "thread/start";
        if start {
            record(json!({ "run": "server", "started": request["params"] }));
        }
        let result = if start {
            json!({ "thread": { "id": THREAD, "turns": [], "status": { "type": "idle" } } })
        } else {
            json!({})
        };
        if socket
            .send(Message::text(
                json!({ "id": id, "result": result }).to_string(),
            ))
            .is_err()
        {
            return;
        }
        if start && behavior.die {
            std::process::exit(0);
        }
        // Codex's question tool: asked of its client, which the broker answers from the board.
        if start && behavior.asks {
            let asked = json!({
                "id": "ask-1",
                "method": "item/tool/requestUserInput",
                "params": {
                    "threadId": THREAD,
                    "questions": [{
                        "id": "q",
                        "header": "Which",
                        "question": "Which one?",
                        "options": [{ "label": "a", "description": "first" }],
                    }],
                },
            });
            if socket.send(Message::text(asked.to_string())).is_err() {
                return;
            }
        }
    }
}

/// Codex's TUI: it reaches the server through the broker, its token in its
/// environment.
fn tui(args: &[String]) -> ExitCode {
    let (Some(remote), Some(token_name)) = (args.get(1), args.get(3)) else {
        fail("the TUI is given --remote <url> --remote-auth-token-env <name>");
    };
    let token = own_var(token_name);
    record(json!({ "run": "tui", "args": args, "token": token, "apiKey": api_key() }));
    let bearer = format!("Bearer {}", token.as_deref().unwrap_or("undefined"));
    let Ok(mut request) = remote.as_str().into_client_request() else {
        fail(&format!("{remote} is no address to connect to"));
    };
    let Ok(value) = HeaderValue::from_str(&bearer) else {
        fail("the token cannot be sent as a header");
    };
    request.headers_mut().insert("authorization", value);
    // A connection that is refused leaves nothing for the TUI to do.
    let Ok((mut socket, _)) = tungstenite::connect(request) else {
        return ExitCode::SUCCESS;
    };
    let mut call = |id: u64, method: &str, params: Value| -> bool {
        let sent = socket.send(Message::text(
            json!({ "id": id, "method": method, "params": params }).to_string(),
        ));
        if sent.is_err() {
            return false;
        }
        while let Ok(message) = socket.read() {
            if let Message::Text(text) = message {
                let answer: Value = serde_json::from_str(text.as_str()).unwrap_or(Value::Null);
                if answer["id"] == id {
                    return true;
                }
            }
        }
        false
    };
    if !call(
        1,
        "initialize",
        json!({ "clientInfo": { "name": "codex-tui", "version": "test" } }),
    ) || !call(
        2,
        "thread/start",
        json!({ "ephemeral": false, "threadSource": "user" }),
    ) {
        return ExitCode::SUCCESS;
    }
    let session = Http::new().get(
        &format!("{}/session", remote.replacen("ws:", "http:", 1)),
        Some(&bearer["Bearer ".len()..]),
    );
    match session.and_then(|reply| reply.json()) {
        Ok(session) => record(json!({ "run": "tui", "session": session })),
        Err(failed) => fail(&failed.to_string()),
    }
    if own_var("CF_TEST_CODEX_TUI").as_deref() != Some("wait") {
        return ExitCode::from(7);
    }
    // Waits for its window to end it, or for the connection to go.
    while socket.read().is_ok() {}
    ExitCode::SUCCESS
}

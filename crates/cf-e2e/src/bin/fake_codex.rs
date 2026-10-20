//! A stand-in for the `codex` that `cf codex-session` starts, for the supervisor
//! on a window: given `app-server --listen …` it is Codex's server, starting
//! thread A when asked; given `--remote …` it is the TUI, which starts a thread
//! through the broker, reads the broker's `/session` with its token, and exits 7
//! (or waits to be ended, and starts a turn with each line the human types into
//! its terminal). Given `resume <thread>` the TUI resumes that thread instead, as
//! the real one asks (`thread/resume`, in the shape Codex 0.159.2 sent it): the
//! server goes on in the thread's rollout, which keeps the developer instructions
//! the thread began with, and the window ends with 1 where the server has no
//! rollout of it, as Codex's does: a thread has one only once it has had a turn.
//! Each run writes what it was given, and what it saw, to `CF_TEST_CODEX_LOG`.
//! `CF_TEST_CODEX_BACKEND` says how the server goes: `fail` at once, `die` once a
//! thread starts; with `CF_TEST_CODEX_QUESTION` it asks its client a question when
//! a thread starts; while the file `CF_TEST_CODEX_RESUME_HANGS` names is there, it
//! never answers a resume. `CF_TEST_CODEX_TUI=wait` has the TUI wait for its window
//! to end it.
//!
//! The server listens where it is told, as Codex's does: on a Unix socket
//! (`unix://<path>`), or on loopback (`ws://127.0.0.1:0`, Windows'), where it
//! takes a port, says which on its stderr, and lets in only a connection that
//! brings the token whose SHA-256 it was given.
//!
//! It is also the Codex of a window the daemon opens (`agent`): a TUI given a
//! message starts the first turn with it, the server takes the turns it is handed
//! and writes their rollout where the daemon reads it, and the servers Codex opens
//! with its window run in groups of their own (`servers`). What the daemon asks
//! of the CLI before it opens a window is answered too (`probes`), and what its
//! two halves read of the arguments they were started with is `arguments`.

// The parts are in `fake_codex/`: cargo looks for the modules of a binary's
// root file beside it, where each would be a binary of its own.
#[path = "fake_codex/agent.rs"]
mod agent;
#[path = "fake_codex/arguments.rs"]
mod arguments;
#[path = "fake_codex/probes.rs"]
mod probes;
#[path = "fake_codex/servers.rs"]
mod servers;
#[path = "fake_codex/typed.rs"]
mod typed;

use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::ExitCode;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use cf_e2e::http::Http;
use cf_e2e::process::own_var;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tungstenite::client::IntoClientRequest;
use tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tungstenite::http::{HeaderValue, StatusCode};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

use agent::Agent;
use arguments::{asked, developer_instructions, opening, resume_request};

fn main() -> ExitCode {
    let given: Vec<String> = std::env::args().skip(1).collect();
    let own = std::env::current_exe().ok();
    let args = asked(&given, own.as_deref());
    match args.first().map(String::as_str) {
        Some(servers::SERVER) => servers::serve(&args[1..]),
        Some(servers::WORKER) => servers::work(&args[1..]),
        Some("queue") => probes::queue_help(),
        _ if args.iter().any(|arg| arg == "app-server") => {
            if args.iter().any(|arg| arg == "--listen") {
                server(args)
            } else {
                probes::configuration()
            }
        }
        _ => tui(args),
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
    let agent = match Agent::start(developer_instructions(args)) {
        Ok(agent) => agent,
        Err(failed) => fail(&failed),
    };
    let listen = option(args, "--listen").unwrap_or_default().to_owned();
    match listen.strip_prefix("unix://") {
        Some(path) => on_a_socket(path, behavior, &agent),
        None => on_loopback(option(args, "--ws-token-sha256"), behavior, &agent),
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
fn on_a_socket(path: &str, behavior: Behavior, agent: &Arc<Agent>) -> ExitCode {
    let listener = match std::os::unix::net::UnixListener::bind(path) {
        Ok(listener) => listener,
        Err(failed) => fail(&format!("{path}: {failed}")),
    };
    for stream in listener.incoming().flatten() {
        let agent = Arc::clone(agent);
        thread::spawn(move || {
            if let Ok(socket) = tungstenite::accept(stream) {
                serve(socket, behavior, &agent);
            }
        });
    }
    ExitCode::SUCCESS
}

/// There is no Unix socket to listen on.
#[cfg(not(unix))]
fn on_a_socket(path: &str, _: Behavior, _: &Arc<Agent>) -> ExitCode {
    fail(&format!("no Unix socket here to listen on: {path}"))
}

/// Listens on loopback, on a port it picks, which it says on its stderr, and
/// lets in only a connection whose bearer token has the SHA-256 it was given.
fn on_loopback(hash: Option<&str>, behavior: Behavior, agent: &Arc<Agent>) -> ExitCode {
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
        let (hash, agent) = (hash.clone(), Arc::clone(agent));
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
                serve(socket, behavior, &agent);
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
/// thread with a thread, the resume of one with it where its rollout is (a thread
/// that never had a turn has none, and is answered with Codex's refusal, which
/// names it), and a message handed over (as the start of a turn, or queued behind
/// one) with what Codex answers, which the agent then takes.
fn serve<S: Read + Write>(mut socket: WebSocket<S>, behavior: Behavior, agent: &Arc<Agent>) {
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
        if request["method"] == "thread/resume" && resume_hangs() {
            record(json!({ "run": "server", "unanswered": request["params"]["threadId"] }));
            continue;
        }
        let answer = match request["method"].as_str() {
            Some("thread/start") => result(
                id,
                json!({ "thread": { "id": agent.thread(), "turns": [], "status": { "type": "idle" } } }),
            ),
            Some("thread/resume") => resumed(id, &request["params"], agent),
            Some("turn/start") => {
                hand_over(agent, &request);
                result(id, json!({ "turn": { "id": "turn-1" } }))
            }
            Some("thread/queue/add") => {
                hand_over(agent, &request);
                result(id, json!({ "queuedMessage": { "id": "queue-1" } }))
            }
            _ => result(id, json!({})),
        };
        if socket.send(Message::text(answer.to_string())).is_err() {
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
                    "threadId": agent.thread(),
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

/// Whether a request to resume a thread goes unanswered: while the file
/// `CF_TEST_CODEX_RESUME_HANGS` names is there. A window opened on a thread then
/// never shows it, as a Codex chief opened again after the app's restart never did
/// (zeewin, 2026-10-19), and once the file is gone the next window does.
fn resume_hangs() -> bool {
    own_var("CF_TEST_CODEX_RESUME_HANGS").is_some_and(|file| std::path::Path::new(&file).exists())
}

/// The answer to the request `id`: `result`.
fn result(id: &Value, result: Value) -> Value {
    json!({ "id": id, "result": result })
}

/// The answer to a request to resume the thread `params` name. The server goes on in
/// the thread's rollout, with the developer instructions the thread began with; and
/// says so, as Codex does, where no rollout holds it (a thread that never had a turn).
/// The log has the thread asked for, and the request as the TUI made it.
fn resumed(id: &Value, params: &Value, agent: &Arc<Agent>) -> Value {
    let thread = params["threadId"].as_str().unwrap_or_default();
    record(json!({ "run": "server", "resumed": thread, "request": params }));
    if agent.resume(thread) {
        result(
            id,
            json!({ "thread": { "id": thread, "turns": [], "status": { "type": "idle" } } }),
        )
    } else {
        json!({
            "id": id,
            "error": { "code": -32600, "message": format!("no rollout found for thread id {thread}") },
        })
    }
}

/// The agent takes the text of the message a request hands over, the first of its
/// input.
fn hand_over(agent: &Arc<Agent>, request: &Value) {
    if let Some(text) = request["params"]["input"][0]["text"].as_str() {
        record(json!({ "run": "server", "handedOver": text }));
        agent.take(text);
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
    // It draws its screen, as the real one does: a window that printed nothing is
    // still being drawn to the daemon, which delivers nothing to it until it has
    // held still (a chief's, which has no first message to show it by).
    let _ = writeln!(std::io::stdout(), "fake-codex: the TUI of a window");
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
    // The answer to a request, or none where the connection went first.
    let mut call = |id: u64, method: &str, params: Value| -> Option<Value> {
        let sent = socket.send(Message::text(
            json!({ "id": id, "method": method, "params": params }).to_string(),
        ));
        sent.ok()?;
        while let Ok(message) = socket.read() {
            if let Message::Text(text) = message {
                let answer: Value = serde_json::from_str(text.as_str()).unwrap_or(Value::Null);
                if answer["id"] == id {
                    return Some(answer);
                }
            }
        }
        None
    };
    let opening = opening(args);
    let started = call(
        1,
        "initialize",
        json!({ "clientInfo": { "name": "codex-tui", "version": "test" } }),
    )
    .and_then(|_| match opening.resume {
        Some(thread) => call(2, "thread/resume", resume_request(thread)),
        None => call(
            2,
            "thread/start",
            json!({ "ephemeral": false, "threadSource": "user" }),
        ),
    });
    let Some(started) = started else {
        return ExitCode::SUCCESS;
    };
    // A window resumed on a thread the server has no rollout of ends at once, as
    // Codex's does: there is nothing to show of it.
    if let Some(refused) = started["error"]["message"].as_str() {
        record(json!({ "run": "tui", "refused": refused }));
        let _ = writeln!(std::io::stderr(), "fake-codex: {refused}");
        return ExitCode::FAILURE;
    }
    let session = Http::new().get(
        &format!("{}/session", remote.replacen("ws:", "http:", 1)),
        Some(&bearer["Bearer ".len()..]),
    );
    match session.and_then(|reply| reply.json()) {
        Ok(session) => record(json!({ "run": "tui", "session": session })),
        Err(failed) => fail(&failed.to_string()),
    }
    // What it draws once it shows a thread, which a window with nothing in its
    // record reads as drawn when it has held still.
    let thread = started["result"]["thread"]["id"].clone();
    let shows = if opening.resume.is_some() {
        "resumed"
    } else {
        "started"
    };
    let _ = writeln!(
        std::io::stdout(),
        "fake codex {shows} on {}",
        thread.as_str().unwrap_or_default()
    );
    // A window opened with a message begins its first turn with it.
    if let Some(message) = opening.message {
        let input = json!([{ "type": "text", "text": message, "text_elements": [] }]);
        call(
            3,
            "turn/start",
            json!({ "threadId": thread, "input": input }),
        );
    }
    if own_var("CF_TEST_CODEX_TUI").as_deref() != Some("wait") {
        return ExitCode::from(7);
    }
    // Waits for its window to end it, or for the connection to go, and starts a turn
    // with each line the human types into its terminal as it waits.
    if let MaybeTlsStream::Plain(stream) = socket.get_mut() {
        let _ = stream.set_read_timeout(Some(Duration::from_millis(100)));
    }
    let typed = typed::typed();
    let mut asked = 3;
    loop {
        match socket.read() {
            Ok(_) => {}
            Err(tungstenite::Error::Io(failed)) if is_a_wait(&failed) => {}
            Err(_) => return ExitCode::SUCCESS,
        }
        while let Ok(text) = typed.try_recv() {
            asked += 1;
            let input = json!([{ "type": "text", "text": text, "text_elements": [] }]);
            let turn = json!({
                "id": asked,
                "method": "turn/start",
                "params": { "threadId": thread, "input": input },
            });
            if socket.send(Message::text(turn.to_string())).is_err() {
                return ExitCode::SUCCESS;
            }
        }
    }
}

/// Whether a read failed for want of anything to read in the time it was given,
/// which is no end of the connection.
fn is_a_wait(failed: &std::io::Error) -> bool {
    matches!(
        failed.kind(),
        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
    )
}

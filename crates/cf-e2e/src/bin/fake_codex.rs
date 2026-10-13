//! A stand-in for the `codex` that `cf codex-session` starts, for the supervisor
//! on a window: given `app-server --listen …` it is Codex's server, starting
//! thread A when asked; given `--remote …` it is the TUI, which starts a thread
//! through the broker, reads the broker's `/session` with its token, and exits 7
//! (or waits to be ended). Given `resume <thread>` it resumes that one instead,
//! and ends with 1 where the server has no rollout of it, as Codex's does: a
//! thread has one only once it has had a turn. Each run writes what it was given,
//! and what it saw, to `CF_TEST_CODEX_LOG`. `CF_TEST_CODEX_BACKEND` says how the
//! server goes:
//! `fail` at once, `die` once a thread starts; with `CF_TEST_CODEX_QUESTION` it
//! asks its client a question when a thread starts. `CF_TEST_CODEX_TUI=wait`
//! has the TUI wait for its window to end it.
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
//! of the CLI before it opens a window is answered too (`probes`).

// The parts are in `fake_codex/`: cargo looks for the modules of a binary's
// root file beside it, where each would be a binary of its own.
#[path = "fake_codex/agent.rs"]
mod agent;
#[path = "fake_codex/probes.rs"]
mod probes;
#[path = "fake_codex/servers.rs"]
mod servers;

use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::ExitCode;
use std::sync::Arc;
use std::thread;

use cf_e2e::http::Http;
use cf_e2e::process::own_var;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tungstenite::client::IntoClientRequest;
use tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tungstenite::http::{HeaderValue, StatusCode};
use tungstenite::{Message, WebSocket};

use agent::Agent;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some(servers::SERVER) => servers::serve(&args[1..]),
        Some(servers::WORKER) => servers::work(&args[1..]),
        Some("queue") => probes::queue_help(),
        _ if args.iter().any(|arg| arg == "app-server") => {
            if args.iter().any(|arg| arg == "--listen") {
                server(&args)
            } else {
                probes::configuration()
            }
        }
        _ => tui(&args),
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
    let agent = match Agent::start() {
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
/// that never had a turn has none, and is answered with an error that names it),
/// and a message handed over (as the start of a turn, or queued behind one) with
/// what Codex answers, which the agent then takes.
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
        let answer = match request["method"].as_str() {
            Some("thread/start") => Ok(
                json!({ "thread": { "id": agent.thread(), "turns": [], "status": { "type": "idle" } } }),
            ),
            Some("thread/resume") => resumed(agent, &request),
            Some("turn/start") => {
                hand_over(agent, &request);
                Ok(json!({ "turn": { "id": "turn-1" } }))
            }
            Some("thread/queue/add") => {
                hand_over(agent, &request);
                Ok(json!({ "queuedMessage": { "id": "queue-1" } }))
            }
            _ => Ok(json!({})),
        };
        let answered = match answer {
            Ok(result) => json!({ "id": id, "result": result }),
            Err(why) => json!({ "id": id, "error": { "code": -32600, "message": why } }),
        };
        if socket.send(Message::text(answered.to_string())).is_err() {
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

/// The agent takes the text of the message a request hands over, the first of its
/// input.
fn hand_over(agent: &Arc<Agent>, request: &Value) {
    if let Some(text) = request["params"]["input"][0]["text"].as_str() {
        record(json!({ "run": "server", "handedOver": text }));
        agent.take(text);
    }
}

/// The thread a request resumes, where the agent has its rollout, and the error
/// that names it where it has none.
fn resumed(agent: &Arc<Agent>, request: &Value) -> Result<Value, String> {
    let thread = request["params"]["threadId"].as_str().unwrap_or_default();
    record(json!({ "run": "server", "resumed": thread }));
    if agent.resume(thread) {
        Ok(json!({ "thread": { "id": thread, "turns": [], "status": { "type": "idle" } } }))
    } else {
        Err(format!("no rollout found for thread id {thread}"))
    }
}

/// What a TUI was given to start with, after the options the broker adds
/// (`--remote <url> --remote-auth-token-env <name>`).
#[derive(Debug, Default, PartialEq, Eq)]
struct Launched<'a> {
    /// The thread it is to resume (`resume <thread>`).
    resume: Option<&'a str>,
    /// The message it starts with: the word that is no flag and no value of one.
    message: Option<&'a str>,
}

/// What the TUI's arguments `args` say of the thread and the message it starts with.
fn launched(args: &[String]) -> Launched<'_> {
    let mut launched = Launched::default();
    let mut words = args.get(4..).unwrap_or_default().iter();
    while let Some(word) = words.next() {
        match word.as_str() {
            "-c" | "--config" | "--enable" | "--disable" | "--model" | "-m" => {
                words.next();
            }
            flag if flag.starts_with('-') => {}
            "resume" if launched.resume.is_none() => {
                launched.resume = words.next().map(String::as_str);
            }
            message => {
                launched.message = Some(message);
                break;
            }
        }
    }
    launched
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
    let launched = launched(args);
    let started = call(
        1,
        "initialize",
        json!({ "clientInfo": { "name": "codex-tui", "version": "test" } }),
    )
    .and_then(|_| match launched.resume {
        // Its workspace roots are null, as Codex's TUI says them since 0.159.
        Some(thread) => call(
            2,
            "thread/resume",
            json!({ "threadId": thread, "runtimeWorkspaceRoots": null }),
        ),
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
    let shows = if launched.resume.is_some() {
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
    if let Some(message) = launched.message {
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
    // Waits for its window to end it, or for the connection to go.
    while socket.read().is_ok() {}
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The arguments of a TUI the broker added its own options to.
    fn tui_args(rest: &str) -> Vec<String> {
        format!("--remote ws://127.0.0.1:1 --remote-auth-token-env TOKEN {rest}")
            .split(' ')
            .filter(|word| !word.is_empty())
            .map(str::to_owned)
            .collect()
    }

    #[test]
    fn a_tui_starts_with_the_message_it_is_given_and_with_none_where_it_is_given_none() {
        let args = tui_args("-c a=b --model fake --enable x HELLO");
        assert_eq!(
            launched(&args),
            Launched {
                resume: None,
                message: Some("HELLO")
            }
        );
        assert_eq!(
            launched(&tui_args("-c a=b --model fake")),
            Launched::default()
        );
        assert_eq!(launched(&[]), Launched::default());
    }

    #[test]
    fn a_tui_told_to_resume_names_the_thread_and_may_have_a_message_after_it() {
        let args = tui_args("-c a=b resume 01a0 --model fake");
        assert_eq!(
            launched(&args),
            Launched {
                resume: Some("01a0"),
                message: None
            }
        );
        let args = tui_args("resume 01a0 --model fake Go on");
        assert_eq!(
            launched(&args),
            Launched {
                resume: Some("01a0"),
                message: Some("Go")
            }
        );
        // A resume that names no thread resumes none.
        assert_eq!(launched(&tui_args("resume")).resume, None);
    }
}

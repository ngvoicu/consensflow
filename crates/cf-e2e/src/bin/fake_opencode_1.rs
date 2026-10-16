//! A stand-in for the `opencode` 1.x a window runs, in the integration tests: as
//! much of OpenCode's server as the daemon's launch of a window asks of it, so
//! that a case sees what the daemon launched a window on. It says `1.19.0` to
//! `--version`. `serve` (the throwaway server the launch makes the window's
//! conversation on) answers the server's health and makes one empty conversation
//! in the folder the request names. Any other launch is the window: its server
//! answers its health and takes the first message (`prompt_async`, which OpenCode
//! answers 204). It writes down, in `CF_TEST_OPENCODE_LOG`, the arguments of each
//! launch (a JSON list to a line) and each request either server is sent (a JSON
//! object: `method`, `target`, `body`), and serves until it is stopped.
//!
//! It is no OpenCode: there is no TUI and no plugin, so a window on it never says
//! which conversation it shows, and nothing answers a message. The cases on it are
//! the daemon's side of a launch, which is the part that is code.

use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::Path;
use std::process::ExitCode;
use std::thread::sleep;
use std::time::Duration;

use cf_e2e::http::server::{Answer, Request, Response, Server};
use cf_e2e::process::own_var;
use serde_json::{json, Value};

/// What an OpenCode 1.x says to `--version`.
const VERSION: &str = "1.19.0\n";

/// The conversation the throwaway server makes, in the shape of OpenCode's ids.
const SESSION: &str = "ses_standin1";

/// Whether `arg` names the stand-in's own file (`own`), by its file name: the
/// script of the shim that opened it, where the system wraps a program in one.
fn is_own_file(arg: &str, own: Option<&Path>) -> bool {
    own.and_then(Path::file_name).is_some_and(|mine| {
        Path::new(arg)
            .file_name()
            .is_some_and(|name| name.eq_ignore_ascii_case(mine))
    })
}

/// What the stand-in was asked, without its own file where a shim put it first:
/// on Windows a program is started as `<program> <script> <arguments>`, and the
/// shim names the stand-in as both.
fn asked<'a>(args: &'a [String], own: Option<&Path>) -> &'a [String] {
    match args.split_first() {
        Some((first, rest)) if is_own_file(first, own) => rest,
        _ => args,
    }
}

/// The word after `flag` in `args`.
fn after<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    let at = args.iter().position(|word| word == flag)?;
    args.get(at + 1).map(String::as_str)
}

/// `%XX` in `text` as the byte it is: the folder a conversation is made in,
/// which the daemon names with `encodeURIComponent`.
fn decoded(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        let escaped = (bytes[at] == b'%')
            .then(|| text.get(at + 1..at + 3))
            .flatten()
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        match escaped {
            Some(byte) => {
                out.push(byte);
                at += 3;
            }
            None => {
                out.push(bytes[at]);
                at += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// What the server of a launch answers `request` (`serving` for the throwaway
/// `serve`, which makes the conversation; the window's takes the first message).
fn answer(request: &Request, serving: bool) -> Response {
    let path = request.path();
    match (request.method.as_str(), path) {
        ("GET", "/global/health") => {
            Response::json(200, &json!({ "healthy": true, "version": VERSION.trim() }))
        }
        ("POST", "/session") if serving => Response::json(
            200,
            &json!({ "id": SESSION, "directory": decoded(request.query("directory").unwrap_or_default()) }),
        ),
        ("POST", _) if !serving && path.ends_with("/prompt_async") => Response::status(204),
        _ => Response::status(404),
    }
}

/// Writes `line` down in `log`, a line of its own.
fn write_down(log: &str, line: &Value) -> io::Result<()> {
    let mut file = OpenOptions::new().create(true).append(true).open(log)?;
    writeln!(file, "{line}")
}

/// The request as it is written down.
fn request_line(request: &Request) -> Value {
    json!({ "method": request.method, "target": request.target, "body": request.body })
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let own = std::env::current_exe().ok();
    let args = asked(&args, own.as_deref()).to_vec();
    let log = own_var("CF_TEST_OPENCODE_LOG");
    if let Some(log) = &log {
        if let Err(failed) = write_down(log, &json!(args)) {
            let _ = writeln!(io::stderr(), "fake-opencode-1: {log}: {failed}");
            return ExitCode::FAILURE;
        }
    }
    if matches!(args.as_slice(), [only] if only == "--version") {
        return match io::stdout().lock().write_all(VERSION.as_bytes()) {
            Ok(()) => ExitCode::SUCCESS,
            Err(_) => ExitCode::FAILURE,
        };
    }
    let serving = args.first().is_some_and(|first| first == "serve");
    let host = after(&args, "--hostname").unwrap_or("127.0.0.1").to_owned();
    let Some(port) = after(&args, "--port").and_then(|port| port.parse::<u16>().ok()) else {
        let _ = writeln!(io::stderr(), "fake-opencode-1: no --port in {args:?}");
        return ExitCode::FAILURE;
    };
    let served = Server::bound((host.as_str(), port), move |request| {
        if let Some(log) = &log {
            let _ = write_down(log, &request_line(request));
        }
        Answer::Respond(answer(request, serving))
    });
    let _server = match served {
        Ok(server) => server,
        Err(failed) => {
            let _ = writeln!(io::stderr(), "fake-opencode-1: {host}:{port}: {failed}");
            return ExitCode::FAILURE;
        }
    };
    loop {
        sleep(Duration::from_secs(3600));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(line: &str) -> Vec<String> {
        line.split(' ')
            .filter(|word| !word.is_empty())
            .map(str::to_owned)
            .collect()
    }

    fn request(method: &str, target: &str) -> Request {
        Request {
            method: method.to_owned(),
            target: target.to_owned(),
            headers: Vec::new(),
            body: String::new(),
        }
    }

    #[test]
    fn the_folder_a_conversation_is_made_in_is_read_as_the_daemon_escaped_it() {
        assert_eq!(decoded("%2FUsers%2Fme%2Fa%20b"), "/Users/me/a b");
        assert_eq!(decoded("C%3A%5Cwork"), "C:\\work");
        // What is no escape is itself.
        assert_eq!(decoded("a%2"), "a%2");
        assert_eq!(decoded("a%zzb"), "a%zzb");
        assert_eq!(decoded("%C3%A9"), "é");
    }

    #[test]
    fn the_throwaway_server_makes_the_conversation_and_the_windows_takes_the_first_message() {
        let health = answer(&request("GET", "/global/health"), true);
        assert_eq!(health.status, 200);
        let made = answer(&request("POST", "/session?directory=%2Fwork"), true);
        assert_eq!(made.status, 200);
        assert_eq!(
            serde_json::from_str::<Value>(&made.body).unwrap(),
            json!({ "id": SESSION, "directory": "/work" })
        );
        let prompt = "/session/ses_standin1/prompt_async?directory=%2Fwork";
        assert_eq!(answer(&request("POST", prompt), false).status, 204);
        assert_eq!(answer(&request("GET", "/global/health"), false).status, 200);
        // Each server does its own part alone.
        assert_eq!(answer(&request("POST", "/session"), false).status, 404);
        assert_eq!(answer(&request("POST", prompt), true).status, 404);
    }

    #[test]
    fn a_launch_is_read_for_its_server_and_the_stand_in_a_shim_names_is_no_argument() {
        let args = words("serve --port 41001 --hostname 127.0.0.1");
        assert_eq!(after(&args, "--port"), Some("41001"));
        assert_eq!(after(&args, "--hostname"), Some("127.0.0.1"));
        assert_eq!(after(&args, "--model"), None);
        let own = Some(Path::new("/bin/fake-opencode-1"));
        let shimmed = words("C:/x/FAKE-OPENCODE-1 --version");
        assert_eq!(asked(&shimmed, own), ["--version"]);
        assert_eq!(asked(&args, own), args.as_slice());
    }
}

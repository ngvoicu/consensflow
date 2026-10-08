//! The empty conversation a fresh window opens on, made on a throwaway `serve`
//! before the window opens, so its id is known. The child serves the launch's
//! own endpoint just long enough to answer an authenticated `/global/health`
//! and a single `POST /session` with `{}`: no model task, no title. It is
//! stopped BEFORE the id returns, so the window's own server can take the port,
//! and on every failure too. The whole of it, from the start, is bounded by 15
//! s; only the health polls are tried again. What the child says on its error
//! stream is never kept.
//!
//! Node learns nothing of its child before its first wait: a start that
//! failed is an `error` event that comes after the code that follows the
//! spawn, and a child that exits at once says so after it too. So the first
//! poll is made whatever became of the child, and a start that failed is
//! seen from the second.

use std::path::PathBuf;

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use cf_base::env::Env;
use cf_base::json::from_slice_lossy;
use serde_json::Value;

use super::directory::real_path;
use super::{is_session_id, succeeded, url, Launched, Wires, OVERSIZED};
use crate::seams::arm;
use crate::seams::loopback::{BodyFailed, Method, Request};
use crate::seams::processes::{Child, Program, Streams};
use crate::shared::child::stop;

/// How long the whole of it has, from the start, for a window
/// (`createSession`'s `timeoutMs`).
pub const TIMEOUT_MS: i64 = 15_000;

/// How long one health poll may take at most.
const POLL_MS: i64 = 500;

/// How long a poll waits before the next at most.
const RETRY_MS: i64 = 100;

/// The most the answer to a poll may say, whose words are never kept.
const POLL_LIMIT: usize = 64 * 1024;

/// The most the new session's record may say.
const SESSION_LIMIT: usize = 1024 * 1024;

/// What a throwaway server is run for.
pub struct Serve<'a> {
    pub executable: &'a str,
    /// The folder the window will work in, which the server runs in.
    pub directory: &'a str,
    /// The environment the server inherits: the engine's, with what the
    /// window's role adds.
    pub env: &'a Env,
    pub launched: &'a Launched,
    /// How long the whole of it has, from the start: [`TIMEOUT_MS`] for a
    /// window.
    pub timeout_ms: i64,
}

/// Makes one empty conversation in `serve.directory` on a server that is
/// stopped before its id returns: the id, or why there is none.
pub async fn create_session(wires: Wires<'_>, serve: &Serve<'_>) -> Result<String, String> {
    let canonical = real_path(serve.directory)
        .map_err(|_| "opencode session needs a working directory".to_owned())?;
    let deadline = wires.time.wall_ms().saturating_add(serve.timeout_ms);
    // A child that did not start is none, and its failure is all it says.
    let child = wires
        .processes
        .spawn(program(serve, &canonical), Streams::Quiet)
        .ok();
    let outcome = async {
        let id = session(wires, serve, &canonical, deadline, child.as_deref()).await?;
        if let Some(child) = child.as_deref() {
            stop(wires.time, child).await?;
        }
        Ok(id)
    }
    .await;
    if outcome.is_err() {
        if let Some(child) = child.as_deref() {
            // A server that will not stop is no more than it was.
            let _ = stop(wires.time, child).await;
        }
    }
    outcome
}

/// The server's program: `opencode serve` on the launch's port, in the folder
/// the window works in, as the launch's environment makes it.
fn program(serve: &Serve<'_>, canonical: &str) -> Program {
    let inherited = serve
        .env
        .iter()
        .map(|(name, value)| (name.to_owned(), value.to_owned()));
    let told = serve
        .launched
        .env
        .iter()
        .map(|(name, value)| (name.into(), value.into()));
    let username = [("OPENCODE_SERVER_USERNAME".into(), "opencode".into())];
    let args = std::iter::once("serve".to_owned())
        .chain(serve.launched.args.iter().cloned())
        .collect();
    Program {
        executable: PathBuf::from(serve.executable),
        args,
        cwd: Some(PathBuf::from(canonical)),
        env: Env::from_vars(inherited.chain(told).chain(username)),
    }
}

/// Whether the server's process is gone, or never started.
fn failed_early(child: Option<&dyn Child>) -> Result<(), String> {
    match child {
        None => Err("opencode serve failed to start".to_owned()),
        Some(child) if child.exited() => Err("opencode serve exited early".to_owned()),
        Some(_) => Ok(()),
    }
}

/// Waits for the server to be healthy, and asks it for a session: the id,
/// once it names this folder.
async fn session(
    wires: Wires<'_>,
    serve: &Serve<'_>,
    canonical: &str,
    deadline: i64,
    child: Option<&dyn Child>,
) -> Result<String, String> {
    let channel = &serve.launched.channel;
    let credentials = STANDARD.encode(format!("opencode:{}", channel.password));
    let authorization = ("authorization".to_owned(), format!("Basic {credentials}"));
    let left = || deadline - wires.time.wall_ms();
    let mut first = true;
    let mut healthy = false;
    while left() > 0 {
        if !std::mem::take(&mut first) {
            failed_early(child)?;
        }
        let poll = arm(wires.time, left().clamp(1, POLL_MS).unsigned_abs());
        let request = Request {
            method: Method::Get,
            url: format!("{}/global/health", channel.endpoint),
            headers: vec![authorization.clone()],
            body: None,
        };
        let status = match poll.bound(wires.loopback.send(request)).await {
            Some(Ok(mut reply)) => {
                let status = reply.status();
                // What it says is read and let go, whatever comes of it.
                let _ = poll
                    .bound(reply.body(if status == 401 {
                        usize::MAX
                    } else {
                        POLL_LIMIT
                    }))
                    .await;
                Some(status)
            }
            Some(Err(_)) | None => None,
        };
        drop(poll);
        match status {
            Some(401) => return Err("opencode session unauthorized".to_owned()),
            Some(status) if succeeded(status) => {
                healthy = true;
                break;
            }
            _ => wires.sleep(left().clamp(1, RETRY_MS).unsigned_abs()).await,
        }
    }
    if !healthy {
        failed_early(child)?;
        return Err("opencode session timed out".to_owned());
    }
    let remaining = left();
    if remaining <= 0 {
        return Err("opencode session timed out".to_owned());
    }
    let request = Request {
        method: Method::Post,
        url: url::creation(&channel.endpoint, canonical),
        headers: vec![
            authorization,
            ("content-type".to_owned(), "application/json".to_owned()),
        ],
        body: Some(b"{}".to_vec()),
    };
    let timeout = arm(wires.time, remaining.unsigned_abs());
    let mut reply = match timeout.bound(wires.loopback.send(request)).await {
        None => return Err("opencode session timed out".to_owned()),
        Some(Err(_)) => return Err("opencode session transport failed".to_owned()),
        Some(Ok(reply)) => reply,
    };
    let status = reply.status();
    if status == 401 {
        let _ = timeout.bound(reply.body(usize::MAX)).await;
        return Err("opencode session unauthorized".to_owned());
    }
    if !succeeded(status) {
        return Err(format!("opencode session rejected with status {status}"));
    }
    let bytes = match timeout.bound(reply.body(SESSION_LIMIT)).await {
        Some(Ok(bytes)) => bytes,
        Some(Err(BodyFailed::TooLarge)) => return Err(OVERSIZED.to_owned()),
        Some(Err(BodyFailed::Cut)) | None => {
            return Err("opencode session returned an unreadable response".to_owned());
        }
    };
    let body = from_slice_lossy(&bytes)
        .map_err(|_| "opencode session returned invalid JSON".to_owned())?;
    let id = body
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| is_session_id(id))
        .ok_or_else(|| "opencode session returned an invalid id".to_owned())?;
    if body.get("directory").and_then(Value::as_str) != Some(canonical) {
        return Err("opencode session returned the wrong directory".to_owned());
    }
    Ok(id.to_owned())
}

//! The channel to a Codex window's broker (`src/channels/codex.js`, and the
//! `codex` branch of `launchConfiguration`, `src/channels.js`). A Codex window
//! runs under ConsensFlow's supervisor, whose broker on loopback knows the
//! thread the window's TUI shows and queues a message on it: the only way into
//! the window. It answers `GET /session` with what the window shows
//! ([`Channel::shown`]) and takes a message at `POST /deliver` ([`send`]).

mod send;

use cf_base::js;
use cf_base::json::from_slice_lossy;
use serde_json::Value;
use url::Url;

use crate::seams::loopback::{Loopback, Method, Request};
use crate::seams::{arm, Time};

pub use send::{send, Answer, Target};

/// How long the broker has to say what the window shows, over the request and
/// its body.
const SESSION_TIMEOUT_MS: u64 = 1_000;

/// The most a reply of the broker's may say: a few hundred bytes, in practice.
/// Node read a reply of any size.
const BODY_LIMIT: usize = 1024 * 1024;

/// A launch's broker: where it listens and the token it is asked with, and
/// the launch it serves.
#[derive(Debug, Clone)]
pub struct Channel {
    launch_id: String,
    endpoint: String,
    token: String,
}

/// What the broker says of the window (`sessionState`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shown {
    pub session: Session,
    /// Whether it would take a delivery now: a thread shown, no switch, its
    /// app-server connected.
    pub available: bool,
}

/// The thread the window's TUI shows (`sessionId`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Session {
    /// None yet: the window starts, switches threads or has no TUI attached.
    Unnamed,
    Thread(String),
    /// An id in a list. JavaScript's pattern test read the list as the id,
    /// and nothing else does: it is never the window's thread.
    Wrapped,
}

impl Session {
    /// Whether it is `thread`, as `===` compares them: a window with no
    /// thread yet shows none.
    pub(crate) fn is(&self, thread: Option<&str>) -> bool {
        match (self, thread) {
            (Session::Unnamed, None) => true,
            (Session::Thread(shown), Some(thread)) => shown == thread,
            _ => false,
        }
    }
}

/// Whether `text` is a thread's id: a UUID of version 1 to 8 (`UUID`,
/// `src/channels/codex.js`), in either case.
pub(crate) fn is_thread(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 36
        && bytes.iter().enumerate().all(|(at, &byte)| match at {
            8 | 13 | 18 | 23 => byte == b'-',
            14 => (b'1'..=b'8').contains(&byte),
            19 => matches!(byte, b'8' | b'9' | b'a' | b'b' | b'A' | b'B'),
            _ => byte.is_ascii_hexdigit(),
        })
}

/// The JSON of a reply's body as `response.json()` reads it: decoded as
/// UTF-8 with a byte order mark taken off.
fn parse(body: &[u8]) -> Option<Value> {
    let body = body.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(body);
    from_slice_lossy(body).ok()
}

impl Channel {
    pub fn new(launch_id: &str, endpoint: String, token: String) -> Self {
        Self {
            launch_id: launch_id.to_owned(),
            endpoint,
            token,
        }
    }

    /// A request to the broker, bearing the launch's token: a `GET`, or a
    /// `POST` of `body` as JSON. Its URL is written as `new URL(route,
    /// endpoint)` writes it.
    fn request(&self, route: &str, body: Option<String>) -> Option<Request> {
        let url = Url::parse(&self.endpoint).ok()?.join(route).ok()?;
        let mut headers = vec![("authorization".to_owned(), format!("Bearer {}", self.token))];
        let method = if body.is_some() {
            headers.push(("content-type".to_owned(), "application/json".to_owned()));
            Method::Post
        } else {
            Method::Get
        };
        Some(Request {
            method,
            url: url.into(),
            headers,
            body: body.map(String::into_bytes),
        })
    }

    /// The broker's word on the window (`sessionState`), or none where it does
    /// not answer for this launch: nothing answered within a second, the
    /// answer was a refusal, was no JSON, was another launch's, or named a
    /// thread that is no id.
    pub async fn shown(&self, time: &dyn Time, loopback: &dyn Loopback) -> Option<Shown> {
        let request = self.request("/session", None)?;
        let attempt = arm(time, SESSION_TIMEOUT_MS);
        let mut reply = attempt.bound(loopback.send(request)).await?.ok()?;
        let status = reply.status();
        let body = attempt.bound(reply.body(BODY_LIMIT)).await?.ok()?;
        let current = parse(&body)?;
        let ours = current.get("launchId").and_then(Value::as_str) == Some(&self.launch_id);
        if !(200..300).contains(&status) || !ours {
            return None;
        }
        let session = match current.get("sessionId")? {
            Value::Null => Session::Unnamed,
            Value::String(text) if is_thread(text) => Session::Thread(text.clone()),
            other if !other.is_string() && is_thread(&js::text(Some(other))) => Session::Wrapped,
            _ => return None,
        };
        Some(Shown {
            session,
            available: current.get("available") == Some(&Value::Bool(true)),
        })
    }
}

#[cfg(test)]
mod tests;

//! A request's body and the two ways it is read. **Frozen**.
//!
//! Both count what has come as it comes and refuse as soon as it is too
//! much, never when it is all there: a body that never ends is refused at its
//! limit, and nothing waits for the rest.
//!
//! - [`read_json`], for the agents' API (`readJson`, `api.js:484-501`): at
//!   most 2 MiB of bytes; nothing is an empty object; anything but one JSON
//!   object is refused.
//! - [`read_text`], for the human's screens (`readBody`,
//!   `agents-server.js:39-49`): at most 64 K UTF-16 code units of text, each
//!   chunk read as text on its own as Node read it, so what is cut between
//!   two chunks reads as U+FFFD.

use std::future::Future;
use std::io;
use std::pin::Pin;
use std::time::Duration;

use bytes::Bytes;
use cf_base::json::from_slice_lossy;
use futures_util::stream::{self, Stream, StreamExt};
use http_body_util::BodyExt;
use hyper::body::Incoming;
use serde_json::{Map, Value};

use super::answer::Failure;

/// The most an agents' request may hold: 2 MiB.
pub const MAX_JSON_BYTES: usize = 2 * 1024 * 1024;

/// The most a screen's request may hold: 64 K UTF-16 code units.
pub const MAX_TEXT_UNITS: usize = 64 * 1024;

/// How much of a body is read and let go once its request was answered before
/// all of it was read: 16 MiB.
pub const DRAIN_BYTES: usize = 16 * 1024 * 1024;

/// How long the rest of such a body is read: two seconds.
pub const DRAIN_WAIT: Duration = Duration::from_secs(2);

/// A request's body, the bytes as they arrive.
pub struct Body {
    chunks: Pin<Box<dyn Stream<Item = io::Result<Bytes>>>>,
}

impl Body {
    /// A body of these chunks, each as the connection brought it: what hyper's
    /// is, and what a test makes to hold a reader to.
    pub fn new(chunks: impl Stream<Item = io::Result<Bytes>> + 'static) -> Self {
        Self {
            chunks: Box::pin(chunks),
        }
    }

    /// A request that has no body.
    pub fn empty() -> Self {
        Self::new(stream::empty())
    }

    /// The body hyper reads off a connection, through a pump that is spawned
    /// beside the request: the data frames come through it as the handler
    /// reads them, a failure of the connection as an error, and then no more.
    /// Once the handler lets the body go, answered before it read all of it (a
    /// check that comes before the body, a body over its limit), the pump reads
    /// what is left and lets it go, [`DRAIN_BYTES`] at most and for
    /// [`DRAIN_WAIT`] at most, as Node's server read the rest once a response
    /// ended: a client still sending reads its answer, where Windows resets a
    /// socket closed with bytes unread, and the answer with it (seen on zeewin).
    /// `arrived` is called once a chunk was handed over, as the connection's
    /// own reads are followed: what the chunk woke is run there, a callback of
    /// its own.
    pub fn pumped(
        incoming: Incoming,
        arrived: impl Fn() + 'static,
    ) -> (Self, impl Future<Output = ()> + 'static) {
        let (sender, mut received) = tokio::sync::mpsc::channel(1);
        let pump = async move {
            let mut incoming = incoming;
            while let Some(frame) = incoming.frame().await {
                let chunk = match frame {
                    Err(failed) => Err(io::Error::other(failed)),
                    Ok(frame) => match frame.into_data() {
                        Ok(data) => Ok(data),
                        Err(_) => continue,
                    },
                };
                let broke = chunk.is_err();
                if sender.send(chunk).await.is_err() {
                    let_go(&mut incoming).await;
                    return;
                }
                arrived();
                if broke {
                    return;
                }
            }
        };
        let body = Self::new(stream::poll_fn(move |context| received.poll_recv(context)));
        (body, pump)
    }

    /// The next chunk: none once the body ended.
    pub async fn next(&mut self) -> Option<io::Result<Bytes>> {
        self.chunks.next().await
    }
}

/// Reads what is left of a body nobody reads any more, and lets it go: to its
/// end, a failure, [`DRAIN_BYTES`], or [`DRAIN_WAIT`], whichever comes first.
async fn let_go(incoming: &mut Incoming) {
    let rest = async {
        let mut read = 0;
        while let Some(Ok(frame)) = incoming.frame().await {
            if let Ok(data) = frame.into_data() {
                read += data.len();
                if read > DRAIN_BYTES {
                    return;
                }
            }
        }
    };
    // Past the wait the rest is left unread: a body that never ends.
    let _ = tokio::time::timeout(DRAIN_WAIT, rest).await;
}

/// The agents' API's body, as the one JSON object it is (`readJson`).
///
/// Over 2 MiB is 413 `too-large` as the chunk that passes it comes; no body
/// at all is `{}`; a body that is no JSON, or JSON that is no object (an
/// array, a text, `null`), is 400 `invalid-json`. JSON is read as `JSON.parse`
/// reads it, so its keys keep their order. A connection that failed while the
/// body came is the failure it said.
pub async fn read_json(body: &mut Body) -> Result<Map<String, Value>, Failure> {
    let mut bytes: Vec<u8> = Vec::new();
    while let Some(chunk) = body.next().await {
        let chunk = chunk.map_err(|failed| Failure::Internal(failed.to_string()))?;
        if bytes.len() + chunk.len() > MAX_JSON_BYTES {
            return Err(Failure::refuse(
                413,
                "too-large",
                "the request is larger than 2 MB",
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    if bytes.is_empty() {
        return Ok(Map::new());
    }
    match from_slice_lossy(&bytes) {
        Ok(Value::Object(fields)) => Ok(fields),
        _ => Err(Failure::refuse(
            400,
            "invalid-json",
            "the request body must be a JSON object",
        )),
    }
}

/// Why a screen's body was not read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unread {
    /// More than 64 K UTF-16 code units came.
    TooLarge,
    /// The connection failed while the body came, with what it said.
    Broke(String),
}

impl Unread {
    /// The words the screens answer with (`error.message`).
    pub fn message(&self) -> &str {
        match self {
            Self::TooLarge => "body too large",
            Self::Broke(words) => words,
        }
    }
}

/// A screen's body, as text (`readBody`): empty when there is none.
pub async fn read_text(body: &mut Body) -> Result<String, Unread> {
    let mut text = String::new();
    let mut units = 0;
    while let Some(chunk) = body.next().await {
        let chunk = chunk.map_err(|failed| Unread::Broke(failed.to_string()))?;
        let read = String::from_utf8_lossy(&chunk);
        units += read.encode_utf16().count();
        if units > MAX_TEXT_UNITS {
            return Err(Unread::TooLarge);
        }
        text.push_str(&read);
    }
    Ok(text)
}

#[cfg(test)]
mod tests;

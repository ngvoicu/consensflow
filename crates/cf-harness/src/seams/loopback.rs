//! Where an adapter asks a peer on loopback over HTTP, as the channels'
//! `fetch` asked ConsensFlow's Codex broker and OpenCode's plugin and
//! server: the request as its caller wrote it, the reply's head, then its
//! body within a size. A failure says whether a head came. A timeout is the
//! caller's: `within` around `send`, and around `body` with what is left; a
//! reply dropped unread is `response.body.cancel()`.

use crate::contract::Work;
use crate::shared::net;

/// A request's method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
}

/// A request to a peer on loopback, its URL as the caller wrote it (built
/// as `new URL` and `searchParams` build it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub method: Method,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
}

/// Why a reply's body could not be read whole.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BodyFailed {
    /// It went past the size asked for.
    TooLarge,
    /// The connection broke or ended before it did.
    Cut(String),
}

/// Where an adapter's requests go.
pub trait Loopback {
    /// Sends `request`: its reply once a head came, or why none did.
    fn send(&self, request: Request) -> Work<'_, Result<Box<dyn Reply>, String>>;
}

/// A reply whose head came.
pub trait Reply {
    fn status(&self) -> u16;
    /// Its body whole, at most `limit` bytes.
    fn body(&mut self, limit: usize) -> Work<'_, Result<Vec<u8>, BodyFailed>>;
}

/// The system's: HTTP/1.1 over a TCP connection of its own per request.
pub struct SystemLoopback;

impl Loopback for SystemLoopback {
    fn send(&self, request: Request) -> Work<'_, Result<Box<dyn Reply>, String>> {
        Box::pin(async move {
            let reply = net::send(request).await?;
            Ok(Box::new(reply) as Box<dyn Reply>)
        })
    }
}

impl Reply for net::Reply {
    fn status(&self) -> u16 {
        net::Reply::status(self)
    }

    fn body(&mut self, limit: usize) -> Work<'_, Result<Vec<u8>, BodyFailed>> {
        Box::pin(net::Reply::body(self, limit))
    }
}

//! Where an adapter asks a peer on loopback over HTTP, as the channels'
//! `fetch` asked ConsensFlow's Codex broker: the request as its
//! caller wrote it, the reply's head, then its
//! body within a size. A failure says whether a head came. A timeout is the
//! caller's: `within` around `send`, and around `body` with what is left; a
//! reply dropped unread is `response.body.cancel()`.

use crate::contract::{PaneHost, Work};
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

/// What undici says of a request no head came for: the sentence its
/// callers pass on.
pub const FETCH_FAILED: &str = "fetch failed";

/// What undici says of a body the connection broke or ended before it was
/// whole.
pub const TERMINATED: &str = "terminated";

/// Why a reply's body could not be read whole.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BodyFailed {
    /// It went past the size asked for.
    TooLarge,
    /// The connection broke or ended before it did ([`TERMINATED`]).
    Cut,
}

/// Where an adapter's requests go.
pub trait Loopback {
    /// Sends `request`: its reply once a head came, or [`FETCH_FAILED`].
    fn send(&self, request: Request) -> Work<'_, Result<Box<dyn Reply>, String>>;

    /// Sends `request` as [`Loopback::send`] does, as the hand-over of text to a
    /// window: once its connection is up, right before a byte of it is written,
    /// `host` is asked whether it admits it ([`PaneHost::admits`]), and one it does
    /// not is never written, the failure
    /// [`NOT_ADMITTED`](crate::contract::NOT_ADMITTED). The connection is a wait of
    /// the system's, and a Close can come during it (Astra's review of 2026-10-16,
    /// round 5).
    fn hand_over<'a>(
        &'a self,
        request: Request,
        host: &'a dyn PaneHost,
    ) -> Work<'a, Result<Box<dyn Reply>, String>>;
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
            let reply = net::send(request, &|| true).await?;
            Ok(Box::new(reply) as Box<dyn Reply>)
        })
    }

    fn hand_over<'a>(
        &'a self,
        request: Request,
        host: &'a dyn PaneHost,
    ) -> Work<'a, Result<Box<dyn Reply>, String>> {
        Box::pin(async move {
            let reply = net::send(request, &|| host.hand_over_to_broker()).await?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::ScriptedHost;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn t1813_system_codex_broker_handover_precedes_the_request_and_reply() {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                let host = ScriptedHost::default();
                let request = Request {
                    method: Method::Post,
                    url: format!(
                        "http://127.0.0.1:{}/deliver",
                        listener.local_addr().unwrap().port()
                    ),
                    headers: Vec::new(),
                    body: Some(b"brief".to_vec()),
                };
                let peer = async {
                    let (mut socket, _) = listener.accept().await.unwrap();
                    let mut buffer = [0; 4096];
                    assert!(socket.read(&mut buffer).await.unwrap() > 0);
                    assert_eq!(
                        host.broker_handovers(),
                        1,
                        "the holding changes before the broker receives the request"
                    );
                    socket
                        .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
                        .await
                        .unwrap();
                };
                let (reply, ()) = tokio::join!(SystemLoopback.hand_over(request, &host), peer);
                assert_eq!(reply.unwrap().status(), 200);
            });
    }
}

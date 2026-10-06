//! The WebSockets the broker speaks: how one is opened to Codex's server,
//! what limits every one has, how a socket is read (only its text and binary
//! messages are the broker's business) and written (by a task of its own, from
//! a queue that counts the bytes it holds).

use std::cell::Cell;
use std::io;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use cf_base::json::{from_slice_lossy, is_json_lossy};
use futures_util::stream::SplitStream;
use futures_util::{Sink, SinkExt, StreamExt};
use serde_json::Value;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::tungstenite::http::Uri;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::tungstenite::{self, ClientRequestBuilder, Message, Utf8Bytes};
use tokio_tungstenite::{client_async_with_config, WebSocketStream};

use crate::endpoint::{Target, Upstream};

/// The most a message, a frame, or what is queued unsent for one socket may
/// hold: 64 MiB.
pub(crate) const MAX_FRAME: usize = 64 * 1024 * 1024;

/// How long a socket has to open: its connection and its handshake.
const HANDSHAKE: Duration = Duration::from_secs(3);

/// Either end of a connection a WebSocket runs over.
pub(crate) trait Io: AsyncRead + AsyncWrite + Unpin {}
impl<T: AsyncRead + AsyncWrite + Unpin> Io for T {}

/// A WebSocket, over whatever carries it.
pub(crate) type Socket = WebSocketStream<Box<dyn Io>>;
pub(crate) type SocketStream = SplitStream<Socket>;

/// What every socket is configured with: messages and frames up to
/// [`MAX_FRAME`] (tungstenite stops a frame at 16 MiB unless told otherwise).
/// No compression is offered or accepted: tungstenite has none, and the
/// broker's own handshake answers none.
pub(crate) fn config() -> WebSocketConfig {
    WebSocketConfig::default()
        .max_message_size(Some(MAX_FRAME))
        .max_frame_size(Some(MAX_FRAME))
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum ConnectError {
    #[error("{0}")]
    Io(#[from] io::Error),
    #[error("{0}")]
    Handshake(#[from] tungstenite::Error),
    #[error("the opening handshake timed out")]
    TimedOut,
}

/// Opens a WebSocket to Codex's server within three seconds.
pub(crate) async fn connect(upstream: &Upstream) -> Result<Socket, ConnectError> {
    tokio::time::timeout(HANDSHAKE, open(upstream))
        .await
        .map_err(|_| ConnectError::TimedOut)?
}

async fn open(upstream: &Upstream) -> Result<Socket, ConnectError> {
    let (io, host): (Box<dyn Io>, String) = match &upstream.target {
        Target::Tcp(address) => {
            let stream = TcpStream::connect(address).await?;
            stream.set_nodelay(true)?;
            (Box::new(stream), address.to_string())
        }
        #[cfg(unix)]
        Target::Unix(path) => (
            Box::new(tokio::net::UnixStream::connect(path).await?),
            "localhost".to_string(),
        ),
    };
    let uri: Uri = format!("ws://{host}/")
        .parse()
        .map_err(tungstenite::Error::from)?;
    let mut request = ClientRequestBuilder::new(uri);
    if let Some(authorization) = &upstream.authorization {
        request = request.with_header("authorization", authorization);
    }
    let (socket, _) = client_async_with_config(request, io, Some(config())).await?;
    Ok(socket)
}

/// What a socket's reader found next.
pub(crate) enum Frame {
    /// A message, as the text it is. A binary message is passed on as text.
    Json(Utf8Bytes),
    /// The socket is closed or failed: nothing more will come.
    End,
}

/// The next message of `stream` that is the broker's business. Pings and
/// pongs are the socket's own and never end it; an error does, and so does a
/// close, once the socket has answered it: reading on after a close is what
/// sends the answer, and the stream ends by itself right after.
pub(crate) async fn next_frame(stream: &mut SocketStream) -> Frame {
    loop {
        match stream.next().await {
            Some(Ok(Message::Text(text))) => return Frame::Json(text),
            Some(Ok(Message::Binary(bytes))) => {
                return Utf8Bytes::try_from(bytes).map_or(Frame::End, Frame::Json);
            }
            Some(Ok(
                Message::Ping(_) | Message::Pong(_) | Message::Frame(_) | Message::Close(_),
            )) => {}
            Some(Err(_)) | None => return Frame::End,
        }
    }
}

/// What a message says, as the broker can read it.
pub(crate) enum Reading {
    Message(Value),
    /// JSON nested too deep for serde_json to build (JavaScript's `JSON.parse`
    /// reads it): not for the broker to learn from, but nothing wrong with it,
    /// so it goes on as it came.
    TooDeep,
    NotJson,
}

pub(crate) fn reading(text: &Utf8Bytes) -> Reading {
    match from_slice_lossy(text.as_bytes()) {
        Ok(message) => Reading::Message(message),
        Err(_) if is_json_lossy(text.as_bytes()) => Reading::TooDeep,
        Err(_) => Reading::NotJson,
    }
}

/// Whether a message was taken for sending.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Sent {
    Queued,
    /// The socket is closed.
    Closed,
    /// It would put more than [`MAX_FRAME`] bytes unsent behind the socket.
    Full,
}

/// What waits in a socket's queue: a message, and for one whose fate matters
/// to someone, what it is watched by.
struct Queued {
    message: Message,
    watch: Option<Watch>,
}

/// What watches a queued message: `ended`, raised once it is no longer
/// wanted, and where the writer says what became of it. A message dropped
/// from the queue unsent says nothing, which its receiver reads as not
/// written.
struct Watch {
    ended: Arc<AtomicBool>,
    written: oneshot::Sender<bool>,
}

/// The way into a socket's writer: messages wait in its queue, and the bytes
/// the queue holds (what the writer has not finished sending included) are
/// counted, so a socket that does not take what is sent cannot be made to hold
/// without limit.
pub(crate) struct Outbox {
    queue: mpsc::UnboundedSender<Queued>,
    unsent: Rc<Cell<usize>>,
    open: Rc<Cell<bool>>,
}

/// The writer's end of an [`Outbox`].
pub(crate) struct Inbox {
    queue: mpsc::UnboundedReceiver<Queued>,
    unsent: Rc<Cell<usize>>,
    open: Rc<Cell<bool>>,
}

/// A queue for one socket's messages: where they go in, and where its writer takes them.
pub(crate) fn queue() -> (Outbox, Inbox) {
    let (sender, receiver) = mpsc::unbounded_channel();
    let unsent = Rc::new(Cell::new(0));
    let open = Rc::new(Cell::new(true));
    (
        Outbox {
            queue: sender,
            unsent: Rc::clone(&unsent),
            open: Rc::clone(&open),
        },
        Inbox {
            queue: receiver,
            unsent,
            open,
        },
    )
}

impl Outbox {
    /// Queues `message` to be sent, in order, unless the socket is closed or
    /// would hold more than [`MAX_FRAME`] bytes unsent. Synchronous, so
    /// whoever sends is not interrupted between a check and the send.
    pub(crate) fn send(&self, message: Message) -> Sent {
        self.enqueue(message, None)
    }

    /// [`Outbox::send`], for a message that is not wanted any more once
    /// `ended` is raised: the writer, which reads it right before it begins
    /// the message, sends nothing of it from then on. What the writer made of
    /// it comes through the receiver: true when it was written to the socket
    /// whole, false when `ended` came first or the socket failed or went
    /// before it was written (a dropped receiver is that too). The writer
    /// begins a message once, and one begun is written whatever is raised
    /// meanwhile.
    pub(crate) fn send_unless(
        &self,
        message: Message,
        ended: Arc<AtomicBool>,
    ) -> Result<oneshot::Receiver<bool>, Sent> {
        let (written, reported) = oneshot::channel();
        match self.enqueue(message, Some(Watch { ended, written })) {
            Sent::Queued => Ok(reported),
            refused => Err(refused),
        }
    }

    fn enqueue(&self, message: Message, watch: Option<Watch>) -> Sent {
        if !self.open.get() {
            return Sent::Closed;
        }
        let size = message.len();
        if self.unsent.get().saturating_add(size) > MAX_FRAME {
            return Sent::Full;
        }
        self.unsent.set(self.unsent.get() + size);
        if self.queue.send(Queued { message, watch }).is_err() {
            self.open.set(false);
            return Sent::Closed;
        }
        Sent::Queued
    }

    /// Nothing is taken for this socket any more.
    pub(crate) fn close(&self) {
        self.open.set(false);
    }
}

/// Sends what the queue holds to `sink`, in order, until the queue ends: true
/// then; false when the socket failed. A message that is not wanted any more
/// when its turn comes is skipped, and the one who asked is told it was not
/// written; so is the one whose message the socket failed on, and whoever's
/// message was left in the queue with it.
pub(crate) async fn write_all<S: Sink<Message> + Unpin>(mut sink: S, mut inbox: Inbox) -> bool {
    while let Some(Queued { message, watch }) = inbox.queue.recv().await {
        let size = message.len();
        let wanted = watch
            .as_ref()
            .is_none_or(|watch| !watch.ended.load(Ordering::SeqCst));
        let written = wanted && sink.send(message).await.is_ok();
        if let Some(watch) = watch {
            // Whoever asked may not be waiting any more.
            let _ = watch.written.send(written);
        }
        if wanted && !written {
            inbox.open.set(false);
            return false;
        }
        inbox.unsent.set(inbox.unsent.get().saturating_sub(size));
    }
    true
}

//! Building a bridge, and the future that runs it.

use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll};
use std::time::Duration;

use cf_proto::bridge::Role;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::mpsc;

use super::reader::read_loop;
use super::state::{ErrorHandler, Inner, ReadHandler, Settings};
use super::writer::write_loop;
use super::Bridge;
use crate::BridgeError;

/// The most a frame may take, a newline not counted, unless the builder says
/// otherwise.
pub const DEFAULT_MAX_FRAME_BYTES: usize = 1024 * 1024;

/// How long a request waits for its answer unless the builder or the call
/// says otherwise.
pub const DEFAULT_DEADLINE: Duration = Duration::from_secs(30);

/// What one end of the bridge is, before it is connected to its streams.
pub struct BridgeBuilder {
    role: Role,
    max_frame_bytes: usize,
    default_deadline: Duration,
    on_error: Option<Rc<ErrorHandler>>,
    on_fatal: Option<Rc<ErrorHandler>>,
    after_read: Option<Rc<ReadHandler>>,
}

impl BridgeBuilder {
    /// The `role` end: it mints that role's ids and accepts the other's.
    pub fn new(role: Role) -> Self {
        Self {
            role,
            max_frame_bytes: DEFAULT_MAX_FRAME_BYTES,
            default_deadline: DEFAULT_DEADLINE,
            on_error: None,
            on_fatal: None,
            after_read: None,
        }
    }

    /// Refuses frames over `bytes`, in and out.
    pub fn max_frame_bytes(mut self, bytes: usize) -> Self {
        self.max_frame_bytes = bytes;
        self
    }

    /// How long a request that is given no deadline waits.
    pub fn default_deadline(mut self, deadline: Duration) -> Self {
        self.default_deadline = deadline;
        self
    }

    /// Told of what is wrong with the input and did not stop the bridge, a
    /// line or a frame that is malformed or over the limit, and of what did
    /// stop it. Called on the bridge's thread, in place.
    pub fn on_error<F: Fn(BridgeError) + 'static>(mut self, handler: F) -> Self {
        self.on_error = Some(Rc::new(handler));
        self
    }

    /// Told once, when the transport fails: never when the input ends, which
    /// is the normal end.
    pub fn on_fatal<F: Fn(BridgeError) + 'static>(mut self, handler: F) -> Self {
        self.on_fatal = Some(Rc::new(handler));
        self
    }

    /// Called after the reader has handled the frames of one read of the
    /// input, in place and before it reads again: where the daemon runs the
    /// work those frames woke to its end, as Node's microtasks ran after each
    /// `data` callback of its event loop, before the next. The frames of one
    /// read are all handled before it is called, as Node's were, and a
    /// request's handler has had its first poll by then. A read that held no
    /// whole frame calls it too. The end of the input and a failed read do
    /// not, having handled nothing; a read whose frames closed the bridge
    /// calls it once, for the frames up to the close.
    pub fn after_read<F: Fn() + 'static>(mut self, handler: F) -> Self {
        self.after_read = Some(Rc::new(handler));
        self
    }

    /// Connects the bridge to its streams: `input` is read for the peer's
    /// frames, and `output` carries nothing but this end's. The bridge hears
    /// and says nothing until its [`Connection`] is polled: spawn it on the
    /// `LocalSet` the daemon runs on. Handlers are added once it is spawned,
    /// before anything is awaited, so none misses a frame.
    ///
    /// ```
    /// # use cf_bridge::local::BridgeBuilder;
    /// # use cf_proto::bridge::Role;
    /// # use serde_json::json;
    /// # let runtime = tokio::runtime::Builder::new_current_thread().build().unwrap();
    /// # tokio::task::LocalSet::new().block_on(&runtime, async {
    /// let (stdin, _peer_writes) = tokio::io::duplex(1024);
    /// let (stdout, _peer_reads) = tokio::io::duplex(1024);
    /// let (bridge, connection) = BridgeBuilder::new(Role::Daemon).connect(stdin, stdout);
    /// tokio::task::spawn_local(connection);
    /// bridge.on("ping", |_, _| async { Ok(json!({ "ok": true })) });
    /// # });
    /// ```
    pub fn connect<R, W>(self, input: R, output: W) -> (Bridge, Connection)
    where
        R: AsyncRead + Unpin + 'static,
        W: AsyncWrite + Unpin + 'static,
    {
        let (queue, queued) = mpsc::unbounded_channel();
        let inner = Rc::new(Inner::new(
            Settings {
                role: self.role,
                max_frame_bytes: self.max_frame_bytes,
                default_deadline: self.default_deadline,
                on_error: self.on_error,
                on_fatal: self.on_fatal,
                after_read: self.after_read,
            },
            queue,
        ));
        let reader = read_loop(Rc::clone(&inner), input);
        let writer = write_loop(Rc::downgrade(&inner), output, queued);
        let run = async move {
            tokio::join!(reader, writer);
        };
        (Bridge { inner }, Connection { run: Box::pin(run) })
    }
}

/// Runs a bridge: reads its input and dispatches what it finds, and writes
/// what is queued for its output. It is finished once the input has ended and
/// the output has been ended too, which happens when the bridge is closed or
/// fails, or when every handle to it is gone.
///
/// It does its work only while it is polled, one thread's worth of it: spawn
/// it on the `LocalSet` the handlers run on.
#[must_use = "a bridge hears and says nothing until its connection is polled"]
pub struct Connection {
    run: Pin<Box<dyn Future<Output = ()>>>,
}

impl Future for Connection {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<()> {
        self.run.as_mut().poll(context)
    }
}

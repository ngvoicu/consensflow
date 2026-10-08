//! The bridge's transport on one thread: the daemon's end of the JSON-lines
//! protocol (`cf_proto::bridge`), over any tokio `AsyncRead` and `AsyncWrite`,
//! on a `LocalSet`. It has every rule of Node's bridge, which it replaced:
//!
//! - [`Bridge::request`] gives the answer's body; `{ok:false, error:"deadline"}`
//!   past the deadline (30 s when it is given none), and an answer that comes
//!   later is dropped; `{ok:false, error:"too-large"}` for a frame over the
//!   limit, which is never written; `Err(Eof)` once the input ended, to the
//!   requests waiting and to those made after.
//! - [`Bridge::on`] handles the peer's requests. The reader starts the
//!   handler and never waits for it, so one that asks the peer for something
//!   in turn does not stop the frames after it. Its failure answers
//!   `{ok:false, error}` with its words; an op with no handler answers
//!   `unknown-op`; an answer over the limit is replaced by the bounded
//!   `too-large`, and when even that does not fit the transport fails.
//! - [`Bridge::on_event`] handles the peer's events, synchronously, on the
//!   reader and in the order they arrived, before the next frame is read.
//!   That is how an exit reaches the engine before the answer written after
//!   it: the handler changes what the exit changes and starts the rest of its
//!   work on a task of its own.
//! - [`BridgeBuilder::after_read`] is told once the reader has handled all the
//!   frames of one read, before it reads again: where the daemon runs, to
//!   their end, the tasks those frames woke, as Node ran its microtasks after
//!   each `data` callback.
//! - A malformed line is reported to `on_error` and skipped; an oversized one
//!   is checked whole before it is routed, so only a well-formed request is
//!   answered and only a well-formed response settles anything.
//! - A failure of the transport (the end of the input excepted) refuses every
//!   request waiting, closes the bridge, ends the output once what was queued
//!   is written, and is told to `on_error` and `on_fatal`.
//! - [`Bridge::ended`] is the one place that says a bridge ended and why (its
//!   input ended, it was closed, its transport failed): the end of the input
//!   is told to no callback, and the daemon stops on it.
//! - Nothing but frames is written to the output. The handle line the app
//!   reads first is not a frame: the daemon writes it before it connects.
//!
//! Where it differs from Node's bridge, on purpose:
//!
//! - The streams belong to the bridge. [`Bridge::close`] ends the output, and
//!   the input is dropped when the bridge stops reading it. Node's bridge left
//!   both to its caller.
//! - [`Bridge::event`] says a frame is queued, not that the stream took it: a
//!   write that fails later fails the bridge, and the events after it are
//!   refused.
//! - A body is a `serde_json::Value`, which is always JSON: the answers
//!   Node's bridge made for a body that would not serialize cannot arise here.
//! - A request handler is a future, not a function that may throw: it fails by
//!   returning its words. An event handler returns nothing, so what
//!   Node's bridge did for one that threw, report it and go on, has no
//!   counterpart. A handler of either kind that panics before its first wait
//!   ends the reader, and with it the bridge: every request waiting is refused
//!   with `Eof`. A request handler that panics after a wait loses its own
//!   answer, and nothing else.
//! - A line that is not UTF-8 is read as Node read it, lossily.
//! - The frames waiting for the output are not bounded, as the buffer of
//!   Node's bridge's stream was not. A peer that stops reading shows as requests
//!   that end at their deadlines.
//! - A handler done at its first poll is answered there, where JavaScript
//!   answered on a later microtask: the answers to requests read together may
//!   go out in another order (an `unknown-op` no longer overtakes a ready
//!   handler's). Each answer names its request, so none is mismatched.
//! - A [`Connection`] dropped before it was ever polled has not started its
//!   reader, so nothing refuses the requests made meanwhile: they end at
//!   their deadlines. A running daemon polls its connection from the start.
//!
//! Everything runs on one thread, as the daemon does: state sits in cells,
//! and no borrow is held while a handler or a callback runs.

#![forbid(unsafe_code)]

mod builder;
mod lines;
mod reader;
mod registry;
mod request;
mod serve;
mod state;
mod writer;

#[cfg(test)]
mod tests;

use std::future::Future;
use std::rc::Rc;

pub use builder::{BridgeBuilder, Connection, DEFAULT_DEADLINE, DEFAULT_MAX_FRAME_BYTES};
pub use registry::Subscription;
use state::Inner;

use crate::BridgeError;

/// Why a bridge ended, the first way it did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ended {
    /// The peer's end of the input ended: the normal end of a bridge, which
    /// is not a failure and is told to no `on_fatal`.
    Input,
    /// This end closed it ([`Bridge::close`]).
    Closed,
    /// The transport failed, as told to `on_fatal`: a read or a write that
    /// the system refused (a peer that has gone from the output).
    Failed(BridgeError),
}

/// One end of the bridge. Cheap to clone: every clone is the same bridge. It
/// belongs to its thread, as the state it serves does:
///
/// ```compile_fail,E0277
/// fn needs_send<T: Send>() {}
/// needs_send::<cf_bridge::local::Bridge>();
/// ```
#[derive(Clone)]
pub struct Bridge {
    inner: Rc<Inner>,
}

impl Bridge {
    /// Whether the bridge has closed: its input ended, it was closed, or its
    /// transport failed.
    pub fn closed(&self) -> bool {
        self.inner.is_closed()
    }

    /// Waits for the bridge to end and says why it did first: its input
    /// ended, it was closed, or its transport failed. The end is latched: it
    /// is told at once to whoever asks after it came, and to as many as ask.
    /// It owns no part of the bridge, so a bridge that is dropped with every
    /// handle to it ends as closed.
    ///
    /// This is how the daemon hears its input end, which no callback of the
    /// builder says: `on_fatal` is told of a failure only.
    pub fn ended(&self) -> impl Future<Output = Ended> + 'static {
        let mut heard = self.inner.watch_ended();
        async move {
            match heard.wait_for(Option::is_some).await {
                Ok(ended) => ended.clone().unwrap_or(Ended::Closed),
                Err(_) => Ended::Closed,
            }
        }
    }

    /// Refuses every request waiting with `Eof`, stops reading, and ends the
    /// output once what was queued is written. It does nothing the second
    /// time.
    pub fn close(&self) {
        self.inner.close();
    }
}

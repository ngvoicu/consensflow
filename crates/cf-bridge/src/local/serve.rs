//! The peer's requests: a handler started without being waited for, and the
//! answer that goes back, which always fits the limit or fails the bridge.

use std::task::{Context, Poll, Waker};

use cf_proto::bridge::{too_large_body, unknown_op_body, Frame, PROTOCOL_VERSION};
use serde_json::Value;

use super::request::encode;
use super::state::refusal;
use super::Bridge;
use crate::BridgeError;

impl Bridge {
    /// Starts the handler of a request on the reader. It is polled once
    /// here, so what it does before its first wait happens before the next
    /// frame is read, as it did when a handler was a function called in
    /// place; if it is not done, the rest runs on a task of its own, and
    /// nothing waits for it. A request for an op with no handler is answered
    /// `unknown-op`.
    pub(super) fn serve(&self, frame: Frame) {
        let Frame { id, op, body, .. } = frame;
        let Some(handler) = self.inner.registry.request(&op) else {
            self.respond(id, op, unknown_op_body());
            return;
        };
        let mut work = handler(self.clone(), body);
        match work.as_mut().poll(&mut Context::from_waker(Waker::noop())) {
            Poll::Ready(result) => self.answer(id, op, result),
            Poll::Pending => {
                let bridge = self.clone();
                tokio::task::spawn_local(async move {
                    let result = work.await;
                    bridge.answer(id, op, result);
                });
            }
        }
    }

    /// A handler's end: its body, or `{ok:false, error}` with its words.
    fn answer(&self, id: String, op: String, result: Result<Value, String>) {
        let body = result.unwrap_or_else(|words| refusal(&words));
        self.respond(id, op, body);
    }

    /// Answers a request. A body over the limit is replaced by `too-large`;
    /// when even that does not fit, the peer could never hear back, so the
    /// bridge fails instead of leaving it waiting.
    pub(super) fn respond(&self, id: String, op: String, body: Value) {
        let inner = &self.inner;
        if inner.is_closed() {
            return;
        }
        let mut frame = Frame {
            v: PROTOCOL_VERSION,
            id,
            kind: "res".to_owned(),
            op,
            body,
        };
        let mut line = match encode(&frame) {
            Ok(line) => line,
            Err(error) => return inner.fail(error),
        };
        if line.len() > inner.max_frame_bytes {
            frame.body = too_large_body();
            line = match encode(&frame) {
                Ok(line) => line,
                Err(error) => return inner.fail(error),
            };
            if line.len() > inner.max_frame_bytes {
                inner.fail(BridgeError::MalformedFrame(format!(
                    "response to {} cannot fit maxFrameBytes",
                    frame.id
                )));
                return;
            }
        }
        inner.write(line);
    }
}

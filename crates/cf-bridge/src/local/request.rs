//! What this end sends of its own: requests and events.

use std::future::Future;
use std::rc::Rc;
use std::time::Duration;

use cf_proto::bridge::{too_large_body, Frame, PROTOCOL_VERSION};
use serde_json::Value;
use tokio::sync::oneshot;
use tokio::time::{timeout_at, Instant};

use super::state::{refusal, Answer, Inner};
use super::Bridge;
use crate::BridgeError;

/// Further than any deadline reaches, for one that would overflow the clock.
const FAR_FUTURE: Duration = Duration::from_secs(30 * 365 * 24 * 60 * 60);

/// A request made, as far as making it goes: settled already, or sent and
/// waiting for its answer.
enum Sent {
    Settled(Answer),
    Waiting {
        answered: oneshot::Receiver<Answer>,
        until: Instant,
        /// Takes the request off the waiting list when it is no longer
        /// waited for, answered or not.
        _forget: Forget,
    },
}

struct Forget {
    inner: Rc<Inner>,
    id: String,
}

impl Drop for Forget {
    fn drop(&mut self) {
        self.inner.forget(&self.id);
    }
}

impl Bridge {
    /// Asks the peer for `op`. The frame is queued, and the deadline starts,
    /// when this is called; the future is the answer's body, which is
    ///
    /// - `{ok:false, error:"deadline"}` once `deadline` (this bridge's default
    ///   when none is given) has passed, and an answer that comes after is
    ///   dropped;
    /// - `{ok:false, error:"too-large"}` when the frame would be over the
    ///   limit, and then it is never written;
    /// - `Err` once the bridge closed: `Eof` after the input ended, or what
    ///   broke the transport. That goes for a request made after as well.
    pub fn request(
        &self,
        op: &str,
        body: Value,
        deadline: Option<Duration>,
    ) -> impl Future<Output = Result<Value, BridgeError>> + 'static {
        let sent = self.send(op, body, deadline);
        async move {
            match sent {
                Sent::Settled(answer) => answer,
                Sent::Waiting {
                    answered,
                    until,
                    _forget,
                } => match timeout_at(until, answered).await {
                    Ok(answer) => answer.unwrap_or(Err(BridgeError::Eof)),
                    Err(_) => Ok(refusal("deadline")),
                },
            }
        }
    }

    fn send(&self, op: &str, body: Value, deadline: Option<Duration>) -> Sent {
        let inner = &self.inner;
        if inner.is_closed() {
            return Sent::Settled(Err(inner.terminal()));
        }
        let id = inner.next_id();
        let frame = Frame {
            v: PROTOCOL_VERSION,
            id: id.clone(),
            kind: "req".to_owned(),
            op: op.to_owned(),
            body,
        };
        let line = match encode(&frame) {
            Ok(line) => line,
            Err(error) => return Sent::Settled(Err(error)),
        };
        if line.len() > inner.max_frame_bytes {
            return Sent::Settled(Ok(too_large_body()));
        }
        let now = Instant::now();
        let until = now
            .checked_add(deadline.unwrap_or(inner.default_deadline))
            .unwrap_or_else(|| now + FAR_FUTURE);
        let answered = inner.expect(id.clone(), op, until);
        inner.write(line);
        Sent::Waiting {
            answered,
            until,
            _forget: Forget {
                inner: Rc::clone(inner),
                id,
            },
        }
    }

    /// Tells the peer of `op`, which it never answers. True when the frame
    /// was queued for the writer; false when the bridge is closed or the
    /// frame would be over the limit, and then nothing is written. A frame
    /// that the output then fails to take fails the bridge.
    pub fn event(&self, op: &str, body: Value) -> bool {
        let inner = &self.inner;
        if inner.is_closed() {
            return false;
        }
        let frame = Frame {
            v: PROTOCOL_VERSION,
            id: inner.next_id(),
            kind: "evt".to_owned(),
            op: op.to_owned(),
            body,
        };
        match encode(&frame) {
            Ok(line) if line.len() <= inner.max_frame_bytes => {
                inner.write(line);
                true
            }
            _ => false,
        }
    }
}

pub(super) fn encode(frame: &Frame) -> Result<Vec<u8>, BridgeError> {
    serde_json::to_vec(frame).map_err(|error| BridgeError::MalformedFrame(error.to_string()))
}

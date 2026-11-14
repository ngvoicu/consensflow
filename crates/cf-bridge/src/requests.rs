//! The requests and their answers: the request and event calls, the dispatch to handlers and the replies.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

use cf_proto::bridge::{too_large_body, unknown_op_body, Frame, PROTOCOL_VERSION};
use serde_json::{json, Value};

use crate::{Bridge, BridgeError, RequestHandler};
use crate::{STREAM_QUEUE_SHARE, WRITER_QUEUE_CAPACITY};

pub(crate) struct PendingRequest {
    op: String,
    pub(crate) sender: mpsc::Sender<Result<Value, BridgeError>>,
}

#[derive(Clone)]
pub(crate) struct RequestHandlerEntry {
    pub(crate) handler: Arc<RequestHandler>,
    pub(crate) launches_panes: bool,
}

impl Bridge {
    pub fn request(
        &self,
        op: impl Into<String>,
        body: Value,
        deadline_ms: Option<u64>,
    ) -> Result<Value, BridgeError> {
        let started = Instant::now();
        let timeout = deadline_ms
            .map(Duration::from_millis)
            .unwrap_or(self.inner.default_request_deadline);
        let deadline = started + timeout;
        if self.is_closed() {
            return Err(BridgeError::Eof);
        }
        let op = op.into();
        let id = self.next_id();
        let frame = Frame {
            v: PROTOCOL_VERSION,
            id: id.clone(),
            kind: "req".to_string(),
            op: op.clone(),
            body,
        };
        let encoded = self.encode(&frame)?;
        if encoded.len() > self.inner.max_frame_bytes {
            return Ok(too_large_body());
        }

        let (sender, receiver) = mpsc::channel();
        {
            let mut pending = self
                .inner
                .pending
                .lock()
                .map_err(|_| BridgeError::LockPoisoned)?;
            if self.is_closed() {
                return Err(BridgeError::Eof);
            }
            pending.insert(id.clone(), PendingRequest { op, sender });
        }
        let canceled = Arc::new(AtomicBool::new(false));
        let admitted = self.enqueue_encoded(
            encoded,
            Arc::clone(&canceled),
            Some(deadline),
            WRITER_QUEUE_CAPACITY,
        )?;
        if !admitted {
            canceled.store(true, Ordering::Release);
            self.remove_pending(&id);
            return Ok(json!({"ok":false,"error":"deadline"}));
        }

        let received = receiver.recv_timeout(deadline.saturating_duration_since(Instant::now()));
        match received {
            Ok(result) => result,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                canceled.store(true, Ordering::Release);
                self.remove_pending(&id);
                Ok(json!({"ok":false,"error":"deadline"}))
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(BridgeError::Eof),
        }
    }

    /// Tells the peer of `op`, which it never answers. It waits for room in the
    /// writer queue for as long as the transport is open, and is never refused for
    /// the queue being full: what comes of it (a window's exit, a privacy dialog)
    /// is not to be lost, the callers are few and each is a thread of its own, and
    /// a peer that reads nothing is a busy one, not a broken one. Only the
    /// transport's end refuses it.
    pub fn event(&self, op: impl Into<String>, body: Value) -> Result<bool, BridgeError> {
        let Some(encoded) = self.encode_event(op.into(), body)? else {
            return Ok(false);
        };
        self.enqueue_encoded(
            encoded,
            Arc::new(AtomicBool::new(false)),
            None,
            WRITER_QUEUE_CAPACITY,
        )
    }

    /// An event from a stream the peer must not lose, such as pane output. It
    /// waits for the writer instead of failing the transport: a burst is a
    /// busy peer, not a broken one, and the pane's unacknowledged-output
    /// budget already bounds how much can wait. A stream fills at most half the
    /// queue, so a response still finds room and keeps its fail-fast rule.
    pub fn stream_event(&self, op: impl Into<String>, body: Value) -> Result<bool, BridgeError> {
        let Some(encoded) = self.encode_event(op.into(), body)? else {
            return Ok(false);
        };
        self.enqueue_encoded(
            encoded,
            Arc::new(AtomicBool::new(false)),
            None,
            STREAM_QUEUE_SHARE,
        )
    }

    fn encode_event(&self, op: String, body: Value) -> Result<Option<Vec<u8>>, BridgeError> {
        if self.inner.output_closed() {
            return Ok(None);
        }
        let frame = Frame {
            v: PROTOCOL_VERSION,
            id: self.next_id(),
            kind: "evt".to_string(),
            op,
            body,
        };
        let encoded = self.encode(&frame)?;
        Ok((encoded.len() <= self.inner.max_frame_bytes).then_some(encoded))
    }

    fn next_id(&self) -> String {
        let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        format!("{}{id}", self.inner.role.prefix())
    }

    pub(crate) fn dispatch_request(&self, frame: Frame) {
        let Some(entry) = self.inner.handlers.get(&frame.op).cloned() else {
            // Its answer may wait for room in the writer queue, which the reader
            // must not: it goes on to the frames after this one.
            let bridge = self.clone();
            thread::spawn(move || {
                bridge.respond_or_report(frame.id, frame.op, unknown_op_body());
            });
            return;
        };
        let Some(admission) = self.admit_handler(entry.launches_panes) else {
            return;
        };
        let handler = entry.handler;
        let bridge = self.clone();
        thread::spawn(move || {
            let op = frame.op;
            let body = match handler(bridge.clone(), frame.body) {
                Ok(body) => body,
                Err(error) => json!({"ok":false,"error":error}),
            };
            drop(admission);
            bridge.respond_or_report(frame.id, op, body);
        });
    }

    fn admit_handler(&self, launches_panes: bool) -> Option<HandlerAdmission> {
        let mut lifecycle = match self.inner.lifecycle.lock() {
            Ok(lifecycle) => lifecycle,
            Err(_) => {
                self.report(BridgeError::LockPoisoned);
                return None;
            }
        };
        if lifecycle.closed {
            return None;
        }
        lifecycle.in_flight_handlers += 1;
        if launches_panes {
            lifecycle.in_flight_launches += 1;
        }
        Some(HandlerAdmission {
            bridge: self.clone(),
            launches_panes,
        })
    }

    fn finish_handler(&self, launches_panes: bool) {
        let mut lifecycle = self
            .inner
            .lifecycle
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        lifecycle.in_flight_handlers = lifecycle.in_flight_handlers.saturating_sub(1);
        if launches_panes {
            lifecycle.in_flight_launches = lifecycle.in_flight_launches.saturating_sub(1);
        }
        if lifecycle.closed {
            self.inner.closed_ready.notify_all();
        }
    }

    pub(crate) fn dispatch_response(&self, frame: Frame) {
        let pending = match self.inner.pending.lock() {
            Ok(mut pending) => match pending.get(&frame.id) {
                Some(request) if request.op != frame.op => {
                    let expected_op = request.op.clone();
                    drop(pending);
                    self.report(BridgeError::MalformedFrame(format!(
                        "response {} op {} does not match pending op {expected_op}",
                        frame.id, frame.op
                    )));
                    return;
                }
                Some(_) => pending.remove(&frame.id),
                None => None,
            },
            Err(_) => {
                self.report(BridgeError::LockPoisoned);
                return;
            }
        };
        if let Some(pending) = pending {
            let _ = pending.sender.send(Ok(frame.body));
        }
    }

    pub(crate) fn dispatch_event(&self, frame: Frame) {
        let Some(handlers) = self.inner.event_handlers.get(&frame.op) else {
            return;
        };
        for handler in handlers {
            let handler = Arc::clone(handler);
            let body = frame.body.clone();
            thread::spawn(move || handler(body));
        }
    }

    fn respond(&self, id: String, op: String, body: Value) -> Result<(), BridgeError> {
        let frame = Frame {
            v: PROTOCOL_VERSION,
            id,
            kind: "res".to_string(),
            op,
            body,
        };
        let mut encoded = self.encode(&frame)?;
        if encoded.len() > self.inner.max_frame_bytes {
            encoded = self.encode(&Frame {
                v: PROTOCOL_VERSION,
                id: frame.id,
                kind: frame.kind,
                op: frame.op,
                body: too_large_body(),
            })?;
            if encoded.len() > self.inner.max_frame_bytes {
                let error = BridgeError::MalformedFrame(
                    "outgoing too-large response cannot fit maxFrameBytes".to_string(),
                );
                self.fail_transport(error.clone());
                return Err(error);
            }
        }
        self.write_response(encoded)
    }

    fn respond_or_report(&self, id: String, op: String, body: Value) {
        if let Err(error) = self.respond(id, op, body) {
            if !self.is_closed() {
                self.report(error);
            }
        }
    }

    fn encode(&self, frame: &Frame) -> Result<Vec<u8>, BridgeError> {
        serde_json::to_vec(frame).map_err(|error| BridgeError::MalformedFrame(error.to_string()))
    }

    fn remove_pending(&self, id: &str) -> Option<PendingRequest> {
        match self.inner.pending.lock() {
            Ok(mut pending) => pending.remove(id),
            Err(_) => {
                self.report(BridgeError::LockPoisoned);
                None
            }
        }
    }
}

struct HandlerAdmission {
    bridge: Bridge,
    launches_panes: bool,
}

impl Drop for HandlerAdmission {
    fn drop(&mut self) {
        self.bridge.finish_handler(self.launches_panes);
    }
}

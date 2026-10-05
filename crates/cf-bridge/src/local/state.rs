//! What every task of a bridge shares: the requests waiting for an answer,
//! the handlers, the queue the writer drains, and the ways a bridge ends.
//! State sits in cells on the one thread, and no borrow is held while a
//! handler or a callback runs, so each of them may call the bridge back.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use cf_proto::bridge::{Frame, Role};
use serde_json::{json, Value};
use tokio::sync::{mpsc, oneshot, watch, Notify};
use tokio::time::Instant;

use super::registry::Registry;
use super::Ended;
use crate::BridgeError;

/// What a request ends in: the peer's body, or why none will come.
pub(super) type Answer = Result<Value, BridgeError>;

pub(super) type ErrorHandler = dyn Fn(BridgeError);

/// A request sent and not answered yet, and when it stops waiting.
struct Pending {
    op: String,
    until: Instant,
    answer: oneshot::Sender<Answer>,
}

/// What a bridge is built with.
pub(super) struct Settings {
    pub(super) role: Role,
    pub(super) max_frame_bytes: usize,
    pub(super) default_deadline: Duration,
    pub(super) on_error: Option<Rc<ErrorHandler>>,
    pub(super) on_fatal: Option<Rc<ErrorHandler>>,
}

pub(super) struct Inner {
    role: Role,
    pub(super) max_frame_bytes: usize,
    pub(super) default_deadline: Duration,
    on_error: Option<Rc<ErrorHandler>>,
    on_fatal: Option<Rc<ErrorHandler>>,
    next_id: Cell<u64>,
    closed: Cell<bool>,
    /// Why the bridge closed, which a request made after that is refused with.
    terminal: RefCell<Option<BridgeError>>,
    pending: RefCell<HashMap<String, Pending>>,
    pub(super) registry: Registry,
    /// The frames the writer has not written yet. Taken away to end the output.
    queue: RefCell<Option<mpsc::UnboundedSender<Vec<u8>>>>,
    /// Told when the bridge closes, to stop the reader where it waits.
    pub(super) stop: Notify,
    /// Why the bridge ended, once it has: what [`super::Bridge::ended`]
    /// waits for, kept for whoever asks after it ended.
    ended: watch::Sender<Option<Ended>>,
}

impl Inner {
    pub(super) fn new(settings: Settings, queue: mpsc::UnboundedSender<Vec<u8>>) -> Self {
        Self {
            role: settings.role,
            max_frame_bytes: settings.max_frame_bytes,
            default_deadline: settings.default_deadline,
            on_error: settings.on_error,
            on_fatal: settings.on_fatal,
            next_id: Cell::new(0),
            closed: Cell::new(false),
            terminal: RefCell::new(None),
            pending: RefCell::new(HashMap::new()),
            registry: Registry::default(),
            queue: RefCell::new(Some(queue)),
            stop: Notify::new(),
            ended: watch::Sender::new(None),
        }
    }

    pub(super) fn is_closed(&self) -> bool {
        self.closed.get()
    }

    /// Waits to hear why the bridge ended: at once when it has.
    pub(super) fn watch_ended(&self) -> watch::Receiver<Option<Ended>> {
        self.ended.subscribe()
    }

    /// Why the bridge closed: what a request made now is refused with.
    pub(super) fn terminal(&self) -> BridgeError {
        self.terminal.borrow().clone().unwrap_or(BridgeError::Eof)
    }

    /// The id of the next request or event this end starts.
    pub(super) fn next_id(&self) -> String {
        let number = self.next_id.get() + 1;
        self.next_id.set(number);
        format!("{}{number}", self.role.prefix())
    }

    /// Whether the frame's id is in the namespace this end accepts for its
    /// kind: a response answers a request this end made, anything else comes
    /// from the peer.
    pub(super) fn accepts(&self, frame: &Frame) -> bool {
        let minted_by = if frame.kind == "res" {
            self.role
        } else {
            self.role.peer()
        };
        frame.id.starts_with(minted_by.prefix())
    }

    /// Queues a frame for the writer. The queue is gone only once the bridge
    /// is closed, and nothing is written then.
    pub(super) fn write(&self, mut line: Vec<u8>) {
        line.push(b'\n');
        if let Some(queue) = &*self.queue.borrow() {
            let _ = queue.send(line);
        }
    }

    /// Puts a request on the waiting list until `until`; what it ends in
    /// comes on the receiver.
    pub(super) fn expect(&self, id: String, op: &str, until: Instant) -> oneshot::Receiver<Answer> {
        let (answer, answered) = oneshot::channel();
        let op = op.to_owned();
        self.pending
            .borrow_mut()
            .insert(id, Pending { op, until, answer });
        answered
    }

    /// How many requests wait for an answer.
    #[cfg(test)]
    pub(super) fn waiting(&self) -> usize {
        self.pending.borrow().len()
    }

    /// Takes a request off the waiting list: nobody waits for it any more.
    pub(super) fn forget(&self, id: &str) {
        let forgotten = self.pending.borrow_mut().remove(id);
        drop(forgotten);
    }

    /// Hands the peer's answer to the request waiting for it. An answer for
    /// no request is dropped: the request is gone, past its deadline. One
    /// that comes past the request's deadline, though nobody has looked at
    /// the request since, settles it as `deadline`, as JavaScript's timer did
    /// on its own. One that names another op leaves the request waiting.
    pub(super) fn settle(&self, id: &str, op: &str, body: Value) {
        let Some(request) = self.pending.borrow_mut().remove(id) else {
            return;
        };
        if request.op != op {
            let expected = request.op.clone();
            self.pending.borrow_mut().insert(id.to_owned(), request);
            self.report(BridgeError::MalformedFrame(format!(
                "response {id} op {op} does not match pending op {expected}"
            )));
            return;
        }
        let answer = if Instant::now() >= request.until {
            refusal("deadline")
        } else {
            body
        };
        let _ = request.answer.send(Ok(answer));
    }

    /// Tells `on_error` of something that did not stop the bridge.
    pub(super) fn report(&self, error: BridgeError) {
        if let Some(handler) = self.on_error.clone() {
            handler(error);
        }
    }

    /// The input ended, the normal end of a bridge: what waits is refused
    /// with `Eof`, and nothing is reported. The output stays as it is.
    pub(super) fn eof(&self) {
        self.end(Ended::Input);
    }

    /// The transport broke: what waits is refused with `error`, `on_error`
    /// and `on_fatal` are told, and the output ends once what was queued
    /// before is written, so the peer sees it end.
    pub(super) fn fail(&self, error: BridgeError) {
        if self.closed.replace(true) {
            return;
        }
        self.finish(error.clone());
        self.ended.send_replace(Some(Ended::Failed(error.clone())));
        self.report(error.clone());
        self.end_output();
        if let Some(handler) = self.on_fatal.clone() {
            handler(error);
        }
    }

    /// Closes the bridge as an end of input does, and ends the output once
    /// what was queued is written.
    pub(super) fn close(&self) {
        self.end(Ended::Closed);
        self.end_output();
    }

    /// The first way a bridge ends that is no failure is the one it says:
    /// what waits is refused with `Eof`, and whoever waits to hear why is told.
    fn end(&self, ended: Ended) {
        if !self.closed.replace(true) {
            self.finish(BridgeError::Eof);
            self.ended.send_replace(Some(ended));
        }
    }

    fn finish(&self, error: BridgeError) {
        *self.terminal.borrow_mut() = Some(error.clone());
        let waiting: Vec<Pending> = self
            .pending
            .borrow_mut()
            .drain()
            .map(|(_, request)| request)
            .collect();
        for request in waiting {
            let _ = request.answer.send(Err(error.clone()));
        }
        self.registry.clear();
        self.stop.notify_one();
    }

    fn end_output(&self) {
        let queue = self.queue.borrow_mut().take();
        drop(queue);
    }
}

/// A body that says why nothing came of a request.
pub(super) fn refusal(error: &str) -> Value {
    json!({ "ok": false, "error": error })
}

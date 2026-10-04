//! The broker's own connection to Codex's server: the one it sends a
//! delivery on, and the one that tells it which threads are idle. Requests are
//! matched to their answers by the id the broker gave them.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use futures_util::StreamExt;
use serde_json::{json, Value};
use tokio::sync::oneshot;
use tokio::task::AbortHandle;
use tokio_tungstenite::tungstenite::{Message, Utf8Bytes};

use super::transport::{
    next_frame, queue, reading, write_all, Frame, Outbox, Reading, Sent, Socket,
};
use super::{now_ms, Shared};

/// What became of a request to Codex's server.
pub(super) enum Asked {
    /// Its answer.
    Answered(Value),
    /// It never went out: the connection is closed, or would hold more than
    /// 64 MiB unsent with it, and is ended for that.
    NotSent,
    /// It went out, and no answer came in time, or the connection was lost.
    Unanswered,
}

/// The connection and the requests it has no answer to yet.
pub(super) struct Control {
    outbox: Outbox,
    /// Each waits for its answer, which is none when the connection is lost.
    pending: RefCell<HashMap<String, oneshot::Sender<Option<Value>>>>,
    open: Cell<bool>,
    tasks: RefCell<Vec<AbortHandle>>,
}

/// A request's place among the pending ones, given up when the request ends
/// however it does: answered, out of time, or dropped with the task that
/// asked (a daemon that hung up while its delivery waited).
struct Waiting<'a> {
    control: &'a Control,
    id: &'a str,
}

impl Drop for Waiting<'_> {
    fn drop(&mut self) {
        if let Ok(mut pending) = self.control.pending.try_borrow_mut() {
            pending.remove(self.id);
        }
    }
}

impl Shared {
    /// Takes `socket` as the broker's own connection to Codex's server.
    pub(super) fn open_control(self: &Rc<Self>, socket: Socket) {
        let (sink, mut stream) = socket.split();
        let (outbox, inbox) = queue();
        let control = Rc::new(Control {
            outbox,
            pending: RefCell::new(HashMap::new()),
            open: Cell::new(true),
            tasks: RefCell::new(Vec::new()),
        });
        *self.control.borrow_mut() = Some(Rc::clone(&control));
        let shared = Rc::clone(self);
        let writer = self.spawn(async move {
            if !write_all(sink, inbox).await {
                shared.control_lost();
            }
        });
        let shared = Rc::clone(self);
        let reader = self.spawn(async move {
            while let Frame::Json(text) = next_frame(&mut stream).await {
                if !shared.control_said(&text) {
                    return;
                }
            }
            shared.control_lost();
        });
        control.tasks.borrow_mut().extend([writer, reader]);
    }

    /// Whether the connection to Codex's server is open.
    pub(super) fn control_open(&self) -> bool {
        self.control
            .borrow()
            .as_ref()
            .is_some_and(|control| control.open.get())
    }

    /// What Codex's server said on the broker's own connection: false when it
    /// was no JSON and the connection was ended for it. JSON too deep to read
    /// is passed over: nothing in it is the broker's to learn.
    fn control_said(&self, text: &Utf8Bytes) -> bool {
        let message = match reading(text) {
            Reading::Message(message) => message,
            Reading::TooDeep => return true,
            Reading::NotJson => {
                self.control_lost();
                return false;
            }
        };
        // Only a response answers a request: the server numbers its own
        // requests too, and one may carry the id of one of ours.
        if message.get("method").is_none() {
            if let Some(id) = message.get("id").and_then(Value::as_str) {
                let waiting = self
                    .control
                    .borrow()
                    .as_ref()
                    .and_then(|control| control.pending.borrow_mut().remove(id));
                if let Some(waiting) = waiting {
                    let _ = waiting.send(Some(message.clone()));
                }
            }
        }
        self.state.borrow_mut().control_said(&message);
        true
    }

    /// The connection to Codex's server is gone, or ended: nothing can be
    /// sent on it, and every request waiting for an answer gets none.
    pub(super) fn control_lost(&self) {
        let control = self.control.borrow().clone();
        if let Some(control) = control {
            control.open.set(false);
            control.outbox.close();
            for task in control.tasks.borrow_mut().drain(..) {
                task.abort();
            }
            let waiting: Vec<_> = control.pending.borrow_mut().drain().collect();
            for (_, request) in waiting {
                let _ = request.send(None);
            }
        }
        self.state.borrow_mut().lose_control();
    }

    /// Sends `message` on the connection, without waiting for anything.
    pub(super) fn send_control(&self, message: &Value) -> bool {
        let control = self.control.borrow().clone();
        control.is_some_and(|control| {
            control.open.get()
                && control.outbox.send(Message::text(message.to_string())) == Sent::Queued
        })
    }

    /// Asks Codex's server `method` and waits for the answer until
    /// `expires_at` (milliseconds since the epoch). The request is sent before
    /// this first waits, so whoever checked something just before calling it
    /// is not interrupted. A connection that takes nothing, with 64 MiB
    /// waiting unsent, is ended: nothing more could reach Codex on it.
    pub(super) async fn request(&self, method: &str, params: Value, expires_at: f64) -> Asked {
        let control = self.control.borrow().clone();
        let Some(control) = control.filter(|control| control.open.get()) else {
            return Asked::NotSent;
        };
        let id = uuid::Uuid::new_v4().to_string();
        let (reply, answer) = oneshot::channel();
        control.pending.borrow_mut().insert(id.clone(), reply);
        let _waiting = Waiting {
            control: &control,
            id: &id,
        };
        let message = json!({ "id": id, "method": method, "params": params });
        match control.outbox.send(Message::text(message.to_string())) {
            Sent::Queued => {}
            Sent::Full => {
                self.control_lost();
                return Asked::NotSent;
            }
            Sent::Closed => return Asked::NotSent,
        }
        let wait = Duration::try_from_secs_f64((expires_at - now_ms()).max(1.0) / 1000.0)
            .unwrap_or(Duration::from_secs(3));
        match tokio::time::timeout(wait, answer).await {
            Ok(Ok(Some(answer))) => Asked::Answered(answer),
            _ => Asked::Unanswered,
        }
    }

    /// How many requests are waiting for an answer.
    #[cfg(test)]
    pub(super) fn pending_requests(&self) -> usize {
        self.control
            .borrow()
            .as_ref()
            .map_or(0, |control| control.pending.borrow().len())
    }
}

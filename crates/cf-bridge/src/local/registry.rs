//! The handlers a bridge calls, and how one is taken away again.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::future::Future;
use std::mem;
use std::pin::Pin;
use std::rc::{Rc, Weak};

use serde_json::Value;

use super::state::Inner;
use super::Bridge;

/// What a request handler returns: its answer, or its words for why not.
pub(super) type HandlerFuture = Pin<Box<dyn Future<Output = Result<Value, String>>>>;
pub(super) type RequestHandler = dyn Fn(Bridge, Value) -> HandlerFuture;
pub(super) type EventHandler = dyn Fn(&Value);

/// Each handler carries the serial it was added with, so taking one away
/// never takes away another added after it.
type Requests = HashMap<String, (u64, Rc<RequestHandler>)>;
type Events = HashMap<String, Vec<(u64, Rc<EventHandler>)>>;

/// The handlers by op.
#[derive(Default)]
pub(super) struct Registry {
    serial: Cell<u64>,
    requests: RefCell<Requests>,
    events: RefCell<Events>,
}

impl Registry {
    fn next_serial(&self) -> u64 {
        let serial = self.serial.get() + 1;
        self.serial.set(serial);
        serial
    }

    /// The one handler of `op`: a new one replaces it.
    fn add_request(&self, op: String, handler: Rc<RequestHandler>) -> u64 {
        let serial = self.next_serial();
        let replaced = self.requests.borrow_mut().insert(op, (serial, handler));
        drop(replaced);
        serial
    }

    pub(super) fn request(&self, op: &str) -> Option<Rc<RequestHandler>> {
        let requests = self.requests.borrow();
        requests.get(op).map(|(_, handler)| Rc::clone(handler))
    }

    fn add_event(&self, op: String, handler: Rc<EventHandler>) -> u64 {
        let serial = self.next_serial();
        self.events
            .borrow_mut()
            .entry(op)
            .or_default()
            .push((serial, handler));
        serial
    }

    /// The handlers of `op` as they are now: one that takes itself away while
    /// the event is delivered does not make the next skip it.
    pub(super) fn events(&self, op: &str) -> Vec<Rc<EventHandler>> {
        let events = self.events.borrow();
        events
            .get(op)
            .map(|handlers| {
                handlers
                    .iter()
                    .map(|(_, handler)| Rc::clone(handler))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// A handler is dropped after the borrow ends: dropping it may run code
    /// that calls the bridge.
    fn remove_request(&self, op: &str, serial: u64) {
        let removed = {
            let mut requests = self.requests.borrow_mut();
            match requests.get(op) {
                Some((added, _)) if *added == serial => requests.remove(op),
                _ => None,
            }
        };
        drop(removed);
    }

    fn remove_event(&self, op: &str, serial: u64) {
        let removed = {
            let mut events = self.events.borrow_mut();
            events.get_mut(op).and_then(|handlers| {
                let at = handlers.iter().position(|(added, _)| *added == serial)?;
                Some(handlers.remove(at))
            })
        };
        drop(removed);
    }

    /// Lets every handler go, once the bridge is closed and none is called.
    pub(super) fn clear(&self) {
        let requests = mem::take(&mut *self.requests.borrow_mut());
        let events = mem::take(&mut *self.events.borrow_mut());
        drop((requests, events));
    }
}

/// What taking a handler away needs to find it again.
enum Registered {
    Request { op: String, serial: u64 },
    Event { op: String, serial: u64 },
}

/// A handler's registration. `off` takes the handler away; dropping this
/// leaves it in place.
pub struct Subscription {
    bridge: Weak<Inner>,
    registered: Registered,
}

impl Subscription {
    /// Stops calling the handler, unless a later one replaced it already.
    pub fn off(self) {
        let Some(inner) = self.bridge.upgrade() else {
            return;
        };
        match self.registered {
            Registered::Request { op, serial } => inner.registry.remove_request(&op, serial),
            Registered::Event { op, serial } => inner.registry.remove_event(&op, serial),
        }
    }
}

impl Bridge {
    /// Handles the peer's requests for `op`: the handler is called with the
    /// bridge and the request's body, and what its future gives is the
    /// answer's body, or `{ok:false, error}` with its words if it fails. It
    /// replaces the handler `op` had. An `op` with none is answered
    /// `unknown-op`.
    ///
    /// The reader calls the handler and polls the future once, so what it
    /// does before its first wait is done before the next frame is read; the
    /// rest runs on a task of its own, never waited for. A handler may ask the
    /// peer for something in turn.
    pub fn on<F, Fut>(&self, op: impl Into<String>, handler: F) -> Subscription
    where
        F: Fn(Bridge, Value) -> Fut + 'static,
        Fut: Future<Output = Result<Value, String>> + 'static,
    {
        let op = op.into();
        let handler: Rc<RequestHandler> =
            Rc::new(move |bridge, body| Box::pin(handler(bridge, body)));
        let serial = self.inner.registry.add_request(op.clone(), handler);
        self.subscription(Registered::Request { op, serial })
    }

    /// Handles the peer's events for `op`. The handler is called on the
    /// reader, in the order the events arrived, before it reads another frame:
    /// what an event changes is changed before the next frame is looked at. It
    /// does the rest of its work, if there is any, on a task of its own.
    /// Several handlers may stand for one `op`; each is called.
    pub fn on_event<F>(&self, op: impl Into<String>, handler: F) -> Subscription
    where
        F: Fn(&Value) + 'static,
    {
        let op = op.into();
        let serial = self.inner.registry.add_event(op.clone(), Rc::new(handler));
        self.subscription(Registered::Event { op, serial })
    }

    fn subscription(&self, registered: Registered) -> Subscription {
        Subscription {
            bridge: Rc::downgrade(&self.inner),
            registered,
        }
    }
}

//! The broker: what stands between a window's Codex TUI, Codex's server and
//! the daemon.
//!
//! The TUI connects here instead of to the server, and each of its
//! connections is proxied to a connection of its own to the server, frame by
//! frame, so the broker knows which thread the window shows (it owns the main
//! chief identity). The daemon asks it over HTTP what the window shows
//! (`GET /session`) and hands it messages (`POST /deliver`), which it checks
//! once more against the thread the window shows, and puts to Codex over a
//! connection of its own. Nothing is delivered to a thread the window moved
//! away from; a message whose fate is unknown is never sent twice.
//!
//! Everything runs on one thread, as Node did: state sits in `Rc<RefCell<…>>`
//! and no borrow is held across an `await`, so each check and the step that
//! follows it cannot be interrupted.

mod control;
mod delivery;
mod http;
mod pair;
mod selection;
mod transport;

#[cfg(test)]
pub(crate) mod tests;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::future::Future;
use std::net::Ipv4Addr;
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use cf_base::js;
use cf_board::Board;
use cf_proto::codex::{Bridge, Session};
use serde_json::json;
use tokio::net::TcpListener;
use tokio::task::{AbortHandle, JoinHandle};

use crate::endpoint::Upstream;
use control::Control;
use pair::Pair;
use selection::{ClientId, Selection};

/// What a broker is started with.
pub(crate) struct Config {
    /// The launch the window belongs to, the token the daemon and the TUI
    /// carry, and the loopback port to listen on (0 for any).
    pub(crate) bridge: Bridge,
    /// Codex's server.
    pub(crate) upstream: Upstream,
    /// Whether the window's first start opens in full-permission mode.
    pub(crate) fresh_bypass: bool,
    /// The board Codex's questions go to; none outside a window.
    pub(crate) board: Option<Arc<Board>>,
    /// How long a question waits for the board before the TUI's dialog takes over.
    pub(crate) question_wait: Duration,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum StartError {
    #[error("Codex native server could not be reached: {0}")]
    Upstream(transport::ConnectError),
    #[error("Codex native server did not initialize")]
    NotInitialized,
    #[error("Codex broker could not listen on 127.0.0.1:{port}: {cause}")]
    Listen { port: u16, cause: std::io::Error },
}

/// A running broker.
pub(crate) struct Broker {
    shared: Rc<Shared>,
    port: u16,
}

/// What every task of a broker shares.
struct Shared {
    bridge: Bridge,
    upstream: Upstream,
    board: Option<Arc<Board>>,
    question_wait: Duration,
    state: RefCell<Selection>,
    control: RefCell<Option<Rc<Control>>>,
    /// The TUI connections' proxies, to end them when the broker closes.
    pairs: RefCell<HashMap<ClientId, Weak<Pair>>>,
    /// Every task the broker started, to end them when it closes.
    tasks: RefCell<Vec<JoinHandle<()>>>,
    clients: Cell<u64>,
}

impl Broker {
    /// Connects to Codex's server, initializes it, and listens on loopback.
    /// A broker that could not does not stay connected to Codex's server.
    pub(crate) async fn start(config: Config) -> Result<Self, StartError> {
        let Config {
            bridge,
            upstream,
            fresh_bypass,
            board,
            question_wait,
        } = config;
        let port = bridge.port;
        let shared = Rc::new(Shared {
            bridge,
            upstream,
            board,
            question_wait,
            state: RefCell::new(Selection::new(fresh_bypass)),
            control: RefCell::new(None),
            pairs: RefCell::new(HashMap::new()),
            tasks: RefCell::new(Vec::new()),
            clients: Cell::new(0),
        });
        let socket = transport::connect(&shared.upstream)
            .await
            .map_err(StartError::Upstream)?;
        shared.open_control(socket);
        let initialized = shared
            .request(
                "initialize",
                json!({
                    "clientInfo": { "name": "consensflow-delivery", "version": "3.0.0" },
                    "capabilities": { "experimentalApi": true },
                }),
                now_ms() + 3000.0,
            )
            .await;
        if !js::truthy(initialized.as_ref().and_then(|answer| answer.get("result"))) {
            shared.control_lost();
            return Err(StartError::NotInitialized);
        }
        shared.send_control(&json!({ "method": "initialized" }));
        shared.state.borrow_mut().ready();
        let listener = match TcpListener::bind((Ipv4Addr::LOCALHOST, port)).await {
            Ok(listener) => listener,
            Err(cause) => {
                shared.control_lost();
                return Err(StartError::Listen { port, cause });
            }
        };
        let bound = listener.local_addr().map_or(port, |address| address.port());
        shared.spawn(http::accept(Rc::clone(&shared), listener));
        Ok(Self {
            shared,
            port: bound,
        })
    }

    /// The loopback port the broker listens on.
    pub(crate) fn port(&self) -> u16 {
        self.port
    }

    /// Refuses new work, forgets the selection, ends every socket and stops
    /// the server. A question being asked of the board stops at its next poll.
    pub(crate) async fn close(&self) {
        let shared = &self.shared;
        shared.state.borrow_mut().close();
        shared.control_lost();
        let pairs: Vec<_> = shared
            .pairs
            .borrow()
            .values()
            .filter_map(Weak::upgrade)
            .collect();
        for pair in pairs {
            pair.retire();
        }
        let tasks = std::mem::take(&mut *shared.tasks.borrow_mut());
        for task in &tasks {
            task.abort();
        }
        for task in tasks {
            let _ = task.await;
        }
    }
}

impl Shared {
    /// Starts `task` on this thread, to be ended when the broker closes.
    fn spawn(&self, task: impl Future<Output = ()> + 'static) -> AbortHandle {
        let handle = tokio::task::spawn_local(task);
        let abort = handle.abort_handle();
        let mut tasks = self.tasks.borrow_mut();
        tasks.retain(|task| !task.is_finished());
        tasks.push(handle);
        abort
    }

    fn next_client(&self) -> ClientId {
        self.clients.set(self.clients.get() + 1);
        ClientId(self.clients.get())
    }

    fn is_closed(&self) -> bool {
        self.state.borrow().is_closed()
    }

    /// What the window shows, as the daemon asks for it.
    fn session(&self) -> Session {
        let open = self.control_open();
        let state = self.state.borrow();
        Session {
            launch_id: self.bridge.launch_id.clone(),
            session_id: state.session_id().map(str::to_string),
            revision: state.revision(),
            empty: state.is_empty(),
            // Whether a delivery would be taken now: the dispatcher holds a
            // message while this is false, instead of spending its attempts.
            available: state.available(open),
        }
    }
}

/// Milliseconds since the epoch: `Date.now()`.
fn now_ms() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |since| since.as_millis() as f64)
}

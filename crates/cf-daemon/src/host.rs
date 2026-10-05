//! The pane host as the engine uses it, over the bridge
//! (`src/core/pane-host.js`): a window's pane is opened and killed by a
//! request to the host, an adapter's own requests (`pane.snapshot`,
//! `pane.input`, `pane.write_paste`, `pane.claim`) go straight through, and
//! the host's `pane.exit` events are told to the engine where they are read.
//!
//! The protocol is the app's (`crates/cf-panes/src/pane_handlers.rs`); this
//! adds no rules of its own, only the Windows shim a window's program may be
//! ([`cf_process::pane_argv`]) and the bound on the wait for an open.

use std::rc::Rc;
use std::time::Duration;

use cf_base::env::Env;
use cf_base::js;
use cf_bridge::local::{Bridge, BridgeBuilder, Subscription};
use cf_bridge::BridgeError;
use cf_engine::host::{EngineHost, Killed, OpenPane, Opened};
use cf_engine::runtime::LocalWork;
use cf_harness::contract::{HostError, Pane, PaneHost, Work};
use cf_proto::bridge::Role;
use serde_json::{json, Map, Value};

use crate::errors::contain_now;
use crate::seams::DaemonSpawn;

/// How long the host has to open a window: the pane's program is started and
/// watched before it answers (`deadlineMs: 60_000`).
pub const OPEN_DEADLINE: Duration = Duration::from_secs(60);

/// What the engine says to the host of the app, through its bridge.
pub struct BridgeHost {
    bridge: Bridge,
    env: Env,
}

impl BridgeHost {
    /// The host on the other end of `bridge`; `env` is the daemon's, which
    /// finds the shim a window's program may be.
    pub fn new(bridge: Bridge, env: Env) -> Self {
        Self { bridge, env }
    }
}

impl PaneHost for BridgeHost {
    fn request<'a>(&'a self, op: &'a str, body: Value) -> Work<'a, Result<Value, HostError>> {
        // Asked where it is called, as the JavaScript call was: the frame is
        // queued and its wait begins now.
        let sent = self.bridge.request(op, body, None);
        Box::pin(async move { sent.await.map_err(host_error) })
    }
}

impl EngineHost for BridgeHost {
    fn open(&self, open: OpenPane) -> Work<'_, Result<Opened, HostError>> {
        let argv = match cf_process::pane_argv(&open.argv, &self.env) {
            Ok(argv) => argv,
            // A window's program that cannot open as it is: the open fails
            // with the sentence, as the JavaScript threw it, and nothing is sent.
            Err(message) => {
                return Box::pin(async move {
                    Err(HostError {
                        error: None,
                        message,
                    })
                });
            }
        };
        let mut env = Map::new();
        for (name, value) in &open.env {
            env.insert(name.clone(), Value::String(value.clone()));
        }
        let body = json!({
            "id": open.pane.id,
            "generation": open.pane.generation,
            "cwd": open.cwd,
            "argv": argv,
            "env": env,
            "dropEnv": open.drop_env,
        });
        let sent = self.bridge.request("pane.open", body, Some(OPEN_DEADLINE));
        Box::pin(async move { sent.await.map(|answer| opened(&answer)).map_err(host_error) })
    }

    fn kill<'a>(&'a self, pane: &'a Pane) -> Work<'a, Result<Killed, HostError>> {
        let body = json!({ "id": pane.id, "generation": pane.generation });
        let sent = self.bridge.request("pane.kill", body, None);
        Box::pin(async move { sent.await.map(|answer| killed(&answer)).map_err(host_error) })
    }
}

/// What a request that never came back says: `eof` where the bridge ended, as
/// the host's end of it ending rejected every request (`error: 'eof'`), else
/// the transport's own words.
fn host_error(error: BridgeError) -> HostError {
    match error {
        BridgeError::Eof => HostError {
            error: Some("eof".to_owned()),
            message: "eof".to_owned(),
        },
        other => HostError {
            error: None,
            message: other.to_string(),
        },
    }
}

/// An open's answer: the pane is open (`ok: true`, its program's process id
/// when the host knows it), or the host says why not, or says nothing of why.
fn opened(answer: &Value) -> Opened {
    if answer.get("ok") == Some(&Value::Bool(true)) {
        let pid = answer
            .get("pid")
            .and_then(Value::as_u64)
            .and_then(|pid| u32::try_from(pid).ok());
        return Opened::Open { pid };
    }
    let error = match answer.get("error") {
        None | Some(Value::Null) => "no answer from the pane host".to_owned(),
        said => js::text(said).into_owned(),
    };
    Opened::Refused { error }
}

/// A kill's answer: taken (`ok: true`), or why not in the host's words.
fn killed(answer: &Value) -> Killed {
    if answer.get("ok") == Some(&Value::Bool(true)) {
        return Killed::Killed;
    }
    let error = match answer.get("error") {
        None | Some(Value::Null) => String::new(),
        said => js::text(said).into_owned(),
    };
    Killed::Refused { error }
}

/// The daemon's end of the bridge, as the engine's work needs it read: the
/// frames of each read are handled together, and then what they woke is run
/// to its end ([`DaemonSpawn::drain`]) before the next read, as Node ran its
/// microtasks after each `data` callback and before the next. A chain of the
/// engine's turns a frame began is over before the next read is handled.
pub fn daemon_bridge(spawn: &Rc<DaemonSpawn>) -> BridgeBuilder {
    let spawn = Rc::clone(spawn);
    BridgeBuilder::new(Role::Daemon).after_read(move || spawn.drain())
}

/// The host's `pane.exit` events, told to `exited` where each is read, in the
/// order they came and before the next frame: what the exit changes is
/// changed before the frame after it is looked at, and what it still has to do
/// (`exited` returns it) is spawned onto the executor, never waited for there,
/// and run by the drain after the frames of the read. A body that names no
/// pane exits nothing; a panic in `exited` is written down and the reader goes
/// on, as one that ends it would end the bridge.
pub fn watch_exits(
    bridge: &Bridge,
    spawn: Rc<DaemonSpawn>,
    exited: impl Fn(Pane) -> Option<LocalWork> + 'static,
) -> Subscription {
    bridge.on_event("pane.exit", move |body| {
        let Some(pane) = pane_of(body) else {
            return;
        };
        match contain_now(|| exited(pane)) {
            Ok(Some(rest)) => spawn.apart("a window's exit failed", rest),
            Ok(None) => {}
            Err(panicked) => spawn.errors().caught("a window's exit failed", &panicked),
        }
    })
}

/// The pane an exit names: `{id, generation}`.
fn pane_of(body: &Value) -> Option<Pane> {
    Some(Pane {
        id: body.get("id")?.as_str()?.to_owned(),
        generation: body.get("generation")?.as_u64()?,
    })
}

#[cfg(test)]
mod tests;

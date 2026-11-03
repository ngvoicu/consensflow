//! The pane host as the engine uses it, over the bridge: a window's pane is
//! opened and killed by a request to the host, an adapter's own requests
//! (`pane.snapshot`, `pane.input`, `pane.write_paste`, `pane.claim`) go
//! straight through, where a window's turn begins is told to it and what the turn
//! left running is asked of it (`pane.mark`, `pane.commands`,
//! `pane.rest`: [`BackgroundWork`]; it ends none of it, a window's close does), and the
//! host's `pane.exit` events are told to the engine where they are read.
//!
//! The protocol is the app's (`crates/cf-panes/src/pane_handlers.rs`); this
//! adds no rules of its own, only the Windows shim a window's program may be
//! ([`cf_process::pane_argv`]), the bound on the wait for an open, and the bound on the wait
//! for a paste into a window whose harness declared how it reads one.

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Mutex;
use std::time::Duration;

use cf_base::env::Env;
use cf_base::js;
use cf_bridge::local::{Bridge, BridgeBuilder, Subscription};
use cf_bridge::BridgeError;
use cf_engine::host::{EngineHost, Killed, OpenPane, Opened};
use cf_engine::runtime::LocalWork;
use cf_engine::seams::{BackgroundWork, CommandsSeen};
use cf_harness::contract::{HostError, Pane, PaneHost, Work};
use cf_proto::bridge::{Role, DEADLINE};
use cf_proto::panes::{
    Background, CommandsAnswer, CommandsRequest, MarkRequest, PaneExit, Rest, RestRequest, Servers,
    SilentPaste,
};
use serde_json::{json, Map, Value};

use crate::errors::contain_now;
use crate::seams::DaemonSpawn;

/// How long the host has to open a window: the pane's program is started and
/// watched before it answers (`deadlineMs: 60_000`, the engine's
/// `Limits::open_ms` unless the environment cut it).
pub const OPEN_DEADLINE: Duration = Duration::from_secs(60);

/// The least a paste into a window has to be answered: the bridge's own default for a request.
/// The pane host holds a delivery's Enter for 2 s at the most in a window whose harness
/// declared nothing, and a paste is answered once its Enter is written.
pub const PASTE_DEADLINE: Duration = Duration::from_secs(30);

/// What a paste into a window whose harness declared how it reads one has beyond the most the
/// pane host holds its Enter for (the declared cap for the paste's length, `SilentPaste::most_ms`):
/// the time to write the Enter and for the answer to come back. A host that has not answered by
/// then did not hold the Enter for the window's read, and the daemon stops waiting.
pub const PASTE_MARGIN: Duration = Duration::from_secs(5);

/// What the engine says to the host of the app, through its bridge.
pub struct BridgeHost {
    bridge: Bridge,
    env: Env,
    open_deadline: Duration,
    /// How each pane's harness declared that it reads a paste, by the pane's id and the
    /// generation it was opened as: said by the engine as the window opened ([`OpenPane`]),
    /// and what a paste's time to be answered is worked out from. An entry stays until the
    /// same pane id is opened again.
    declared: Mutex<HashMap<String, (u64, SilentPaste)>>,
}

impl BridgeHost {
    /// The host on the other end of `bridge`; `env` is the daemon's, which
    /// finds the shim a window's program may be. It has [`OPEN_DEADLINE`] to
    /// answer an open.
    pub fn new(bridge: Bridge, env: Env) -> Self {
        Self {
            bridge,
            env,
            open_deadline: OPEN_DEADLINE,
            declared: Mutex::new(HashMap::new()),
        }
    }

    /// The host, which has `deadline` to answer an open.
    #[must_use]
    pub fn opening_within(mut self, deadline: Duration) -> Self {
        self.open_deadline = deadline;
        self
    }

    /// Notes how the harness of `pane` reads a paste, or that it declared nothing.
    fn declare(&self, pane: &Pane, silent: Option<SilentPaste>) {
        if let Ok(mut declared) = self.declared.lock() {
            match silent {
                Some(silent) => {
                    declared.insert(pane.id.clone(), (pane.generation, silent));
                }
                None => {
                    declared.remove(&pane.id);
                }
            }
        }
    }

    /// How long a request has to be answered, where that is more than the bridge's own default:
    /// a paste into a window whose harness declared how it reads one is held at the pane host
    /// for that harness's cap for the paste's length, so it has the cap and [`PASTE_MARGIN`],
    /// and [`PASTE_DEADLINE`] at the least. Any other request, or paste, has the bridge's.
    fn deadline_of(&self, op: &str, body: &Value) -> Option<Duration> {
        if op != "pane.write_paste" {
            return None;
        }
        let id = body.get("id")?.as_str()?;
        let generation = body.get("generation")?.as_u64()?;
        let silent = {
            let declared = self.declared.lock().ok()?;
            let (opened, silent) = declared.get(id)?;
            (*opened == generation).then_some(*silent)?
        };
        let characters = body
            .get("body")
            .and_then(Value::as_str)
            .map_or(0, |text| text.chars().count());
        let cap = Duration::from_millis(silent.most_ms(characters));
        Some(cap.saturating_add(PASTE_MARGIN).max(PASTE_DEADLINE))
    }
}

impl PaneHost for BridgeHost {
    fn request<'a>(&'a self, op: &'a str, body: Value) -> Work<'a, Result<Value, HostError>> {
        // Asked where it is called, as the JavaScript call was: the frame is
        // queued and its wait begins now.
        let deadline = self.deadline_of(op, &body);
        let sent = self.bridge.request(op, body, deadline);
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
        let mut body = json!({
            "id": open.pane.id,
            "generation": open.pane.generation,
            "cwd": open.cwd,
            "argv": argv,
            "env": env,
            "dropEnv": open.drop_env,
        });
        // Said only for a harness that reads a paste silently: every other open is as Node
        // sent it.
        if let Some(silent) = open.silent_paste {
            body["silentPaste"] = json!(silent);
        }
        self.declare(&open.pane, open.silent_paste);
        let sent = self
            .bridge
            .request("pane.open", body, Some(self.open_deadline));
        Box::pin(async move { sent.await.map(|answer| opened(&answer)).map_err(host_error) })
    }

    fn kill<'a>(&'a self, pane: &'a Pane) -> Work<'a, Result<Killed, HostError>> {
        let body = json!({ "id": pane.id, "generation": pane.generation });
        let sent = self.bridge.request("pane.kill", body, None);
        Box::pin(async move { sent.await.map(|answer| killed(&answer)).map_err(host_error) })
    }

    fn quiet<'a>(&'a self, pane: &'a Pane) -> Work<'a, Result<Option<f64>, HostError>> {
        let body = json!({ "id": pane.id, "generation": pane.generation });
        let sent = self.bridge.request("pane.snapshot", body, None);
        Box::pin(async move {
            sent.await
                .map(|answer| quiet_ms(&answer))
                .map_err(host_error)
        })
    }
}

/// How long the host says a pane has printed nothing: its `pane.snapshot`'s
/// `outputQuietMs`, none where the pane has printed nothing (null) or the answer has no
/// such field.
fn quiet_ms(answer: &Value) -> Option<f64> {
    answer.get("outputQuietMs").and_then(Value::as_f64)
}

impl BackgroundWork for BridgeHost {
    fn mark<'a>(&'a self, pane: &'a Pane, servers: Servers, harness: &'a str) -> Work<'a, ()> {
        // How the harness keeps its servers, and which harness it is, are said only
        // where they are known: a window whose harness says nothing is marked with
        // the request a host has always had.
        let request = MarkRequest {
            id: pane.id.clone(),
            generation: pane.generation,
            servers,
            harness: (!harness.is_empty()).then(|| harness.to_owned()),
        };
        let sent = serde_json::to_value(&request)
            .ok()
            .map(|body| self.bridge.request("pane.mark", body, None));
        // A host that cannot mark has no background work to tell of: nothing
        // the engine can do about it, and nothing it should fail for.
        Box::pin(async move {
            if let Some(sent) = sent {
                let _ = sent.await;
            }
        })
    }

    fn commands<'a>(
        &'a self,
        pane: &'a Pane,
        since: Option<u64>,
        after: Option<i64>,
    ) -> Work<'a, CommandsSeen> {
        let request = CommandsRequest {
            id: pane.id.clone(),
            generation: pane.generation,
            since,
            after,
        };
        let sent = serde_json::to_value(&request)
            .ok()
            .map(|body| self.bridge.request("pane.commands", body, None));
        Box::pin(async move {
            match sent {
                Some(sent) => commands_seen(sent.await.ok().as_ref()),
                None => CommandsSeen::Unanswered,
            }
        })
    }

    fn rest<'a>(&'a self, pane: &'a Pane, keep: &'a [u64]) -> Work<'a, Option<u64>> {
        let request = RestRequest {
            id: pane.id.clone(),
            generation: pane.generation,
            keep: keep.to_vec(),
        };
        let sent = serde_json::to_value(&request)
            .ok()
            .map(|body| self.bridge.request("pane.rest", body, None));
        // A host that keeps none, does not answer or is older keeps no rest.
        Box::pin(async move {
            let answer = sent?.await.ok()?;
            let rest: Rest = serde_json::from_value(answer.get("rest")?.clone()).ok()?;
            rest
        })
    }

    fn left_running<'a>(&'a self, pane: &'a Pane) -> Work<'a, Option<Background>> {
        let body = json!({ "id": pane.id, "generation": pane.generation });
        let sent = self.bridge.request("pane.background", body, None);
        // A host that does not answer, or answers what is no look, has nothing to name.
        Box::pin(async move { serde_json::from_value(sent.await.ok()?).ok() })
    }
}

/// What `pane.commands`' answer says (none where no answer came): so many commands run,
/// the host cannot tell them (`commands: null`), a process of the window cannot be read
/// (`null` and `unreadable`), the window's processes kept changing as it read them (`null`
/// and `unsettled`), or nothing that is an answer (an older host's, which knows no such
/// request, among it).
fn commands_seen(answer: Option<&Value>) -> CommandsSeen {
    let Some(answer) = answer.filter(|answer| answer.get("commands").is_some()) else {
        return CommandsSeen::Unanswered;
    };
    match serde_json::from_value::<CommandsAnswer>(answer.clone()) {
        Ok(CommandsAnswer {
            commands: Some(commands),
            ..
        }) => CommandsSeen::Running(commands.total),
        Ok(CommandsAnswer {
            commands: None,
            unreadable: true,
            ..
        }) => CommandsSeen::Unreadable,
        Ok(CommandsAnswer {
            commands: None,
            unsettled: true,
            ..
        }) => CommandsSeen::Unsettled,
        Ok(CommandsAnswer { commands: None, .. }) => CommandsSeen::Untold,
        Err(_) => CommandsSeen::Unanswered,
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
/// when the host knows it), or none came by the open's deadline, or the host says
/// why not, or says nothing of why.
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
    // The bridge's own word for a request it stopped waiting for: the host never
    // said no, and may still start the window.
    if error == DEADLINE {
        return Opened::Unanswered;
    }
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
/// pane exits nothing; one that says no more than which pane ended (an older
/// host's) is an exit all the same, as is one with a field that is not what it
/// should be, which was not said. A panic in `exited` is written down and the
/// reader goes on, as one that ends it would end the bridge.
pub fn watch_exits(
    bridge: &Bridge,
    spawn: Rc<DaemonSpawn>,
    exited: impl Fn(PaneExit) -> Option<LocalWork> + 'static,
) -> Subscription {
    bridge.on_event("pane.exit", move |body| {
        let Ok(exit) = serde_json::from_value::<PaneExit>(body.clone()) else {
            return;
        };
        match contain_now(|| exited(exit)) {
            Ok(Some(rest)) => spawn.apart("a window's exit failed", rest),
            Ok(None) => {}
            Err(panicked) => spawn.errors().caught("a window's exit failed", &panicked),
        }
    })
}

#[cfg(test)]
mod tests;

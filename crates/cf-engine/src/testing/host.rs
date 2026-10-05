//! A pane host that opens nothing real and ends panes when told to: the
//! twin of `fakeHost` in `core-dispatcher.test.mjs`. What a call does and
//! reads, it does when called, as the JavaScript fake's async functions did
//! up to their first wait; each answer comes a turn later, as their promises
//! did, and a turn more for each wait inside them (`open` waits on its hold,
//! `kill` on each engine told of the exit); and every call is written down in
//! the Node traces' shape.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use cf_harness::contract::{HostError, Pane, PaneHost, Work};
use serde_json::{json, Map, Value};

use super::executor::Gate;
use super::recorder::Recorder;
use crate::dispatcher::Dispatcher;
use crate::host::{EngineHost, Killed, OpenPane, Opened};
use crate::runtime::next_turn;

/// What the test's pane host does, and what it was asked.
#[derive(Default)]
pub struct FakeHost {
    recorder: Recorder,
    /// Where exits go: each engine made, in order (`host.onExit` of each).
    engines: RefCell<Vec<Weak<Dispatcher>>>,
    opened: RefCell<Vec<OpenPane>>,
    killed: RefCell<Vec<Pane>>,
    requests: RefCell<Vec<(String, Value)>>,
    /// The next open is refused.
    pub refuse: Cell<bool>,
    /// A kill is refused: the window stays, and no exit comes.
    pub refuse_kills: Cell<bool>,
    /// An open waits for this gate.
    pub hold: RefCell<Option<Gate>>,
    /// A kill's exit is held until the test sends it.
    pub hold_exits: Cell<bool>,
    /// What `pane.snapshot` answers besides `ok`.
    pub snapshot: RefCell<Map<String, Value>>,
    /// The window's process, when the test names one.
    pub pid: Cell<Option<u32>>,
}

impl FakeHost {
    pub fn new(recorder: Recorder) -> Rc<Self> {
        Rc::new(Self {
            recorder,
            ..Self::default()
        })
    }

    /// Exits go to `engine` too (`host.onExit`).
    pub fn attach(&self, engine: &Rc<Dispatcher>) {
        self.engines.borrow_mut().push(Rc::downgrade(engine));
    }

    /// The panes opened, in order.
    pub fn opened(&self) -> Vec<OpenPane> {
        self.opened.borrow().clone()
    }

    /// The panes killed, in order.
    pub fn killed(&self) -> Vec<Pane> {
        self.killed.borrow().clone()
    }

    /// The requests made, in order.
    pub fn requests(&self) -> Vec<(String, Value)> {
        self.requests.borrow().clone()
    }

    /// The last pane opened for `handle`'s window: its own, or one of its sessions'.
    pub fn last(&self, handle: &str) -> Option<OpenPane> {
        self.opened
            .borrow()
            .iter()
            .rev()
            .find(|open| window_of(&open.pane.id, handle))
            .cloned()
    }

    /// The last window of `handle` exits, as the pane host says it.
    pub async fn exit(&self, handle: &str) {
        if let Some(open) = self.last(handle) {
            self.exited(open.pane).await;
        }
    }

    /// Tells each engine `pane` ended, one after the other, and waits for
    /// what each does about it.
    pub async fn exited(&self, pane: Pane) {
        let engines: Vec<Rc<Dispatcher>> = self
            .engines
            .borrow()
            .iter()
            .filter_map(Weak::upgrade)
            .collect();
        // Each engine writes down the exit as it begins (`on_operation`).
        for engine in engines {
            if let Some(rest) = engine.pane_exited(pane.clone()) {
                rest.await;
            }
            // `await listener(...)`: the call returned a turn before the loop goes on.
            next_turn().await;
        }
    }
}

/// Whether a pane id is a window of `handle`: its own, or one of its sessions'.
pub fn window_of(id: &str, handle: &str) -> bool {
    if id.ends_with(&format!("-{handle}")) {
        return true;
    }
    let Some(rest) = id.strip_prefix('p') else {
        return false;
    };
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    digits > 0
        && rest[digits..]
            .strip_prefix('-')
            .is_some_and(|participant| participant.starts_with(&format!("{handle}-")))
}

/// A pane as the Node traces write it.
fn pane_json(pane: &Pane) -> Value {
    json!({ "id": pane.id, "generation": pane.generation })
}

/// An open as the Node traces write its body.
fn open_json(open: &OpenPane) -> Value {
    let env: Map<String, Value> = open
        .env
        .iter()
        .map(|(name, value)| (name.clone(), json!(value)))
        .collect();
    json!({
        "id": open.pane.id,
        "generation": open.pane.generation,
        "cwd": open.cwd,
        "argv": open.argv,
        "env": env,
        "dropEnv": open.drop_env,
    })
}

impl PaneHost for FakeHost {
    fn request<'a>(&'a self, op: &'a str, body: Value) -> Work<'a, Result<Value, HostError>> {
        let at = self
            .recorder
            .call("host", Some("request"), json!([op, body.clone()]));
        self.requests.borrow_mut().push((op.to_owned(), body));
        let mut answer = Map::new();
        answer.insert("ok".to_owned(), json!(true));
        if op == "pane.snapshot" {
            answer.extend(self.snapshot.borrow().clone());
        }
        let answer = Value::Object(answer);
        Box::pin(async move {
            next_turn().await;
            self.recorder.answered(at, answer.clone());
            Ok(answer)
        })
    }
}

impl EngineHost for FakeHost {
    fn open(&self, open: OpenPane) -> Work<'_, Result<Opened, HostError>> {
        let at = self
            .recorder
            .call("host", Some("open"), json!([open_json(&open)]));
        Box::pin(async move {
            if self.refuse.replace(false) {
                next_turn().await;
                let error = "refused by the test".to_owned();
                self.recorder
                    .answered(at, json!({ "ok": false, "error": error }));
                return Ok(Opened::Refused { error });
            }
            let hold = self.hold.borrow().clone();
            match hold {
                Some(gate) => gate.wait().await,
                None => next_turn().await,
            }
            let pane = open.pane.clone();
            self.opened.borrow_mut().push(open);
            let pid = self.pid.get();
            let mut answer = json!({ "ok": true, "id": pane.id, "generation": pane.generation });
            if let Some(pid) = pid {
                answer["pid"] = json!(pid);
            }
            self.recorder.answered(at, answer);
            // The JavaScript fake waited on its hold, and its answer reached
            // the caller a turn after it returned.
            next_turn().await;
            Ok(Opened::Open { pid })
        })
    }

    fn kill<'a>(&'a self, pane: &'a Pane) -> Work<'a, Result<Killed, HostError>> {
        let at = self
            .recorder
            .call("host", Some("kill"), json!([pane_json(pane)]));
        self.killed.borrow_mut().push(pane.clone());
        Box::pin(async move {
            if self.refuse_kills.get() {
                next_turn().await;
                let error = "refused by the test".to_owned();
                self.recorder
                    .answered(at, json!({ "ok": false, "error": error }));
                return Ok(Killed::Refused { error });
            }
            // A killed process is gone before the next pass, so its exit
            // lands at once; a test that wants the gap holds it and sends it.
            if !self.hold_exits.get() {
                self.exited(pane.clone()).await;
            }
            next_turn().await;
            self.recorder.answered(at, json!({ "ok": true }));
            Ok(Killed::Killed)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pane_is_a_window_of_its_participant_or_of_one_of_its_sessions() {
        assert!(window_of("p1-zeus", "zeus"));
        assert!(window_of("p1-zeus-amber-pine", "zeus"));
        assert!(window_of("p12-zeus-amber-pine", "zeus-amber-pine"));
        assert!(!window_of("p1-zeusx", "zeus"));
        assert!(!window_of("p1-diana", "zeus"));
        assert!(!window_of("x1-zeus-amber", "zeus-a"));
    }
}

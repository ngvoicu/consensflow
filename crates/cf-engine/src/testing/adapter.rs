//! A harness adapter whose agents do exactly what the test tells them: the
//! twin of `fakeAdapter` in `core-dispatcher.test.mjs`. An adapter answers
//! for any harness the test names it under ([`super::FakeAdapters`]); each
//! window is an agent, found by its launch, and by the handle it is for.
//! What a call does and reads, it does when called, as the JavaScript fake's
//! async functions did; each is answered a turn later, as their promises
//! were, and written down in the Node traces' shape, under the harness it
//! was asked as.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use cf_harness::contract::{Adapter, Launch, Prepared, Readiness, Waiting, Window, Work};
use cf_harness::records::{Item, Quota, Reading, Record, Role, Settlement};
use serde_json::{json, Value};

use super::executor::{next_turn, Gate};
use super::recorder::Recorder;
use super::window::FakeWindow;

/// An agent's window, as the test tells it to be.
#[derive(Debug, Clone)]
pub struct FakeAgent {
    pub launch: String,
    pub handle: String,
    /// The conversation its launch opened on.
    pub native: String,
    pub items: Vec<Item>,
    pub settled: bool,
    pub waiting: Option<Waiting>,
    pub quota: Option<Arc<Quota>>,
    /// Whether a delivery is taken.
    pub admit: bool,
    /// Whether a delivery taken shows in the record.
    pub arrive: bool,
    /// Whether the harness's own queue takes a delivery.
    pub queued: bool,
    pub failed: bool,
    /// The conversation the window shows, when the human switched it.
    pub shows: Option<String>,
    /// Each conversation's record, once the window switched away from it.
    pub records: HashMap<String, Vec<Item>>,
    /// Why the window has not said which conversation it shows yet.
    pub unnamed: Option<String>,
}

/// What a test makes `ready` answer, where it gives the adapter one.
pub type Ready = Rc<dyn Fn() -> Result<Readiness, String>>;

/// What a test makes `started` answer, where it gives the adapter one: the
/// conversation the harness named, or why the window could not take its
/// first message.
pub type Started = Rc<dyn Fn() -> Result<Option<String>, String>>;

/// The test's adapter, and its agents.
pub struct FakeAdapter {
    pub(super) recorder: Recorder,
    agents: RefCell<Vec<FakeAgent>>,
    /// Each launch prepared, as asked, in the shape of the JavaScript
    /// fake's `prepared`: its launch id, participant's handle, role,
    /// project, folder, resume, message, agent and instructions.
    prepared: RefCell<Vec<Value>>,
    /// The test's items are numbered across its adapters.
    items: Rc<Cell<u64>>,
    /// A `ready` of the test's own; without one a window is ready, and
    /// nothing is asked (the JavaScript fake had none).
    pub ready: RefCell<Option<Ready>>,
    /// A `started` of the test's own.
    pub started: RefCell<Option<Started>>,
    /// Every `prepare` fails with this, and prepares nothing, until the test
    /// takes it away.
    pub prepare_fails: RefCell<Option<String>>,
    /// How many `prepare`s failed so.
    failed_prepares: Cell<usize>,
    /// Every `deliver` waits for this gate, and is made once it opens.
    pub hold_deliveries: RefCell<Option<Gate>>,
    /// Every `observe` of the window of this launch waits for the gate, and
    /// is made once it opens.
    pub hold_observes: RefCell<Option<(String, Gate)>>,
}

impl FakeAdapter {
    pub fn new(recorder: Recorder) -> Rc<Self> {
        Self::numbering(recorder, Rc::new(Cell::new(0)))
    }

    /// Another adapter of the same test, with agents of its own: Codex's
    /// own fake (`fakeAdapter('codex')`), its items numbered with this one's.
    pub fn another(&self) -> Rc<Self> {
        Self::numbering(self.recorder.clone(), Rc::clone(&self.items))
    }

    fn numbering(recorder: Recorder, items: Rc<Cell<u64>>) -> Rc<Self> {
        Rc::new(Self {
            recorder,
            agents: RefCell::new(Vec::new()),
            prepared: RefCell::new(Vec::new()),
            items,
            ready: RefCell::new(None),
            started: RefCell::new(None),
            prepare_fails: RefCell::new(None),
            failed_prepares: Cell::new(0),
            hold_deliveries: RefCell::new(None),
            hold_observes: RefCell::new(None),
        })
    }

    /// How many `prepare`s failed ([`FakeAdapter::prepare_fails`]).
    pub fn failed_prepares(&self) -> usize {
        self.failed_prepares.get()
    }

    /// The launches prepared, in order.
    pub fn prepared(&self) -> Vec<Value> {
        self.prepared.borrow().clone()
    }

    /// An item of a conversation, numbered as the test's own: `i-1`, `i-2`…
    pub fn item(&self, role: Role, text: &str) -> Item {
        self.items.set(self.items.get() + 1);
        Item {
            id: Arc::from(format!("i-{}", self.items.get())),
            role,
            text: Arc::from(text),
            complete: role == Role::Assistant,
            at: None,
            commentary: false,
        }
    }

    /// Changes the latest window of `handle`: its own, or its newest session's.
    pub fn with<T>(&self, handle: &str, change: impl FnOnce(&mut FakeAgent) -> T) -> T {
        let mut agents = self.agents.borrow_mut();
        let agent = agents
            .iter_mut()
            .rev()
            .find(|agent| agent.handle == handle || agent.handle.starts_with(&format!("{handle}-")))
            .unwrap_or_else(|| panic!("no window of {handle}"));
        change(agent)
    }

    /// The latest window of `handle`, as it is now.
    pub fn agent(&self, handle: &str) -> FakeAgent {
        self.with(handle, |agent| agent.clone())
    }

    /// The agent answers: its turn ends with `text`.
    pub fn answer(&self, handle: &str, text: &str) {
        let item = self.item(Role::Assistant, text);
        self.with(handle, |agent| {
            agent.items.push(item);
            agent.settled = true;
        });
    }

    /// The agent is at work on a turn.
    pub fn busy(&self, handle: &str) {
        self.with(handle, |agent| agent.settled = false);
    }

    /// What the window's harness says of its quota.
    pub fn quota(&self, handle: &str, quota: Option<Quota>) {
        self.with(handle, |agent| agent.quota = quota.map(Arc::new));
    }

    /// The harness refuses the turn, as Claude Code writes its limit: the
    /// turn ends on the refusal, which the record reads as a failure.
    pub fn refuse(&self, handle: &str, quota: Quota) {
        let mut item = self.item(Role::Assistant, "You've hit your weekly limit");
        if let Quota::Exhausted { at: Some(at), .. } = &quota {
            item.at = Some(json!(at));
        }
        self.with(handle, |agent| {
            agent.items.push(item);
            agent.quota = Some(Arc::new(quota));
            agent.failed = true;
            agent.settled = true;
        });
    }

    /// A turn gets through: the record writes the agent's words, with their time.
    pub fn writes(&self, handle: &str, text: &str, at: &str) {
        let mut item = self.item(Role::Assistant, text);
        item.at = Some(json!(at));
        item.complete = false;
        self.with(handle, |agent| agent.items.push(item));
    }

    /// The human switches the window to another conversation: from then on
    /// it writes that one's record, idle at first.
    pub fn switch_to(&self, handle: &str, native: &str) {
        self.with(handle, |agent| {
            let shown = agent.shows.clone().unwrap_or_else(|| agent.native.clone());
            let items = std::mem::take(&mut agent.items);
            agent.records.insert(shown, items);
            agent.items = agent.records.get(native).cloned().unwrap_or_default();
            agent.shows = Some(native.to_owned());
            agent.settled = true;
        });
    }

    pub(super) fn of_launch<T>(&self, launch: &str, read: impl FnOnce(&mut FakeAgent) -> T) -> T {
        let mut agents = self.agents.borrow_mut();
        let agent = agents
            .iter_mut()
            .find(|agent| agent.launch == launch)
            .unwrap_or_else(|| panic!("no window of launch {launch}"));
        read(agent)
    }

    /// What the record of `session` holds (`adapter.record`): the latest
    /// window's on it.
    pub(super) fn record_of(&self, session: &str) -> Reading {
        let agents = self.agents.borrow();
        match agents.iter().rev().find(|agent| agent.native == session) {
            None => Reading::Unknown("unknown".to_owned()),
            Some(agent) => Reading::Known(Record {
                items: agent.items.clone(),
                in_flight: false,
                asking: false,
                failed: false,
                quota: None,
                settlement: Settlement::Unknown,
            }),
        }
    }
}

/// The adapter as the engine asks it for one harness.
pub(super) struct Asked {
    harness: String,
    fake: Rc<FakeAdapter>,
}

impl Asked {
    pub(super) fn new(harness: &str, fake: Rc<FakeAdapter>) -> Self {
        Self {
            harness: harness.to_owned(),
            fake,
        }
    }
}

impl Adapter for Asked {
    fn prepare<'a>(&'a self, launch: &'a Launch<'a>) -> Work<'a, Result<Prepared, String>> {
        let fake = &self.fake;
        let seam = format!("adapter:{}", self.harness);
        let asked = json!({
            "launchId": launch.id.as_str(),
            "participant": { "handle": launch.handle },
            "role": launch.role,
            "project": { "id": launch.project },
            "directory": launch.directory,
            "resume": launch.resume,
            "message": launch.message,
            "agent": launch.agent.map(|agent| json!({
                "model": agent.model,
                "effort": agent.effort,
                "thinking": agent.thinking,
                "designer": agent.designer,
            })),
            "instructions": launch.instructions,
        });
        let at = fake
            .recorder
            .call(&seam, Some("prepare"), json!([asked.clone()]));
        // A failing one prepared nothing: no agent, and nothing prepared.
        let failing = fake.prepare_fails.borrow().clone();
        if let Some(reason) = failing {
            fake.failed_prepares.set(fake.failed_prepares.get() + 1);
            return Box::pin(async move {
                next_turn().await;
                fake.recorder
                    .answered(at, json!({ "$error": { "message": reason } }));
                Err(reason)
            });
        }
        fake.prepared.borrow_mut().push(asked);
        let id = launch.id.as_str().to_owned();
        let native = launch
            .resume
            .map_or_else(|| format!("native-{id}"), str::to_owned);
        let mut agent = FakeAgent {
            launch: id.clone(),
            handle: launch.handle.to_owned(),
            native: native.clone(),
            items: Vec::new(),
            settled: true,
            waiting: None,
            quota: None,
            admit: true,
            arrive: true,
            queued: false,
            failed: false,
            shows: None,
            records: HashMap::new(),
            unnamed: None,
        };
        if let Some(message) = launch.message {
            agent.items.push(fake.item(Role::User, message));
            agent.settled = false;
        }
        fake.agents.borrow_mut().push(agent);
        let handle = launch.handle.to_owned();
        let window: Rc<dyn Window> = Rc::new(FakeWindow::new(
            seam,
            id.clone(),
            native.clone(),
            Rc::clone(fake),
        ));
        Box::pin(async move {
            next_turn().await;
            fake.recorder.answered(
                at,
                json!({
                    "argv": ["/bin/fake-agent", handle],
                    "env": { "FAKE_AGENT": handle },
                    "dropEnv": [],
                    "nativeSession": native,
                    "launch": { "launchId": id, "nativeSession": native },
                }),
            );
            Ok(Prepared {
                argv: vec!["/bin/fake-agent".to_owned(), handle.clone()],
                env: vec![("FAKE_AGENT".to_owned(), handle)],
                drop_env: Vec::new(),
                native_session: Some(native),
                window,
            })
        })
    }
}

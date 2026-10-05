//! A harness adapter whose agents do exactly what the test tells them: the
//! twin of `fakeAdapter` in `core-dispatcher.test.mjs`. One adapter answers
//! for any harness the test names it under; each window is an agent, found
//! by its launch, and by the handle it is for. What a call does and reads, it
//! does when called, as the JavaScript fake's async functions did; each is
//! answered a turn later, as their promises were, and written down in the
//! Node traces' shape, under the harness it was asked as.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use cf_harness::contract::{
    Adapter, Admission, Launch, Observed, Pane, PaneHost, Prepared, Readiness, Records, Waiting,
    Window, Work,
};
use cf_harness::records::{Item, Options, Quota, Reading, Record, Role, Settlement};
use cf_proto::agents::Harness;
use serde_json::{json, Value};

use super::recorder::Recorder;
use crate::runtime::next_turn;
use crate::seams::Adapters;

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

/// What a test makes `prepare` do, where it gives the adapter one: told the
/// launch as asked, it may fail it, before any window is made.
pub type Prepare = Rc<dyn Fn(&Value) -> Result<(), String>>;

/// The test's adapter, and its agents.
pub struct FakeAdapter {
    recorder: Recorder,
    agents: RefCell<Vec<FakeAgent>>,
    /// Each launch prepared, as asked, in the shape of the JavaScript
    /// fake's `prepared`: its launch id, participant's handle, role,
    /// project, folder, resume, message, agent and instructions.
    prepared: RefCell<Vec<Value>>,
    items: Cell<u64>,
    /// A `ready` of the test's own; without one a window is ready, and
    /// nothing is asked (the JavaScript fake had none).
    pub ready: RefCell<Option<Ready>>,
    /// A `prepare` of the test's own, that fails some launches.
    pub prepare: RefCell<Option<Prepare>>,
}

impl FakeAdapter {
    pub fn new(recorder: Recorder) -> Rc<Self> {
        Rc::new(Self {
            recorder,
            agents: RefCell::new(Vec::new()),
            prepared: RefCell::new(Vec::new()),
            items: Cell::new(0),
            ready: RefCell::new(None),
            prepare: RefCell::new(None),
        })
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

    fn of_launch<T>(&self, launch: &str, read: impl FnOnce(&mut FakeAgent) -> T) -> T {
        let mut agents = self.agents.borrow_mut();
        let agent = agents
            .iter_mut()
            .find(|agent| agent.launch == launch)
            .unwrap_or_else(|| panic!("no window of launch {launch}"));
        read(agent)
    }

    /// What the record of `session` holds (`adapter.record`): the latest
    /// window's on it.
    fn record_of(&self, session: &str) -> Reading {
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
struct Asked {
    harness: String,
    fake: Rc<FakeAdapter>,
}

/// One window of the fake: its agent, by its launch, and the conversation
/// the window is followed on (the launch bag's `nativeSession`).
struct FakeWindow {
    seam: String,
    launch: String,
    session: RefCell<String>,
    fake: Rc<FakeAdapter>,
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
        // A test's own `prepare` fails before the real one runs: nothing is
        // prepared and no window made, as where it threw in JavaScript.
        let own = fake.prepare.borrow().clone();
        let overridden = own.is_some();
        if let Some(Err(reason)) = own.map(|own| own(&asked)) {
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
        let window: Rc<dyn Window> = Rc::new(FakeWindow {
            seam,
            launch: id.clone(),
            session: RefCell::new(native.clone()),
            fake: Rc::clone(fake),
        });
        Box::pin(async move {
            next_turn().await;
            // A test's own `prepare` is an async function that returns the
            // real one's promise: its answer takes two turns more to settle.
            if overridden {
                next_turn().await;
                next_turn().await;
            }
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

impl Window for FakeWindow {
    fn opened(&self, _pid: Option<u32>) {}

    fn follow(&self, session: &str) {
        *self.session.borrow_mut() = session.to_owned();
    }

    fn started(&self) -> Work<'_, Result<Option<String>, String>> {
        let at = self.call(
            "started",
            json!([{ "launch": { "launchId": self.launch } }]),
        );
        Box::pin(async move {
            next_turn().await;
            self.fake.recorder.answered(at, json!({}));
            Ok(None)
        })
    }

    fn ready<'a>(
        &'a self,
        _host: &'a dyn PaneHost,
        _pane: &'a Pane,
    ) -> Work<'a, Result<Readiness, String>> {
        let ready = self.fake.ready.borrow().clone();
        let Some(ready) = ready else {
            return Box::pin(async { Ok(Readiness::Ready) });
        };
        let at = self.call("ready", json!([{ "launch": { "launchId": self.launch } }]));
        let answer = ready();
        Box::pin(async move {
            next_turn().await;
            let written = match &answer {
                Ok(Readiness::Ready) => json!(true),
                Ok(Readiness::Held(held)) => json!(held.sentence()),
                Err(error) => json!({ "$error": { "message": error } }),
            };
            self.fake.recorder.answered(at, written);
            answer
        })
    }

    fn deliver<'a>(
        &'a self,
        _host: &'a dyn PaneHost,
        pane: &'a Pane,
        text: &'a str,
    ) -> Work<'a, Result<Admission, String>> {
        let at = self.call(
            "deliver",
            json!([{
                "launch": { "launchId": self.launch },
                "pane": { "id": pane.id, "generation": pane.generation },
                "text": text,
            }]),
        );
        let admission = self.fake.of_launch(&self.launch, |agent| {
            if !agent.admit {
                return Admission::Refused {
                    reason: "refused by the test".to_owned(),
                };
            }
            if agent.arrive {
                agent.items.push(self.fake.item(Role::User, text));
                agent.settled = false;
                // A message that arrives starts a turn: the one before it ended as it did.
                agent.failed = false;
            }
            Admission::Admitted {
                queued: agent.queued,
            }
        });
        Box::pin(async move {
            next_turn().await;
            let written = match &admission {
                Admission::Admitted { queued: true } => json!({ "admitted": true, "queued": true }),
                Admission::Admitted { queued: false } => json!({ "admitted": true }),
                Admission::Refused { reason } | Admission::Uncertain { reason } => {
                    json!({ "admitted": false, "reason": reason })
                }
            };
            self.fake.recorder.answered(at, written);
            Ok(admission)
        })
    }

    fn observe(&self) -> Work<'_, Result<Observed, String>> {
        // JavaScript handed the fake the participant's conversation, which
        // this window follows: its session.
        let session = self.session.borrow().clone();
        let at = self.call(
            "observe",
            json!([{
                "launch": { "launchId": self.launch },
                "conversation": { "nativeSession": session },
            }]),
        );
        let observed = self
            .fake
            .of_launch(&self.launch, |agent| looked(agent, &session));
        Box::pin(async move {
            next_turn().await;
            self.fake.recorder.answered(at, observed_json(&observed));
            Ok(observed)
        })
    }
}

impl FakeWindow {
    fn call(&self, method: &str, args: Value) -> usize {
        self.fake.recorder.call(&self.seam, Some(method), args)
    }
}

/// What a look at `agent`'s window finds, its launch on `session`.
fn looked(agent: &FakeAgent, session: &str) -> Observed {
    let reading = |items: Vec<Item>| {
        Some(Arc::new(Reading::Known(Record {
            items,
            in_flight: false,
            asking: false,
            failed: false,
            quota: None,
            settlement: Settlement::Unknown,
        })))
    };
    // A window that shows another conversation than its launch's: that
    // record's last look, and which session it shows now.
    if let Some(shows) = agent.shows.as_ref().filter(|shows| *shows != session) {
        return Observed {
            reading: reading(agent.records.get(session).cloned().unwrap_or_default()),
            settled: false,
            waiting: None,
            failed: agent.failed,
            quota: agent.quota.clone(),
            switched: Some(shows.clone()),
            unnamed: false,
        };
    }
    let waiting = match &agent.unnamed {
        Some(reason) => Some(Waiting {
            reason: Some(reason.clone()),
        }),
        None => agent.waiting.clone(),
    };
    Observed {
        reading: reading(agent.items.clone()),
        settled: agent.settled,
        waiting,
        failed: agent.failed,
        quota: agent.quota.clone(),
        switched: None,
        unnamed: agent.unnamed.is_some(),
    }
}

/// A look as the Node traces write the fake's answer.
fn observed_json(observed: &Observed) -> Value {
    let mut written = json!({
        "items": observed.items(),
        "settled": observed.settled,
        "waiting": observed.waiting.as_ref().map(|waiting| json!({ "reason": waiting.reason })),
        "quota": observed.quota.as_deref(),
        "failed": observed.failed,
    });
    if let Some(session) = &observed.switched {
        written["switched"] = json!({ "nativeSession": session });
    }
    if observed.unnamed {
        written["unnamed"] = json!(true);
    }
    written
}

/// The adapters the test's engine is made with: the fake under every
/// harness the test names, as `{ 'claude-code': adapter, opencode: adapter }`.
pub struct FakeAdapters {
    harnesses: Vec<String>,
    fake: Rc<FakeAdapter>,
}

impl FakeAdapters {
    pub fn new(fake: Rc<FakeAdapter>, harnesses: &[&str]) -> Self {
        Self {
            harnesses: harnesses
                .iter()
                .map(|harness| (*harness).to_owned())
                .collect(),
            fake,
        }
    }
}

impl Adapters for FakeAdapters {
    fn adapter(&self, harness: &str) -> Option<Rc<dyn Adapter>> {
        self.harnesses
            .iter()
            .any(|named| named == harness)
            .then(|| {
                Rc::new(Asked {
                    harness: harness.to_owned(),
                    fake: Rc::clone(&self.fake),
                }) as Rc<dyn Adapter>
            })
    }
}

/// The records as the fake's agents write them: the engine's look at a
/// conversation with no window (`adapter.record` in JavaScript).
pub struct FakeRecords {
    fake: Rc<FakeAdapter>,
}

impl FakeRecords {
    pub fn new(fake: Rc<FakeAdapter>) -> Self {
        Self { fake }
    }
}

impl Records for FakeRecords {
    fn look<'a>(
        &'a self,
        harness: Harness,
        session: &'a str,
        _options: &'a Options,
    ) -> Work<'a, Arc<Reading>> {
        let seam = format!("adapter:{}", harness.kind());
        let at = self.fake.recorder.call(
            &seam,
            Some("record"),
            json!([{ "conversation": { "nativeSession": session } }]),
        );
        let reading = self.fake.record_of(session);
        Box::pin(async move {
            next_turn().await;
            let written = match &reading {
                Reading::Unknown(_) => json!({ "unknown": true }),
                Reading::Known(record) => json!({ "items": record.items }),
            };
            self.fake.recorder.answered(at, written);
            Arc::new(reading)
        })
    }

    fn has_transcript<'a>(
        &'a self,
        _harness: Harness,
        session: &'a str,
    ) -> Work<'a, Result<bool, String>> {
        let known = !matches!(self.fake.record_of(session), Reading::Unknown(_));
        Box::pin(async move { Ok(known) })
    }
}

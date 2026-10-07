//! A harness adapter whose agents do exactly what the test tells them: the
//! twin of `fakeAdapter` in `core-dispatcher.test.mjs`. One adapter answers
//! for any harness the test names it under ([`super::FakeAdapters`]); each
//! window is an agent, found by its launch, and by the handle it is for.
//! What a call does and reads, it does when called, as the JavaScript fake's
//! async functions did, and is written down in the Node traces' shape, under
//! the harness it was asked as. The turn an `await` of the call costs is the
//! engine's, whether or not the call waited ([`crate::runtime::returning`]);
//! the fake takes only the turns of its own promises. A test that replaced one of the fake's functions, or set
//! what it reads, says what it did in its place ([`FakeAdapter::prepare`],
//! [`FakeAdapter::after_prepare`], [`FakeAdapter::started`],
//! [`FakeAdapter::deliver`], [`FakeAdapter::ready`],
//! [`FakeAdapter::interrupt`]); one that held calls until it let them go
//! (`hold`) says which, and when ([`FakeAdapter::prepare_holds`],
//! [`FakeAdapter::observe_holds`], [`FakeAdapter::start_holds`],
//! [`FakeAdapter::ready_holds`], [`FakeAdapter::deliver_holds`]).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use cf_harness::contract::{
    Adapter, Admission, Interrupt, Launch, Prepared, Readiness, Waiting, Window, Work,
};
use cf_harness::records::{Item, Quota, Role};
use serde_json::{json, Value};

use super::holds::{Holds, Wrapped};
use super::recorder::Recorder;
use super::window::FakeWindow;
use crate::runtime::next_turn;

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
    /// How often the engine told the window it pressed the interrupt keys.
    pub interrupted: u32,
    /// The window stops where it is when it is interrupted, writing nothing:
    /// a harness that wrote no record of a turn stopped before its first word.
    pub stops_when_interrupted: bool,
    /// What it stops by being interrupted, it also takes back: the message of
    /// its turn is out of the conversation it answers from (Claude's input box).
    pub takes_back: bool,
    /// The latest look found the window taken back so, until a message
    /// arrives that begins another turn.
    pub took_back: bool,
}

/// What a test makes `ready` answer, where it gives the adapter one.
pub type Ready = Rc<dyn Fn() -> Result<Readiness, String>>;

/// What a test makes `started` answer, where it gives the adapter one: why
/// the window could not take its first message, if it could not.
pub type Started = Rc<dyn Fn() -> Result<(), String>>;

/// What a test makes `prepare` do, where it gives the adapter one: told the
/// launch as asked, it may fail it, before any window is made.
pub type Prepare = Rc<dyn Fn(&Value) -> Result<(), String>>;

/// What a test's own `prepare` does with a window once the fake's has
/// prepared it, given the window's handle.
pub type AfterPrepare = Rc<dyn Fn(&str)>;

/// What the fake does with a delivery when it takes it: the agent refuses
/// it, or takes it and shows it in its record. A test that replaced
/// `deliver` runs it where it did (`deliver(request)` in the replacement).
pub type Taking = Box<dyn FnOnce() -> Admission>;

/// What a test's own `deliver` does with a delivery in place of the fake's.
pub type Deliver = Rc<dyn Fn(Taking) -> Work<'static, Result<Admission, String>>>;

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
    /// A `started` of the test's own, that fails some windows.
    pub started: RefCell<Option<Started>>,
    /// A `prepare` of the test's own, that fails some launches.
    pub prepare: RefCell<Option<Prepare>>,
    /// A `prepare` that does something with the window once the fake's own
    /// has prepared it (`await original(request)`), its answer a turn later
    /// still, as the replacement's promise settled after the original's.
    pub after_prepare: RefCell<Option<AfterPrepare>>,
    /// A `deliver` of the test's own, in place of the fake's.
    pub deliver: RefCell<Option<Deliver>>,
    /// The keys that interrupt a turn in its windows (`adapter.interrupt`):
    /// Escape once, as an adapter that says no more.
    pub interrupt: Cell<Interrupt>,
    /// Launches the test holds (`hold(adapter, 'prepare', ...)`): one let go
    /// fails with the reason, where the test gave one.
    pub prepare_holds: Holds<String>,
    /// Looks the test holds: one let go fails with the reason, where the test
    /// gave one.
    pub observe_holds: Holds<String>,
    /// Starts the test holds: one let go names the conversation its window
    /// opened, where the test gave one.
    pub start_holds: Holds<String>,
    /// Calls of `ready` the test holds: the test gives a `ready` of its own
    /// too, as the JavaScript fake had none to wrap.
    pub ready_holds: Holds,
    /// Deliveries the test holds.
    pub deliver_holds: Holds,
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
            prepare: RefCell::new(None),
            after_prepare: RefCell::new(None),
            deliver: RefCell::new(None),
            interrupt: Cell::new(Interrupt {
                presses: 1,
                close_after: None,
            }),
            prepare_holds: Holds::default(),
            observe_holds: Holds::default(),
            start_holds: Holds::default(),
            ready_holds: Holds::default(),
            deliver_holds: Holds::default(),
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

    /// Changes the window of `launch`, whichever participant it is for: the
    /// test kept the agent it got (`agent(handle)` of JavaScript was the
    /// object itself) while another project's window took the handle.
    pub fn with_launch<T>(&self, launch: &str, change: impl FnOnce(&mut FakeAgent) -> T) -> T {
        self.of_launch(launch, change)
    }

    /// The agent answers: its turn ends with `text`.
    pub fn answer(&self, handle: &str, text: &str) {
        let item = self.item(Role::Assistant, text);
        self.with(handle, |agent| {
            agent.items.push(item);
            agent.settled = true;
            agent.took_back = false;
        });
    }

    /// The agent is at work on a turn.
    pub fn busy(&self, handle: &str) {
        self.with(handle, |agent| {
            agent.settled = false;
            agent.took_back = false;
        });
    }

    /// The agent is at work on a turn it stops, at once and writing nothing,
    /// when the engine presses the interrupt keys into its window.
    pub fn busy_until_interrupted(&self, handle: &str) {
        self.with(handle, |agent| {
            agent.settled = false;
            agent.stops_when_interrupted = true;
        });
    }

    /// The same, and what it stops it takes back: Claude, stopped before a
    /// word of its answer, has the message of its turn in its input box again
    /// and out of its conversation.
    pub fn busy_until_taken_back(&self, handle: &str) {
        self.busy_until_interrupted(handle);
        self.with(handle, |agent| agent.takes_back = true);
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
    /// window's on it, none where no window had it.
    pub(super) fn items_of(&self, session: &str) -> Option<Vec<Item>> {
        let agents = self.agents.borrow();
        agents
            .iter()
            .rev()
            .find(|agent| agent.native == session)
            .map(|agent| agent.items.clone())
    }

    /// A delivery into `launch`'s window, as the fake takes it.
    pub(super) fn take(&self, launch: &str, text: &str) -> Admission {
        self.of_launch(launch, |agent| {
            if !agent.admit {
                return Admission::Refused {
                    reason: "refused by the test".to_owned(),
                };
            }
            if agent.arrive {
                agent.items.push(self.item(Role::User, text));
                agent.settled = false;
                // A message that arrives starts a turn: the one before it ended as it did.
                agent.failed = false;
                agent.took_back = false;
            }
            Admission::Admitted {
                queued: agent.queued,
            }
        })
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

    /// The launch as the fake prepares it, its call written down at `at`:
    /// what the fake's function does it does when it is made, as far as its
    /// first wait.
    fn made<'a>(
        &'a self,
        launch: &'a Launch<'a>,
        asked: Value,
        at: usize,
    ) -> Work<'a, Result<Prepared, String>> {
        let fake = &self.fake;
        let seam = format!("adapter:{}", self.harness);
        // A test's own `prepare` fails before the real one runs: nothing is
        // prepared and no window made, as where it threw in JavaScript.
        let own = fake.prepare.borrow().clone();
        let overridden = own.is_some();
        if let Some(Err(reason)) = own.map(|own| own(&asked)) {
            return Box::pin(async move {
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
            interrupted: 0,
            stops_when_interrupted: false,
            takes_back: false,
            took_back: false,
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
            // A test's own `prepare` is an async function that returns the
            // real one's promise: its answer takes two turns to settle.
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
            let after = fake.after_prepare.borrow().clone();
            if let Some(after) = after {
                after(&handle);
                next_turn().await;
            }
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

impl Adapter for Asked {
    fn prepare<'a>(&'a self, launch: &'a Launch<'a>) -> Work<'a, Result<Prepared, String>> {
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
        let args = json!([asked.clone()]);
        let at = self
            .fake
            .recorder
            .call(&seam, Some("prepare"), args.clone());
        let wrapped = self.fake.prepare_holds.wrap(&args);
        let Wrapped::Held { .. } = wrapped else {
            let made = self.made(launch, asked, at);
            return Box::pin(async move {
                let prepared = made.await;
                wrapped.after().await;
                prepared
            });
        };
        // A call held is made once the test lets it go, or, where the test
        // gave a reason, fails in its place, as the wrapper's throw did.
        Box::pin(async move {
            if let Some(reason) = wrapped.before().await {
                return Err(reason);
            }
            let prepared = self.made(launch, asked, at).await;
            wrapped.after().await;
            prepared
        })
    }

    fn interrupt(&self) -> Interrupt {
        self.fake.interrupt.get()
    }
}

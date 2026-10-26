//! A harness adapter whose agents do exactly what the test tells them: the twin
//! of `fakeAdapter` in Node's dispatcher suite. One adapter answers for any
//! harness the test names it under ([`super::FakeAdapters`]); each window is an
//! agent, found by its launch, and by the handle it is for. What a call does
//! and reads, it does when called, as the JavaScript fake's async functions
//! did, and is written down in the Node traces' shape, under the harness it was
//! asked as. The turn an `await` of the call costs is the engine's, whether or
//! not the call waited ([`crate::runtime::returning`]); the fake takes only the
//! turns of its own promises. A test that replaced one of the fake's functions,
//! or set what it reads, says what it did in its place
//! ([`FakeAdapter::prepare`], [`FakeAdapter::after_prepare`],
//! [`FakeAdapter::started`], [`FakeAdapter::deliver`], [`FakeAdapter::ready`],
//! [`FakeAdapter::interrupt`]); one that held calls until it let them go
//! (`hold`) says which, and when ([`FakeAdapter::prepare_holds`],
//! [`FakeAdapter::observe_holds`], [`FakeAdapter::start_holds`],
//! [`FakeAdapter::ready_holds`], [`FakeAdapter::deliver_holds`],
//! [`FakeAdapter::resumable_holds`], [`FakeAdapter::record_holds`],
//! [`FakeAdapter::cut_holds`]).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use cf_harness::contract::{
    Adapter, Admission, Input, Interrupt, Launch, Prepared, Readiness, Skill, Waiting, Window, Work,
};
use cf_harness::records::{Item, Monitor, Pending, Quota, Role};
use cf_proto::panes::Servers;
use cf_proto::specs::Variant;
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
    /// The settled turn left background agents or jobs running (the reading says so).
    pub pending: Pending,
    /// The monitors among those jobs, as the reading says them.
    pub monitors: Vec<Monitor>,
    pub waiting: Option<Waiting>,
    pub quota: Option<Arc<Quota>>,
    /// Whether a delivery is taken.
    pub admit: bool,
    /// Whether a delivery taken shows in the record.
    pub arrive: bool,
    /// Whether the harness's own queue takes a delivery.
    pub queued: bool,
    /// The harness says, after the hand-over, that it put the delivery to its
    /// agent (`Window::took`).
    pub took: bool,
    /// The harness holds the delivery in a turn it is in still (`Window::held`).
    pub held: bool,
    pub failed: bool,
    /// Why the turn failed, in the harness's words, where its record says.
    pub failure: Option<String>,
    /// The conversation the window shows, when the human switched it.
    pub shows: Option<String>,
    /// Each conversation's record, once the window switched away from it.
    pub records: HashMap<String, Vec<Item>>,
    /// Whether each of those records, read with no window open, reads at rest: as the
    /// window was when it switched away, unless the test says otherwise.
    pub records_settled: HashMap<String, bool>,
    /// What the harness's own store says each conversation's window took, read with no
    /// window open (`Records::took`, Devin's history rows): a conversation it holds no
    /// list of cannot tell, as every harness but Devin.
    pub stored: HashMap<String, Vec<String>>,
    /// Those of them that cannot be read: Devin's of a session its store holds no row of.
    pub records_unreadable: Vec<String>,
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
    /// Its harness says its window waits at its prompt, whatever its record says of the
    /// turn ([`Observed::at_prompt`]): set by [`FakeAdapter::answer_never_over`].
    pub at_prompt: bool,
    /// Its record says its turn is over though its status kept the look from
    /// reading it settled (`Observed::over`): Claude's busy while a background
    /// agent runs, after a turn an interrupt or a refusal ended.
    pub over: bool,
    /// Interrupted, its turn ends in its record and its status stays busy: the
    /// look reads it over, not settled (a Claude whose background agents run).
    pub busy_when_interrupted: bool,
    /// Its record, read with no window open, cannot tell whether its turn was
    /// cut: Devin's, which reads a turn as going on only while its window shows it.
    pub cannot_tell_a_cut: bool,
    /// What its harness says of going on with the conversation, as its own record
    /// tells it: it holds it (`Some(true)`, as every harness did before it was
    /// asked), it holds nothing (`Some(false)`: Codex's thread before its first
    /// turn), or the record cannot tell (none).
    pub resumable: Option<bool>,
    /// Its record cannot be read at a look (`Reading::Unknown`), as when the reader's
    /// read of the transcript fails.
    pub unread: bool,
}

/// What a test makes `ready` answer, where it gives the adapter one.
pub type Ready = Rc<dyn Fn() -> Result<Readiness, String>>;

/// What a test makes `started` answer, where it gives the adapter one: why
/// the window could not take its first message, if it could not.
pub type Started = Rc<dyn Fn() -> Result<(), String>>;

/// What a test makes `came_up` answer, where it gives the adapter one: the
/// conversation a window that came up after its `started` gave up says it is on
/// (`Some(None)` where it says none). Without one, none came up.
pub type CameUp = Rc<dyn Fn() -> Option<Option<String>>>;

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
    /// The time each of those launches was given for its first message, in
    /// milliseconds: kept apart from `prepared`, which is what Node's fake
    /// was asked and is held to Node's traces.
    first_message_ms: RefCell<Vec<i64>>,
    /// Whether each of those launches was told its window before was proven
    /// gone, kept apart for the same reason.
    previous_gone: RefCell<Vec<bool>>,
    /// How many windows in a row before each of those launches were given up for
    /// not coming up, kept apart for the same reason.
    starts_given_up: RefCell<Vec<u32>>,
    /// The test's items are numbered across its adapters.
    items: Rc<Cell<u64>>,
    /// A `ready` of the test's own; without one a window is ready, and
    /// nothing is asked (the JavaScript fake had none).
    pub ready: RefCell<Option<Ready>>,
    /// A `started` of the test's own, that fails some windows.
    pub started: RefCell<Option<Started>>,
    /// A `came_up` of the test's own, that says a window came up after all.
    pub came_up: RefCell<Option<CameUp>>,
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
    /// How its windows' harness keeps its servers (`adapter.servers`): not known,
    /// as an adapter that says no more.
    pub servers: Cell<Servers>,
    /// Whether its windows' harness keeps the instructions a resumed
    /// conversation began with (`adapter.keeps_first_instructions`, which no
    /// trace of Node's has): not, as an adapter that says no more.
    pub keeps_first_instructions: Cell<bool>,
    /// Whether every window of its harness is logged into the one account
    /// (`adapter.one_account`, which no trace of Node's has): not, as an
    /// adapter that says no more.
    pub one_account: Cell<bool>,
    /// Whether its windows are asked whether they are ready before they have
    /// drawn their screens too, as Devin's are: not, as an adapter that says no more.
    pub ready_while_drawing: Cell<bool>,
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
    /// Its windows take their first message once they are up, as OpenCode's do
    /// through its server, and only if the host admits it then: not with the
    /// launch, as a message on a command line goes in.
    pub seeds: Cell<bool>,
    /// Where its harness has the Specmint variant installed, when the test says
    /// it has: the `SKILL.md` it answers. Without, the copy ConsensFlow would
    /// keep for it, which is the same for every launch.
    pub specmint_installed: RefCell<Option<String>>,
    /// The variants it was asked where its harness has them, in order: a launch
    /// that is given no specs asks none.
    pub specmint_asked: RefCell<Vec<Variant>>,
    /// Asks of whether a conversation can be gone on with that the test holds,
    /// told the conversation as `[session]`: what the harness says is its own
    /// when the ask is let go. Node's dispatcher asked nothing of the kind.
    pub resumable_holds: Holds,
    /// Whether the host said, at each delivery in order, that the engine emptied
    /// the window's input box just before it (`PaneHost::cleared`). Not written
    /// down in the traces: Node's dispatcher emptied no box.
    pub cleared: RefCell<Vec<bool>>,
    /// Looks at a record with no window open that the test holds, told the
    /// conversation as `[session]`: one let go reads the record as it is then.
    /// Node's dispatcher suite held none.
    pub record_holds: Holds,
    /// Asks of whether a conversation's record ends in a turn that was cut, with no
    /// window open, that the test holds, told the conversation as `[session]`: one
    /// let go reads the record as it is then. Node's dispatcher asked nothing of the
    /// kind.
    pub cut_holds: Holds,
    /// Its windows' record holds a paste in the wrapper Claude Code 2.1.296 puts it
    /// in (`<pasted_content id="…">`), and a launch's first message as it was
    /// given: not, as the fake of Node's suite, which recorded both as given.
    pub wraps_pastes: Cell<bool>,
    /// Its windows' record holds nothing of the first message a launch gives them:
    /// each goes before its harness takes it, where the fake of Node's suite recorded
    /// it as it prepared the launch.
    pub forgets_first_messages: Cell<bool>,
    /// A window resumed on a conversation reads that conversation's record as the
    /// last window on it left it, as a harness's `--resume` does: not, as the fake of
    /// Node's suite, whose every window began with an empty record.
    pub resumes_records: Cell<bool>,
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
            first_message_ms: RefCell::new(Vec::new()),
            previous_gone: RefCell::new(Vec::new()),
            starts_given_up: RefCell::new(Vec::new()),
            items,
            ready: RefCell::new(None),
            started: RefCell::new(None),
            came_up: RefCell::new(None),
            prepare: RefCell::new(None),
            after_prepare: RefCell::new(None),
            deliver: RefCell::new(None),
            interrupt: Cell::new(Interrupt {
                presses: 1,
                close_after: None,
            }),
            servers: Cell::new(Servers::Unknown),
            keeps_first_instructions: Cell::new(false),
            one_account: Cell::new(false),
            ready_while_drawing: Cell::new(false),
            prepare_holds: Holds::default(),
            observe_holds: Holds::default(),
            start_holds: Holds::default(),
            ready_holds: Holds::default(),
            deliver_holds: Holds::default(),
            seeds: Cell::new(false),
            specmint_installed: RefCell::new(None),
            specmint_asked: RefCell::new(Vec::new()),
            resumable_holds: Holds::default(),
            cleared: RefCell::new(Vec::new()),
            record_holds: Holds::default(),
            cut_holds: Holds::default(),
            wraps_pastes: Cell::new(false),
            forgets_first_messages: Cell::new(false),
            resumes_records: Cell::new(false),
        })
    }

    /// The launches prepared, in order.
    pub fn prepared(&self) -> Vec<Value> {
        self.prepared.borrow().clone()
    }

    /// The time each launch prepared was given for its first message, in
    /// milliseconds, in the order they were prepared.
    pub fn first_message_waits(&self) -> Vec<i64> {
        self.first_message_ms.borrow().clone()
    }

    /// Whether each launch prepared was told the program of the window before
    /// it was proven gone, in the order they were prepared.
    pub fn previous_gone(&self) -> Vec<bool> {
        self.previous_gone.borrow().clone()
    }

    /// How many windows in a row before each launch prepared were given up for not
    /// coming up, in the order they were prepared.
    pub fn starts_given_up(&self) -> Vec<u32> {
        self.starts_given_up.borrow().clone()
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
            compaction: false,
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
            agent.over = false;
            agent.pending = Pending::default();
            agent.monitors.clear();
            agent.took_back = false;
            agent.at_prompt = false;
        });
    }

    /// The agent answers, and its window waits at its prompt, though its record never reads
    /// the turn as over (T-526: a conversation opened again with no proof that the program
    /// before was gone keeps what that program had queued).
    pub fn answer_never_over(&self, handle: &str, text: &str) {
        let item = self.item(Role::Assistant, text);
        self.with(handle, |agent| {
            agent.items.push(item);
            agent.settled = false;
            agent.over = false;
            agent.at_prompt = true;
            agent.pending = Pending::default();
            agent.monitors.clear();
        });
    }

    /// The agent answers, and its turn ends with `pending` still running: its
    /// harness lets it take input, and the end of each wakes a turn.
    pub fn answer_leaving(&self, handle: &str, text: &str, pending: Pending) {
        self.answer(handle, text);
        self.with(handle, |agent| agent.pending = pending);
    }

    /// The agent answers, and its turn ends with `pending` still running, `monitors` among
    /// it as the reading says them.
    pub fn answer_leaving_monitors(
        &self,
        handle: &str,
        text: &str,
        pending: Pending,
        monitors: Vec<Monitor>,
    ) {
        self.answer_leaving(handle, text, pending);
        self.with(handle, |agent| agent.monitors = monitors);
    }

    /// The agent answers, and its turn ends with a background agent still running.
    pub fn answer_leaving_agents(&self, handle: &str, text: &str) {
        self.answer_leaving(
            handle,
            text,
            Pending {
                agents: 1,
                ..Pending::default()
            },
        );
    }

    /// The agent answers, and its turn ends with a job it started in the background
    /// (a command, a monitor) still running.
    pub fn answer_leaving_a_job(&self, handle: &str, text: &str) {
        self.answer_leaving(
            handle,
            text,
            Pending {
                jobs: 1,
                ..Pending::default()
            },
        );
    }

    /// The harness wakes the agent for the end of a job or an agent it left running: a
    /// turn of its own begins with the harness's notification, `said`, as the user's item
    /// Claude Code writes it as.
    pub fn wake(&self, handle: &str, said: &str) {
        let item = self.item(Role::User, said);
        self.with(handle, |agent| {
            agent.items.push(item);
            agent.settled = false;
            agent.over = false;
            agent.pending = Pending::default();
            agent.monitors.clear();
            agent.took_back = false;
            agent.at_prompt = false;
        });
    }

    /// The agent is at work on a turn.
    pub fn busy(&self, handle: &str) {
        self.with(handle, |agent| {
            agent.settled = false;
            agent.over = false;
            agent.pending = Pending::default();
            agent.monitors.clear();
            agent.took_back = false;
            agent.at_prompt = false;
        });
    }

    /// The agent's record cannot be read from now on (`unread`), or can again: its
    /// window is as it was.
    pub fn unread(&self, handle: &str, unread: bool) {
        self.with(handle, |agent| agent.unread = unread);
    }

    /// The agent is at work on a turn it stops, at once and writing nothing,
    /// when the engine presses the interrupt keys into its window.
    pub fn busy_until_interrupted(&self, handle: &str) {
        self.with(handle, |agent| {
            agent.settled = false;
            agent.stops_when_interrupted = true;
        });
    }

    /// The agent is at work on a turn that an interrupt ends in its record while
    /// its status stays busy: Claude's, with background agents running, whose
    /// interrupted turn writes no count of them (T-604). The look reads it over,
    /// never settled.
    pub fn busy_with_agents_until_interrupted(&self, handle: &str) {
        self.busy(handle);
        self.with(handle, |agent| agent.busy_when_interrupted = true);
    }

    /// The harness refuses the turn as `refuse` says, and the look still does not
    /// read the window settled: its record reads the turn in flight past the
    /// refusal, as Claude's reader read T-502's (a queued message of a process the
    /// window's reopen was not told had gone), or its status says busy.
    pub fn refuse_unsettled(&self, handle: &str, quota: Quota) {
        self.refuse(handle, quota);
        self.with(handle, |agent| agent.settled = false);
    }

    /// Its record cannot tell, with no window open, whether its turn was cut,
    /// as Devin's cannot.
    pub fn cannot_tell_a_cut(&self, handle: &str) {
        self.with(handle, |agent| agent.cannot_tell_a_cut = true);
    }

    /// Its harness holds nothing of the window's conversation, as Codex holds
    /// nothing of a thread before its first turn.
    pub fn holds_nothing(&self, handle: &str) {
        self.with(handle, |agent| agent.resumable = Some(false));
    }

    /// Its record cannot tell whether its harness holds the window's
    /// conversation: the engine goes on with it, as it did before it asked.
    pub fn cannot_tell_a_resume(&self, handle: &str) {
        self.with(handle, |agent| agent.resumable = None);
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
            agent.over = false;
            agent.at_prompt = false;
            agent.pending = Pending::default();
            agent.monitors.clear();
        });
    }

    /// The window begins a turn of its own, with words nobody pasted (Claude
    /// Code's meta prompt once its limit resets, or what the human typed into
    /// it), dated `at`: in it still.
    pub fn begins_its_own_turn(&self, handle: &str, prompt: &str, at: &str) {
        let mut said = self.item(Role::User, prompt);
        said.at = Some(json!(at));
        self.with(handle, |agent| {
            agent.items.push(said);
            agent.settled = false;
            agent.over = false;
            agent.pending = Pending::default();
            agent.failed = false;
            agent.took_back = false;
            agent.at_prompt = false;
        });
    }

    /// The window's own turn gets through and ends with `text`, dated `at`: as a
    /// harness reads a turn that got through, the refusal before it is no longer
    /// its last word.
    pub fn ends_its_own_turn(&self, handle: &str, text: &str, at: &str) {
        let mut item = self.item(Role::Assistant, text);
        item.at = Some(json!(at));
        self.with(handle, |agent| {
            agent.items.push(item);
            agent.settled = true;
            agent.failed = false;
            agent.quota = None;
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
            agent.records_settled.insert(shown.clone(), agent.settled);
            agent.records.insert(shown, items);
            agent.items = agent.records.get(native).cloned().unwrap_or_default();
            agent.shows = Some(native.to_owned());
            agent.settled = true;
            agent.pending = Pending::default();
            agent.monitors.clear();
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

    /// Whether the turn `session`'s record ends in was cut, as a harness reads it
    /// with no window open: none where no window had it, or its record cannot tell.
    pub(super) fn cut_of(&self, session: &str) -> Option<bool> {
        let agents = self.agents.borrow();
        agents
            .iter()
            .rev()
            .find(|agent| agent.native == session)
            .filter(|agent| !agent.cannot_tell_a_cut)
            .map(|agent| !agent.settled)
    }

    /// Whether the store of `session` says its window took a paste that begins with
    /// `pasted` (`Records::took`): none where the test gave it no store.
    pub(super) fn took_of(&self, session: &str, pasted: &str) -> Option<bool> {
        let agents = self.agents.borrow();
        agents
            .iter()
            .rev()
            .find_map(|agent| agent.stored.get(session))
            .map(|rows| rows.iter().any(|row| row.starts_with(pasted)))
    }

    /// What the harness says of going on with `session`: what the latest window
    /// that opened on it, or shows it now, is set to say, and that it holds it where
    /// no window had it, as every harness did before it was asked.
    pub(super) fn resumable_of(&self, session: &str) -> Option<bool> {
        let agents = self.agents.borrow();
        agents
            .iter()
            .rev()
            .find(|agent| agent.native == session || agent.shows.as_deref() == Some(session))
            .map_or(Some(true), |agent| agent.resumable)
    }

    /// What the record of `session` holds (`adapter.record`): the latest
    /// window's on it, none where no window had it, and whether the turn it
    /// ends in is over (an agent that was never answered is in one still, as a
    /// window the app's quit cut).
    pub(super) fn record_of(&self, session: &str) -> Option<(Vec<Item>, bool)> {
        let agents = self.agents.borrow();
        agents
            .iter()
            .rev()
            .find(|agent| agent.native == session)
            .filter(|agent| !agent.records_unreadable.iter().any(|left| left == session))
            .map(
                |agent| match agent.shows.as_ref().filter(|shows| *shows != session) {
                    // A conversation the window switched away from is its record as it was left.
                    Some(_) => (
                        agent.records.get(session).cloned().unwrap_or_default(),
                        agent.records_settled.get(session).copied().unwrap_or(true),
                    ),
                    None => (agent.items.clone(), agent.settled),
                },
            )
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
                let recorded = if self.wraps_pastes.get() {
                    format!(
                        "\n\n<pasted_content id=\"b118\">\n{text}\n</pasted_content id=\"b118\">\n"
                    )
                } else {
                    text.to_owned()
                };
                agent.items.push(self.item(Role::User, &recorded));
                agent.settled = false;
                agent.pending = Pending::default();
                agent.monitors.clear();
                // A message that arrives starts a turn: the one before it ended as it did.
                agent.failed = false;
                agent.failure = None;
                agent.took_back = false;
                agent.at_prompt = false;
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
        fake.first_message_ms
            .borrow_mut()
            .push(launch.first_message_ms);
        fake.previous_gone.borrow_mut().push(launch.previous_gone);
        fake.starts_given_up
            .borrow_mut()
            .push(launch.starts_given_up);
        let id = launch.id.as_str().to_owned();
        let native = launch
            .resume
            .map_or_else(|| format!("native-{id}"), str::to_owned);
        let resumed = launch
            .resume
            .filter(|_| fake.resumes_records.get())
            .and_then(|session| fake.record_of(session))
            .map(|(items, _)| items);
        let mut agent = FakeAgent {
            launch: id.clone(),
            handle: launch.handle.to_owned(),
            native: native.clone(),
            items: resumed.unwrap_or_default(),
            settled: true,
            pending: Pending::default(),
            monitors: Vec::new(),
            waiting: None,
            quota: None,
            admit: true,
            arrive: true,
            queued: false,
            took: false,
            held: false,
            failed: false,
            failure: None,
            shows: None,
            records: HashMap::new(),
            records_settled: HashMap::new(),
            stored: HashMap::new(),
            records_unreadable: Vec::new(),
            unnamed: None,
            interrupted: 0,
            stops_when_interrupted: false,
            takes_back: false,
            took_back: false,
            at_prompt: false,
            over: false,
            busy_when_interrupted: false,
            cannot_tell_a_cut: false,
            resumable: Some(true),
            unread: false,
        };
        let seeded = launch
            .message
            .filter(|_| fake.seeds.get())
            .map(str::to_owned);
        if let Some(message) = launch
            .message
            .filter(|_| seeded.is_none() && !fake.forgets_first_messages.get())
        {
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
            seeded,
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
        let mut asked = json!({
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
        // Said only where specs were given, which no trace of Node's has.
        if let Some(specs) = launch.specs {
            asked["specs"] = json!({
                "variant": specs.variant.as_str(),
                "skill": specs.skill.path(),
                "installed": matches!(specs.skill, Skill::Global(_)),
            });
        }
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

    /// Asked where Node's dispatcher asked nothing, so it is no call of the adapter
    /// in a trace.
    fn resumable<'a>(&'a self, session: &'a str) -> Work<'a, Option<bool>> {
        let wrapped = self.fake.resumable_holds.wrap(&json!([session]));
        Box::pin(async move {
            wrapped.before().await;
            let answer = self.fake.resumable_of(session);
            wrapped.after().await;
            answer
        })
    }

    fn interrupt(&self) -> Interrupt {
        self.fake.interrupt.get()
    }

    fn servers(&self) -> Servers {
        self.fake.servers.get()
    }

    /// Not a call of Node's adapter: nothing is written down of it in the
    /// traces.
    fn keeps_first_instructions(&self) -> bool {
        self.fake.keeps_first_instructions.get()
    }

    /// Not a call of Node's adapter: nothing is written down of it in the
    /// traces.
    fn one_account(&self) -> bool {
        self.fake.one_account.get()
    }

    /// Not a call of Node's adapter: nothing is written down of it in the
    /// traces. The fake posts its first message once up where the test says it
    /// seeds, as OpenCode does.
    fn first_message_once_up(&self) -> bool {
        self.fake.seeds.get()
    }

    /// Not a call of Node's adapter: nothing is written down of it in the
    /// traces. As the harness it answers for takes it: Claude Code and Devin
    /// pasted, their boxes emptied with Control-C, the others over a channel.
    fn input(&self) -> Input {
        match self.harness.as_str() {
            "claude-code" | "devin" => Input::Pasted { clear: &[0x03] },
            _ => Input::Channel,
        }
    }

    /// Not a call of Node's adapter: nothing is written down of it in the
    /// traces. What the fake says is where the test says its harness has the
    /// variant installed, else the copy ConsensFlow would keep for it: at a
    /// place that no launch is in, as the adapters' is.
    fn specmint(&self, variant: Variant) -> Result<Skill, String> {
        self.fake.specmint_asked.borrow_mut().push(variant);
        Ok(match self.fake.specmint_installed.borrow().clone() {
            Some(file) => Skill::Global(file),
            None => Skill::Own(format!(
                "/consensflow/integrations/specmint/fake/{}/.claude/skills/{}/SKILL.md",
                variant.as_str(),
                variant.skill_name()
            )),
        })
    }
}

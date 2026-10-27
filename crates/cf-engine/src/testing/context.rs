//! A test's engine, as the setup of Node's dispatcher suite made it: a real
//! ledger on a temporary file, at a clock that moves when the test says (from
//! 2026-09-19 12:00 UTC) and naming sessions from a fixed list; the fake pane
//! host and adapter; the other seams' fakes; and the engine made with them, its
//! work run to stillness by the engine's own executor, which the kit drains.
//! What the engine asks of its seams is written down ([`Recorder`]) for the
//! Node trace of the same test to be held against.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::path::PathBuf;
use std::rc::Rc;

use cf_base::time::Clock;
use cf_harness::seams::Time;
use cf_harness::testing::ManualTime;
use cf_ledger::{
    open_ledger, Ledger, NewProject, NewTask, Options, ProjectView, TaskCreated, TaskThread,
};
use cf_proto::specs::AppSpecs;
use serde_json::{json, Value};

use super::adapter::FakeAdapter;
use super::adapters::{FakeAdapters, FakeRecords};
use super::background::FakeBackground;
use super::executor::Answer;
use super::host::FakeHost;
use super::operations::{answer, nothing};
use super::recorder::Recorder;
use super::seams::{
    CountingLaunchIds, FakeComputerUse, FakeCredentials, FakeLaunchFiles, FakeLog, FakePaneEnv,
    FakeRoles, FakeRoster, FakeTrace,
};
use super::time::TestTime;
use crate::dispatcher::Dispatcher;
use crate::runtime::Executor;
use crate::seams::{Adapters, EngineError, Limits, Seams};

/// 2026-09-19T12:00:00.000Z, where every test's clock starts.
pub const START_MS: i64 = 1_789_819_200_000;

/// The names the ledger gives sessions, in turn.
const NAMES: [&str; 6] = [
    "amber-pine",
    "brisk-birch",
    "calm-brook",
    "coral-canyon",
    "crisp-cedar",
    "dusky-cliff",
];

/// The ledger's clock: the test's.
struct TestClock(Rc<ManualTime>);

impl Clock for TestClock {
    fn now_ms(&mut self) -> i64 {
        self.0.wall_ms()
    }
}

/// What a test's ledger is opened with: the test's clock, the names in turn, and each
/// event it logs written down.
fn ledger_options(time: &Rc<ManualTime>, recorder: &Recorder) -> Options {
    let named = Cell::new(0);
    let told = recorder.clone();
    Options {
        clock: Box::new(TestClock(Rc::clone(time))),
        names: Box::new(move || {
            let name = NAMES[named.get() % NAMES.len()];
            named.set(named.get() + 1);
            name.to_owned()
        }),
        trace: Box::new(move |event| {
            told.event(json!({
                "at": event.at,
                "project": event.project,
                "kind": event.kind,
                "data": event.data,
            }));
        }),
    }
}

/// What a test's engine is made with besides the defaults.
pub struct Made {
    /// The harnesses the fake adapter answers for.
    pub harnesses: Vec<&'static str>,
    /// Codex has a fake of its own ([`Context::codex`]), so a test sees which
    /// harness a window opened on (`withCodex`).
    pub codex: bool,
    /// The settle of the pacing ([`Limits::settle_ms`]): none, unless the case is
    /// the pacing's.
    pub settle_ms: i64,
}

impl Default for Made {
    fn default() -> Self {
        Self {
            // The fake answers for any harness; OpenCode is here for a mixed staff.
            harnesses: vec!["claude-code", "opencode"],
            codex: false,
            settle_ms: 0,
        }
    }
}

/// A test's engine and everything it is made with.
pub struct Context {
    pub executor: Rc<Executor>,
    pub recorder: Recorder,
    pub ledger: Rc<RefCell<Ledger>>,
    /// The clock a test moves ([`Context::advance`]).
    pub time: Rc<ManualTime>,
    /// The timers an engine's sleeps wait on, which the clock never sees.
    pub timers: Rc<TestTime>,
    pub host: Rc<FakeHost>,
    /// What the host says of the work a window's turn left running.
    pub background: Rc<FakeBackground>,
    pub adapter: Rc<FakeAdapter>,
    /// The fake of Codex's own, which answers for it when the test made it so.
    pub codex: Rc<FakeAdapter>,
    pub roster: Rc<FakeRoster>,
    /// The role texts the engine is given, which a test may say were rewritten.
    pub roles: Rc<FakeRoles>,
    pub trace: Rc<FakeTrace>,
    pub log: Rc<FakeLog>,
    pub launch_files: Rc<FakeLaunchFiles>,
    /// What the machine says of computer use: none of it, unless a test says so.
    pub computer_use: Rc<FakeComputerUse>,
    pub dispatcher: Rc<Dispatcher>,
    /// What the engine is made with, for the engines a restart makes.
    pub(super) seams: Seams,
    /// The engines a restart made ([`Context::make`]), dropped with the test.
    pub(super) restarted: RefCell<Vec<Rc<Dispatcher>>>,
    file: PathBuf,
    // Last, so the ledger's file goes after the ledger.
    _dir: tempfile::TempDir,
}

impl Context {
    pub fn new() -> Self {
        Self::made(Made::default())
    }

    pub fn made(made: Made) -> Self {
        let dir = tempfile::Builder::new()
            .prefix("cf-dispatch-")
            .tempdir()
            .expect("a temporary folder");
        let file = dir.path().join("consensflow.db");
        let recorder = Recorder::default();
        let time = Rc::new(ManualTime::new(START_MS));
        let timers = Rc::new(TestTime::new(Rc::clone(&time), recorder.clone()));
        let ledger = open_ledger(&file, ledger_options(&time, &recorder)).expect("a ledger");
        let ledger = Rc::new(RefCell::new(ledger));
        let executor = Rc::new(Executor::strict());
        let host = FakeHost::new(recorder.clone());
        let background = FakeBackground::new();
        let adapter = FakeAdapter::new(recorder.clone());
        let codex = adapter.another();
        let mut table = FakeAdapters::new(&adapter, &made.harnesses);
        if made.codex {
            table = table.with("codex", &codex);
        }
        let adapters = Rc::new(table);
        let roster = Rc::new(FakeRoster {
            recorder: recorder.clone(),
            gone: RefCell::new(HashSet::new()),
            retired: RefCell::new(HashMap::new()),
            designers: RefCell::new(HashSet::new()),
            broken: Cell::new(false),
            model: RefCell::new(None),
            app_specs: RefCell::new(AppSpecs::default()),
            app_specs_asked: Cell::new(0),
        });
        let roles = Rc::new(FakeRoles {
            recorder: recorder.clone(),
            rewritten: RefCell::new(String::new()),
        });
        let trace = Rc::new(FakeTrace {
            recorder: recorder.clone(),
            lines: RefCell::new(Vec::new()),
            forgets: Cell::new(false),
            forgotten: RefCell::new(Vec::new()),
        });
        let log = Rc::new(FakeLog {
            recorder: recorder.clone(),
            failures: RefCell::new(Vec::new()),
            warnings: RefCell::new(Vec::new()),
        });
        let launch_files = Rc::new(FakeLaunchFiles {
            recorder: recorder.clone(),
            forgotten: RefCell::new(Vec::new()),
        });
        let computer_use = Rc::new(FakeComputerUse::default());
        let seams = Seams {
            ledger: Rc::clone(&ledger),
            host: Rc::clone(&host) as Rc<_>,
            background: Rc::clone(&background) as Rc<_>,
            adapters: Rc::clone(&adapters) as Rc<dyn Adapters>,
            records: Rc::new(FakeRecords::new(adapters)),
            time: Rc::clone(&timers) as Rc<dyn Time>,
            launch_ids: Rc::new(CountingLaunchIds::default()),
            credentials: Rc::new(FakeCredentials {
                recorder: recorder.clone(),
                ledger: Rc::clone(&ledger),
            }),
            pane_env: Rc::new(FakePaneEnv {
                recorder: recorder.clone(),
            }),
            roster: Rc::clone(&roster) as Rc<_>,
            roles: Rc::clone(&roles) as Rc<_>,
            trace: Rc::clone(&trace) as Rc<_>,
            log: Rc::clone(&log) as Rc<_>,
            launch_files: Rc::clone(&launch_files) as Rc<_>,
            spawn: Rc::clone(&executor) as Rc<_>,
            computer_use: Rc::clone(&computer_use) as Rc<_>,
            limits: Limits {
                arrival_ms: 30_000,
                launch_ms: 120_000,
                open_ms: 60_000,
                max_attempts: 3,
                // No pacing, so every case keeps the timing it was written with; a
                // case of the pacing's own asks for a settle (`Made::settle_ms`).
                settle_ms: made.settle_ms,
            },
        };
        let dispatcher = Dispatcher::new(seams.clone());
        host.attach(&dispatcher);
        Self {
            executor,
            recorder,
            ledger,
            time,
            timers,
            host,
            background,
            adapter,
            codex,
            roster,
            roles,
            trace,
            log,
            launch_files,
            computer_use,
            dispatcher,
            seams,
            restarted: RefCell::new(Vec::new()),
            file,
            _dir: dir,
        }
    }

    /// Runs `work` and all it begins to stillness: its answer.
    pub fn run<T: 'static>(&self, work: impl Future<Output = T> + 'static) -> T {
        self.finish(self.executor.start(work))
    }

    /// Runs what was started, and all it begins, until `answer` is there. A
    /// sleep the work waits on ends once nothing else can move, as a timer
    /// does in JavaScript's loop, the earliest first.
    pub fn finish<T>(&self, answer: Answer<T>) -> T {
        loop {
            self.executor.drain();
            if answer.ended() || !self.timers.fire_next() {
                break;
            }
        }
        answer
            .take()
            .expect("the work waits on nothing the test releases")
    }

    /// Runs everything started to stillness, its sleeps not ended: what
    /// waits on one keeps waiting, as before a timer is due.
    pub fn settle(&self) {
        self.executor.drain();
    }

    /// One pass, everything it began run to stillness.
    pub fn pass(&self) -> Result<(), EngineError> {
        let dispatcher = Rc::clone(&self.dispatcher);
        let pass = self.operation("pass", json!([]), nothing, async move {
            dispatcher.pass().await
        });
        self.run(pass)
    }

    /// A project opened (`dispatcher.openProject`), `request` as the API
    /// gives it, run to stillness.
    pub fn open_project(&self, request: Value) -> Result<Option<ProjectView>, EngineError> {
        let dispatcher = Rc::clone(&self.dispatcher);
        let asked = request.clone();
        let opening = self.operation("openProject", json!([request]), answer, async move {
            let request = NewProject::from_json(&asked)?;
            dispatcher.open_project(request).await
        });
        self.run(opening)
    }

    /// A project with its chief window up, `workers` (standard workers on
    /// Claude Code) its staff from the start (`withStaff`).
    pub fn with_staff(&self, workers: &[&str]) -> ProjectView {
        let staff: Vec<Value> = workers
            .iter()
            .map(|agent| member(agent, "worker", "standard"))
            .collect();
        self.open_project(app(staff))
            .expect("a project with its staff")
            .expect("the project, not deleted")
    }

    /// A tiered staff (`withTiers`): standard workers (`workers`), a light
    /// worker, and two standard reviewers.
    pub fn with_tiers(&self, workers: &[&str]) -> ProjectView {
        let mut staff: Vec<Value> = workers
            .iter()
            .map(|agent| member(agent, "worker", "standard"))
            .collect();
        staff.extend([
            member("hera", "worker", "light"),
            member("calliope", "reviewer", "standard"),
            member("astraeus", "reviewer", "standard"),
        ]);
        self.open_project(app(staff))
            .expect("a project with its tiers")
            .expect("the project, not deleted")
    }

    /// The project as the ledger has it now.
    pub fn project(&self, project: i64) -> ProjectView {
        self.ledger
            .borrow()
            .project(project)
            .expect("the ledger read")
            .unwrap_or_else(|| panic!("no project {project}"))
    }

    /// The id of `handle` in `project`, as the ledger has it now.
    pub fn id(&self, project: i64, handle: &str) -> i64 {
        self.project(project)
            .participants
            .into_iter()
            .find(|participant| participant.handle == handle)
            .map(|participant| participant.id)
            .unwrap_or_else(|| panic!("no @{handle} in project {project}"))
    }

    /// A task from the chief to `to` by name (`ledger.createTask`).
    pub fn give(&self, project: i64, to: &str, body: &str) -> TaskCreated {
        self.create_task(
            project,
            NewTask {
                from: "chief".to_owned(),
                to: Some(to.to_owned()),
                body: body.to_owned(),
                ..NewTask::default()
            },
        )
    }

    /// A task as the ledger is asked for it (`ledger.createTask`).
    pub fn create_task(&self, project: i64, task: NewTask) -> TaskCreated {
        self.ledger
            .borrow_mut()
            .create_task(project, &task)
            .expect("a task")
    }

    /// Task `number` of `project`, with its thread.
    pub fn task(&self, project: i64, number: i64) -> TaskThread {
        self.ledger
            .borrow()
            .task(project, number)
            .expect("the ledger read")
            .expect("the task")
    }

    /// Moves the clock on by `ms`.
    pub fn advance(&self, ms: i64) {
        self.time.settle_at(self.time.wall_ms() + ms);
    }

    /// The ledger's file, changed by `change` while no ledger holds it, as an earlier
    /// build or a start that stopped partway left it, and opened again as a start
    /// opens it.
    pub fn edit_ledger(&self, change: impl FnOnce(&std::path::Path)) {
        let mut ledger = self.ledger.borrow_mut();
        ledger.close_in_place().expect("the file is given up");
        change(&self.file);
        *ledger = open_ledger(&self.file, ledger_options(&self.time, &self.recorder))
            .expect("the ledger opens again");
    }

    /// The test is over: nothing may have failed unseen, and the engine and
    /// the ledger are closed, its file left to read.
    pub fn close(self) -> Closed {
        let failures = self.log.failures.borrow().clone();
        assert!(failures.is_empty(), "nothing failed unseen: {failures:?}");
        let events = self.recorder.events();
        let Self {
            executor,
            ledger,
            dispatcher,
            restarted,
            host,
            adapter,
            seams,
            file,
            _dir: dir,
            ..
        } = self;
        drop((executor, dispatcher, restarted, host, adapter, seams));
        let ledger = Rc::try_unwrap(ledger)
            .unwrap_or_else(|_| panic!("something of the test still holds the ledger"));
        drop(ledger.into_inner());
        Closed { events, file, dir }
    }
}

/// The project `withStaff` and `withTiers` open: `/work/app`, the chief on
/// apollo, `staff` from the start.
fn app(staff: Vec<Value>) -> Value {
    json!({
        "directory": "/work/app",
        "name": "app",
        "chief": { "harness": "claude-code", "agent": "apollo" },
        "staff": staff,
    })
}

/// A member of a project's first staff, on Claude Code.
fn member(agent: &str, role: &str, tier: &str) -> Value {
    json!({ "agent": agent, "harness": "claude-code", "role": role, "tier": tier })
}

/// A test's engine, closed: what it asked of its seams, and its ledger's
/// file, there until this is dropped.
pub struct Closed {
    pub events: Vec<serde_json::Value>,
    pub file: PathBuf,
    pub dir: tempfile::TempDir,
}

impl Default for Context {
    fn default() -> Self {
        Self::new()
    }
}

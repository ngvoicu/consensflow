//! A test's engine, as `setup` in `core-dispatcher.test.mjs` makes it: a
//! real ledger on a temporary file, at a clock that moves when the test
//! says (from 2026-09-19 12:00 UTC) and naming sessions from a fixed list;
//! the fake pane host and adapter; the other seams' fakes; and the engine
//! made with them, its work run to stillness by the kit's executor. What
//! the engine asks of its seams is written down ([`Recorder`]) for the Node
//! trace of the same test to be held against.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::future::Future;
use std::path::PathBuf;
use std::rc::Rc;

use cf_base::time::Clock;
use cf_harness::seams::Time;
use cf_harness::testing::ManualTime;
use cf_ledger::{open_ledger, Ledger, Options, ProjectView};
use serde_json::json;

use super::adapter::{FakeAdapter, FakeAdapters, FakeRecords};
use super::executor::Executor;
use super::host::FakeHost;
use super::recorder::Recorder;
use super::seams::{
    CountingLaunchIds, FakeCredentials, FakeLaunchFiles, FakeLog, FakePaneEnv, FakeRoles,
    FakeRoster, FakeTrace,
};
use crate::chief_switch::SwitchTo;
use crate::dispatcher::{Dispatcher, SwitchWhen};
use crate::seams::{EngineError, Limits, Seams};

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

/// What a test's engine is made with besides the defaults.
pub struct Made {
    /// The harnesses the fake adapter answers for.
    pub harnesses: Vec<&'static str>,
}

impl Default for Made {
    fn default() -> Self {
        Self {
            // The fake answers for any harness; OpenCode is here for a mixed staff.
            harnesses: vec!["claude-code", "opencode"],
        }
    }
}

/// A test's engine and everything it is made with.
pub struct Context {
    pub executor: Rc<Executor>,
    pub recorder: Recorder,
    pub ledger: Rc<RefCell<Ledger>>,
    pub time: Rc<ManualTime>,
    pub host: Rc<FakeHost>,
    pub adapter: Rc<FakeAdapter>,
    pub roster: Rc<FakeRoster>,
    pub trace: Rc<FakeTrace>,
    pub log: Rc<FakeLog>,
    pub launch_files: Rc<FakeLaunchFiles>,
    pub dispatcher: Rc<Dispatcher>,
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
        let named = Cell::new(0);
        let told = recorder.clone();
        let ledger = open_ledger(
            &file,
            Options {
                clock: Box::new(TestClock(Rc::clone(&time))),
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
            },
        )
        .expect("a ledger");
        let ledger = Rc::new(RefCell::new(ledger));
        let executor = Rc::new(Executor::default());
        let host = FakeHost::new(recorder.clone());
        let adapter = FakeAdapter::new(recorder.clone());
        let roster = Rc::new(FakeRoster {
            recorder: recorder.clone(),
            gone: RefCell::new(HashSet::new()),
            designers: RefCell::new(HashSet::new()),
            broken: Cell::new(false),
        });
        let trace = Rc::new(FakeTrace {
            recorder: recorder.clone(),
            lines: RefCell::new(Vec::new()),
        });
        let log = Rc::new(FakeLog {
            recorder: recorder.clone(),
            failures: RefCell::new(Vec::new()),
        });
        let launch_files = Rc::new(FakeLaunchFiles {
            recorder: recorder.clone(),
            forgotten: RefCell::new(Vec::new()),
        });
        let dispatcher = Dispatcher::new(Seams {
            ledger: Rc::clone(&ledger),
            host: Rc::clone(&host) as Rc<_>,
            adapters: Rc::new(FakeAdapters::new(Rc::clone(&adapter), &made.harnesses)),
            records: Rc::new(FakeRecords::new(Rc::clone(&adapter))),
            time: Rc::clone(&time) as Rc<dyn Time>,
            launch_ids: Rc::new(CountingLaunchIds::default()),
            credentials: Rc::new(FakeCredentials {
                recorder: recorder.clone(),
                ledger: Rc::clone(&ledger),
            }),
            pane_env: Rc::new(FakePaneEnv {
                recorder: recorder.clone(),
            }),
            roster: Rc::clone(&roster) as Rc<_>,
            roles: Rc::new(FakeRoles {
                recorder: recorder.clone(),
            }),
            trace: Rc::clone(&trace) as Rc<_>,
            log: Rc::clone(&log) as Rc<_>,
            launch_files: Rc::clone(&launch_files) as Rc<_>,
            spawn: Rc::clone(&executor) as Rc<_>,
            limits: Limits {
                arrival_ms: 30_000,
                launch_ms: 120_000,
                max_attempts: 3,
            },
        });
        host.attach(&dispatcher);
        Self {
            executor,
            recorder,
            ledger,
            time,
            host,
            adapter,
            roster,
            trace,
            log,
            launch_files,
            dispatcher,
            file,
            _dir: dir,
        }
    }

    /// Runs `work` and all it begins to stillness: its answer.
    pub fn run<T: 'static>(&self, work: impl Future<Output = T> + 'static) -> T {
        self.executor
            .finish(work)
            .expect("the work waits on nothing the test releases")
    }

    /// One pass, everything it began run to stillness.
    pub fn pass(&self) -> Result<(), EngineError> {
        self.recorder.op("pass", json!([]));
        let dispatcher = Rc::clone(&self.dispatcher);
        self.run(async move { dispatcher.pass().await })
    }

    /// Moves the clock on by `ms`.
    pub fn advance(&self, ms: i64) {
        self.time.settle_at(self.time.wall_ms() + ms);
    }

    /// Switch chief (`dispatcher.switchChief`), run to stillness.
    pub fn switch_chief(
        &self,
        project: i64,
        to: SwitchTo,
        when: SwitchWhen,
        note: bool,
    ) -> Result<ProjectView, EngineError> {
        self.recorder.op(
            "switchChief",
            json!([project, { "harness": to.harness, "agent": to.agent }]),
        );
        let dispatcher = Rc::clone(&self.dispatcher);
        self.run(async move { dispatcher.switch_chief(project, to, when, note).await })
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
            host,
            adapter,
            file,
            _dir: dir,
            ..
        } = self;
        drop((executor, dispatcher, host, adapter));
        let ledger = Rc::try_unwrap(ledger)
            .unwrap_or_else(|_| panic!("something of the test still holds the ledger"));
        drop(ledger.into_inner());
        Closed { events, file, dir }
    }
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

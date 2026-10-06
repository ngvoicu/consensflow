//! The daemon's real pieces around a real engine: the executor and its
//! driver, the bridge's reader with its checkpoint, the pane host over it
//! (the test, as bytes: [`super::host`]), the dispatcher on a real ledger, the
//! daemon's own trace, credentials and role texts. What stands in for the
//! harnesses is the engine kit's fake adapter, whose agents do what the test
//! tells them.
//!
//! [`Pieces`] is the part without an engine, for tests of what the executor
//! is given from outside; [`Rig`] adds the engine.

use std::cell::RefCell;
use std::future::Future;
use std::rc::Rc;

use cf_base::env::Env;
use cf_base::refusal::Refusal;
use cf_bridge::local::Bridge;
use cf_engine::runtime::{begin, Begun, Spawn};
use cf_engine::seams::{Adapters, Limits, Roster, SavedAgent, Seams};
use cf_engine::testing::{CountingLaunchIds, FakeAdapter, FakeAdapters, FakeRecords, Recorder};
use cf_engine::Dispatcher;
use cf_harness::seams::{SystemTime, Time};
use cf_ledger::{open_ledger, Ledger, NewProject, NewTask, Options, ProjectView};
use serde_json::{json, Value};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::{TcpListener, TcpStream};

use super::host::Host;
use crate::api::credentials::Credentials;
use crate::errors::Errors;
use crate::files::{Log, Trace};
use crate::host::{daemon_bridge, watch_exits, BridgeHost};
use crate::seams::{DaemonSpawn, LaunchFolders, RoleTexts, WindowEnv};

/// What the bridge runs over.
#[derive(Clone, Copy)]
pub enum Transport {
    /// Memory: a write wakes the reader at once.
    Memory,
    /// A socket on loopback, as the daemon's standard input is a pipe: what is
    /// written is heard when tokio next polls its sockets, which is when it
    /// fires its timers.
    Socket,
}

/// The executor with its driver, the bridge's reader with its checkpoint, and
/// the pane host at the other end: no engine.
pub struct Pieces {
    pub errors: Rc<Errors>,
    pub spawn: Rc<DaemonSpawn>,
    pub bridge: Bridge,
    /// The pane host, as bytes.
    pub host: Host,
    /// The home: the daemon's log and trace, until it goes. Last, so that the
    /// folder goes after what has files open in it (Windows will not remove
    /// them from under it).
    pub home: tempfile::TempDir,
}

impl Pieces {
    /// The pieces over `transport`. Called inside the local set the test runs
    /// in.
    pub async fn new(transport: Transport) -> Self {
        let home = tempfile::tempdir().expect("a home");
        let errors = Rc::new(Errors::new(
            Rc::new(Log::new(home.path())),
            Rc::new(Trace::new(home.path())),
        ));
        let spawn = Rc::new(DaemonSpawn::new(Rc::clone(&errors)));
        spawn.drive();
        let (daemon, host) = match transport {
            Transport::Memory => {
                let (daemon_end, host_end) = tokio::io::duplex(1 << 20);
                (boxed(daemon_end), boxed(host_end))
            }
            Transport::Socket => {
                let listener = TcpListener::bind(("127.0.0.1", 0)).await.expect("a port");
                let host_end = TcpStream::connect(listener.local_addr().expect("an address"))
                    .await
                    .expect("a connection");
                let (daemon_end, _) = listener.accept().await.expect("the connection");
                for end in [&host_end, &daemon_end] {
                    end.set_nodelay(true).expect("no delay");
                }
                (boxed(daemon_end), boxed(host_end))
            }
        };
        let (input, output) = daemon;
        let (bridge, connection) = daemon_bridge(&spawn).connect(input, output);
        errors.spawn("the bridge's task failed", connection);
        let (from_daemon, to_daemon) = host;
        Self {
            errors,
            spawn,
            bridge,
            host: Host::new(from_daemon, to_daemon),
            home,
        }
    }
}

/// One end of a stream, as the read half and the write half.
type Ends = (Box<dyn AsyncRead + Unpin>, Box<dyn AsyncWrite + Unpin>);

fn boxed(end: impl AsyncRead + AsyncWrite + Unpin + 'static) -> Ends {
    let (read, write) = tokio::io::split(end);
    (Box::new(read), Box::new(write))
}

/// Every agent is saved, on a model of its own.
struct AnyAgent;

impl Roster for AnyAgent {
    fn agent(&self, _name: &str) -> Result<Option<SavedAgent>, Refusal> {
        Ok(Some(SavedAgent {
            model: Some("a-model".to_owned()),
            effort: None,
            thinking: None,
            designer: false,
        }))
    }
}

/// What makes the time the engine reads, given the executor's spawn.
type Timing = Box<dyn FnOnce(&Rc<DaemonSpawn>) -> Rc<dyn Time>>;

/// What makes the engine's adapters of the kit's, given the executor's spawn
/// and the time the engine reads.
type Adapting =
    Box<dyn FnOnce(&Rc<DaemonSpawn>, &Rc<dyn Time>, Rc<dyn Adapters>) -> Rc<dyn Adapters>>;

/// How a rig differs from the daemon's own wiring: the time the engine reads,
/// and what stands in for the harnesses' adapters.
pub struct Wiring {
    time: Timing,
    adapters: Adapting,
}

impl Wiring {
    /// The system's own time, and the kit's adapters as they are.
    pub fn new() -> Self {
        Self {
            time: Box::new(|_| Rc::new(SystemTime)),
            adapters: Box::new(|_, _, adapters| adapters),
        }
    }

    /// The engine reads its time from what `time` makes of the executor.
    pub fn time(mut self, time: impl FnOnce(&Rc<DaemonSpawn>) -> Rc<dyn Time> + 'static) -> Self {
        self.time = Box::new(time);
        self
    }

    /// The engine's adapters are what `adapters` makes of the kit's, given the
    /// executor and the time the engine reads.
    pub fn adapters(
        mut self,
        adapters: impl FnOnce(&Rc<DaemonSpawn>, &Rc<dyn Time>, Rc<dyn Adapters>) -> Rc<dyn Adapters>
            + 'static,
    ) -> Self {
        self.adapters = Box::new(adapters);
        self
    }
}

/// An engine and everything it runs on, as the daemon wires them.
pub struct Rig {
    pub ledger: Rc<RefCell<Ledger>>,
    pub dispatcher: Rc<Dispatcher>,
    pub adapter: Rc<FakeAdapter>,
    /// What the engine asked of the adapter, in order.
    pub recorder: Recorder,
    events: Rc<RefCell<Vec<(String, Value)>>>,
    /// What there was when the project opened: its chief's launch is the
    /// setup, and the reads of what follows leave it out.
    opened: Opened,
    /// Last, so that the home goes after the ledger that has its file open.
    pub pieces: Pieces,
}

/// How much was logged and asked when the project opened.
#[derive(Default)]
struct Opened {
    events: usize,
    started: usize,
}

impl Rig {
    /// A rig over memory on the system's own time. Called inside the local
    /// set the test runs in.
    pub async fn new() -> Self {
        Self::wired(Wiring::new()).await
    }

    /// A rig wired as `wiring` says.
    pub async fn wired(wiring: Wiring) -> Self {
        let pieces = Pieces::new(Transport::Memory).await;
        let (home, errors, spawn, bridge) = (
            pieces.home.path(),
            &pieces.errors,
            &pieces.spawn,
            &pieces.bridge,
        );
        let events: Rc<RefCell<Vec<(String, Value)>>> = Rc::default();
        let told = Rc::clone(&events);
        let ledger = open_ledger(
            &home.join("consensflow.db"),
            Options {
                trace: Box::new(move |event| {
                    told.borrow_mut()
                        .push((event.kind.clone(), event.data.clone()));
                }),
                ..Options::default()
            },
        )
        .expect("a ledger");
        let ledger = Rc::new(RefCell::new(ledger));

        let recorder = Recorder::default();
        let adapter = FakeAdapter::new(recorder.clone());
        let fakes = Rc::new(FakeAdapters::new(&adapter, &["claude-code"]));
        let time = (wiring.time)(spawn);
        let adapters = (wiring.adapters)(spawn, &time, Rc::clone(&fakes) as Rc<dyn Adapters>);
        let env = Env::default();
        let seams = Seams {
            ledger: Rc::clone(&ledger),
            host: Rc::new(BridgeHost::new(bridge.clone(), env.clone())),
            adapters,
            records: Rc::new(FakeRecords::new(fakes)),
            time,
            launch_ids: Rc::new(CountingLaunchIds::default()),
            credentials: Rc::new(Credentials::new()),
            pane_env: Rc::new(WindowEnv::new(&env, "http://127.0.0.1:1", "/bundle/bin")),
            roster: Rc::new(AnyAgent),
            roles: Rc::new(RoleTexts::new(env, "/bundle/bin/cf".to_owned())),
            trace: Rc::new(Trace::new(home)),
            log: Rc::new(Log::new(home)),
            launch_files: Rc::new(LaunchFolders::new(home.to_path_buf(), Rc::clone(errors))),
            spawn: Rc::clone(spawn) as Rc<dyn Spawn>,
            limits: Limits::default(),
        };
        let dispatcher = Dispatcher::new(seams);
        let exiting = Rc::clone(&dispatcher);
        // Dropped, it leaves the handler where it is.
        drop(watch_exits(bridge, Rc::clone(spawn), move |pane| {
            exiting.pane_exited(pane)
        }));
        Self {
            pieces,
            ledger,
            dispatcher,
            adapter,
            recorder,
            events,
            opened: Opened::default(),
        }
    }

    /// Begins `work` as the daemon begins an engine operation (a pass, a
    /// request's work): its first part is done here, and what that woke is
    /// run to its end before this returns.
    pub async fn begun<T: 'static>(&self, work: impl Future<Output = T> + 'static) -> Begun<T> {
        let begun = begin(&*self.pieces.spawn, work).await;
        self.pieces.spawn.drain();
        begun
    }

    /// Answers the host's requests, one round after another, until `done`.
    pub async fn serve_until(&mut self, done: impl Fn() -> bool) {
        for _ in 0..50 {
            self.pieces.host.serve(|_| false).await;
            if done() {
                return;
            }
        }
        panic!("what the test waited for did not come about");
    }

    /// Lets the engine and the host go on until nothing is left to do: the
    /// host answers what is asked, and what is ready runs.
    pub async fn quiet(&mut self) {
        for _ in 0..4 {
            self.pieces.host.serve(|_| false).await;
        }
    }

    /// A project of a chief (`apollo`) and a worker for each of `workers`,
    /// its chief's window open.
    pub async fn open_project(&mut self, workers: &[&str]) -> ProjectView {
        let staff: Vec<Value> = workers
            .iter()
            .map(|agent| {
                json!({ "agent": agent, "harness": "claude-code", "role": "worker", "tier": "standard" })
            })
            .collect();
        let request = NewProject::from_json(&json!({
            "directory": "/work/app",
            "name": "app",
            "chief": { "harness": "claude-code", "agent": "apollo" },
            "staff": staff,
        }))
        .expect("a project");
        let dispatcher = Rc::clone(&self.dispatcher);
        let opening = self
            .begun(async move { dispatcher.open_project(request).await })
            .await;
        self.serve_until(|| opening.ended()).await;
        let project = opening
            .await
            .expect("the project opened")
            .expect("the project is there");
        // The chief's window is asked for apart, and has more to do once open.
        self.quiet().await;
        self.mark();
        project
    }

    /// What follows is what a test reads: the events and the calls of the
    /// adapter are counted from here.
    pub fn mark(&mut self) {
        self.opened = Opened {
            events: self.events.borrow().len(),
            started: self.asked("started"),
        };
    }

    /// A task from the chief to `to`.
    pub fn give(&self, project: i64, to: &str, body: &str) {
        self.ledger
            .borrow_mut()
            .create_task(
                project,
                &NewTask {
                    from: "chief".to_owned(),
                    to: Some(to.to_owned()),
                    body: body.to_owned(),
                    ..NewTask::default()
                },
            )
            .expect("a task");
    }

    /// One pass, begun as the loop begins it: its first part done, what that
    /// woke run to its end.
    pub async fn pass(&self) -> Begun<Result<(), String>> {
        let dispatcher = Rc::clone(&self.dispatcher);
        self.begun(async move { dispatcher.pass().await.map_err(|failed| failed.to_string()) })
            .await
    }

    /// `count` passes, one after another, each to its end, the host answering
    /// what the engine asks of it.
    pub async fn passes(&mut self, count: usize) {
        for _ in 0..count {
            let pass = self.pass().await;
            self.serve_until(|| pass.ended()).await;
            pass.await.expect("a pass");
            self.quiet().await;
        }
    }

    /// The state of task `number`.
    pub fn task_state(&self, project: i64, number: i64) -> String {
        self.ledger
            .borrow()
            .task(project, number)
            .expect("the ledger read")
            .expect("the task")
            .task
            .state
    }

    /// The events the ledger logged since the mark, in order: their kind and
    /// data.
    pub fn events(&self) -> Vec<(String, Value)> {
        self.events.borrow()[self.opened.events..].to_vec()
    }

    /// How many times the engine asked `method` of the adapter's windows
    /// since the mark.
    pub fn calls(&self, method: &str) -> usize {
        self.asked(method) - self.opened.started
    }

    /// How many times in all.
    fn asked(&self, method: &str) -> usize {
        self.recorder.calls("adapter:claude-code", &[method]).len()
    }
}

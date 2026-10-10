//! What a test of an OpenCode window stands on: a throwaway home with a
//! stand-in `opencode` on PATH, the servers and children a prepare talks to,
//! scripted, and the launch a test asks to have prepared. The throwaway
//! `serve` is a scripted child, and the programs a window starts are kept as
//! they were given, to be looked at after.

use std::cell::RefCell;
use std::fs;
use std::future::Future;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

use cf_base::env::Env;
use cf_base::path;
use cf_harness::contract::{Adapter, Agent, Launch, LaunchId, Pane, Prepared, Records, Work};
use cf_harness::opencode::OpenCodeAdapter;
use cf_harness::records::{Item, Options, Reading, Record, Role, Settlement};
use cf_harness::seams::processes::{Child, Ending, Failed, Limits, Processes, Program, Streams};
use cf_harness::seams::Services;
use cf_harness::testing::{
    fake_executable, ChildScript, Driver, Ends, Fakes, ScriptedProcesses, Sent, Served,
    FIRST_MESSAGE_MS,
};
use cf_proto::agents::Harness;
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::scenarios::real_name;

/// The launch's id: a uuid, as the engine mints one (Node's tests took any
/// filename-safe word).
pub(super) const LAUNCH: &str = "0a1b2c3d-4e5f-4061-8a7b-9c0d1e2f3a4b";
pub(super) const TASK: &str =
    "[ConsensFlow m-1 \u{b7} T-1 \u{b7} task from @chief]\nWrite the parser";
pub(super) const MODEL: &str = "opencode/muse-spark-1.3-contributor-free";
pub(super) const ROLE: &str = "# ConsensFlow worker\n\nRole text for the test.";

/// A throwaway home, ConsensFlow's folder, a folder to work in and a
/// stand-in `opencode` on PATH.
pub(super) struct Home {
    _dir: Rc<TempDir>,
    pub(super) root: String,
    vars: Vec<(String, String)>,
    /// Where the stand-in `opencode` is found.
    pub(super) executable: String,
}

impl Home {
    /// A home with a stand-in `opencode` on PATH.
    pub(super) fn new() -> Self {
        let mut home = Self::without_opencode();
        home.executable = home.install_opencode();
        home
    }

    /// A home whose PATH holds no `opencode`, yet. Its folders are as the
    /// system names them, so a folder a window asks the real name of is the
    /// one it is written as.
    pub(super) fn without_opencode() -> Self {
        let dir = tempfile::Builder::new()
            .prefix("cf-opencode-adapter-")
            .tempdir()
            .unwrap();
        let root = real_name(dir.path()).to_string_lossy().into_owned();
        let vars: Vec<(String, String)> = [
            ("HOME", "home"),
            ("CONSENSFLOW_HOME", "consensflow"),
            ("PATH", "bin"),
        ]
        .into_iter()
        .map(|(name, folder)| (name.to_owned(), path::join(&[&root, folder])))
        .collect();
        fs::create_dir_all(Path::new(&root).join("bin")).unwrap();
        fs::create_dir_all(Path::new(&root).join("work")).unwrap();
        Self {
            _dir: Rc::new(dir),
            root,
            vars,
            executable: String::new(),
        }
    }

    /// This home, its environment with `name` set to `value` besides.
    pub(super) fn sharing(&self, name: &str, value: &str) -> Self {
        let mut vars = self.vars.clone();
        vars.retain(|(held, _)| held != name);
        vars.push((name.to_owned(), value.to_owned()));
        Self {
            _dir: Rc::clone(&self._dir),
            root: self.root.clone(),
            vars,
            executable: self.executable.clone(),
        }
    }

    /// Puts a stand-in `opencode` on PATH: where it is found.
    pub(super) fn install_opencode(&self) -> String {
        let file = Path::new(&self.root).join("bin").join("opencode");
        fake_executable(&file).to_string_lossy().into_owned()
    }

    pub(super) fn var(&self, name: &str) -> &str {
        let found = self.vars.iter().find(|(held, _)| held == name);
        &found.unwrap().1
    }

    pub(super) fn env(&self) -> Env {
        Env::from_vars(self.vars.iter().cloned())
    }

    /// The folder a window works in.
    pub(super) fn work(&self) -> String {
        path::join(&[&self.root, "work"])
    }

    /// Where the launch's own files are.
    pub(super) fn launch_folder(&self) -> String {
        path::join(&[
            self.var("CONSENSFLOW_HOME"),
            "integrations",
            "opencode",
            LAUNCH,
        ])
    }
}

/// A launch to have prepared: a worker's, unless a test changes it.
#[derive(Clone)]
pub(super) struct Wanted {
    pub(super) resume: Option<String>,
    pub(super) message: Option<String>,
    pub(super) instructions: String,
    pub(super) role: &'static str,
    pub(super) model: Option<String>,
    pub(super) effort: Option<String>,
    /// The folder it works in, the home's own where none is said.
    pub(super) directory: Option<String>,
}

impl Default for Wanted {
    fn default() -> Self {
        Self {
            resume: None,
            message: Some(TASK.to_owned()),
            instructions: ROLE.to_owned(),
            role: "worker",
            model: Some(MODEL.to_owned()),
            effort: Some("high".to_owned()),
            directory: None,
        }
    }
}

impl Wanted {
    /// A launch of the conversation `session`, kept.
    pub(super) fn resuming(session: &str) -> Self {
        Self {
            resume: Some(session.to_owned()),
            message: None,
            ..Self::default()
        }
    }
}

/// The programs a test scripts, each one started kept as it was given, and
/// each child started kept to be looked at when the window is done with it.
pub(super) struct Watching {
    scripted: Rc<ScriptedProcesses>,
    pub(super) started: RefCell<Vec<Program>>,
    pub(super) children: RefCell<Vec<Rc<dyn Child>>>,
}

/// A child the window holds and the test looks at.
struct Shared(Rc<dyn Child>);

impl Child for Shared {
    fn write_line<'a>(&'a self, line: &'a str) -> Work<'a, Result<(), String>> {
        self.0.write_line(line)
    }

    fn read_line(&self, limit: usize) -> Work<'_, Result<Option<String>, String>> {
        self.0.read_line(limit)
    }

    fn exited(&self) -> bool {
        self.0.exited()
    }

    fn closed(&self) -> Work<'_, ()> {
        self.0.closed()
    }

    fn terminate(&self, how: Ending) {
        self.0.terminate(how);
    }
}

impl Processes for Watching {
    fn run(&self, program: Program, limits: Limits) -> Work<'_, Result<String, Failed>> {
        self.scripted.run(program, limits)
    }

    fn spawn(&self, program: Program, streams: Streams) -> Result<Box<dyn Child>, String> {
        self.started.borrow_mut().push(program.clone());
        let child: Rc<dyn Child> = Rc::from(self.scripted.spawn(program, streams)?);
        self.children.borrow_mut().push(Rc::clone(&child));
        Ok(Box::new(Shared(child)))
    }
}

/// An adapter, the fakes that serve it, and the programs it started.
pub(super) struct Stage {
    pub(super) home: Home,
    pub(super) fakes: Fakes,
    pub(super) started: Rc<Watching>,
    pub(super) adapter: Rc<OpenCodeAdapter>,
}

impl Stage {
    /// A stage whose windows read the conversation's own record, from the
    /// files of its home.
    pub(super) fn new() -> Self {
        Self::with(Home::new(), None)
    }

    /// A stage on `home`, its windows' records `records` if given.
    pub(super) fn with(home: Home, records: Option<Rc<dyn Records>>) -> Self {
        let env = home.env();
        let fakes = Fakes::new(&env);
        let started = Rc::new(Watching {
            scripted: Rc::clone(&fakes.processes),
            started: RefCell::new(Vec::new()),
            children: RefCell::new(Vec::new()),
        });
        let mut services: Services = fakes.services(&env, Path::new(&home.root));
        services.processes = Rc::clone(&started) as Rc<dyn Processes>;
        if let Some(records) = records {
            services.records = records;
        }
        let adapter = Rc::new(OpenCodeAdapter::new(&services));
        Self {
            home,
            fakes,
            started,
            adapter,
        }
    }

    /// Scripts what a fresh window's throwaway server answers: healthy, and
    /// a session `id` made in the folder the window works in.
    pub(super) fn serves_creation(&self, id: &str) {
        self.fakes
            .loopback
            .serve("GET /global/health", [served(200, br#"{"healthy":true}"#)]);
        let made = json!({ "id": id, "directory": self.home.work() });
        self.fakes
            .loopback
            .serve("POST /session", [served(200, made.to_string().as_bytes())]);
        self.fakes.processes.child(
            "opencode",
            ChildScript {
                lines: Vec::new(),
                ends: Ends::Asked,
            },
        );
    }

    /// A window prepared, the clock moved on whenever it waits for a timer.
    pub(super) fn prepare(&self, wanted: &Wanted) -> Result<Prepared, String> {
        finish(&self.fakes, self.preparing(wanted))
    }

    /// The work of preparing a window, for a test that moves the clock
    /// itself.
    pub(super) fn preparing(
        &self,
        wanted: &Wanted,
    ) -> impl Future<Output = Result<Prepared, String>> + 'static {
        let adapter = Rc::clone(&self.adapter);
        let directory = wanted.directory.clone().unwrap_or_else(|| self.home.work());
        let wanted = wanted.clone();
        async move {
            let id = LaunchId::new(LAUNCH).unwrap();
            let agent = Agent {
                model: wanted.model.as_deref(),
                effort: wanted.effort.as_deref(),
                thinking: None,
                designer: false,
            };
            let launch = Launch {
                id: &id,
                project: 1,
                handle: "zeus",
                role: wanted.role,
                directory: &directory,
                resume: wanted.resume.as_deref(),
                message: wanted.message.as_deref(),
                agent: Some(agent),
                instructions: &wanted.instructions,
                first_message_ms: FIRST_MESSAGE_MS,
            };
            adapter.prepare(&launch).await
        }
    }

    /// A window prepared as a fresh one, its conversation `id`.
    pub(super) fn fresh(&self, id: &str) -> Prepared {
        self.serves_creation(id);
        self.prepare(&Wanted::default()).unwrap()
    }

    /// A window prepared as `session` resumed.
    pub(super) fn resumed(&self, session: &str) -> Prepared {
        self.prepare(&Wanted::resuming(session)).unwrap()
    }
}

/// `work` run to its end, the clock moved on to the next timer each time it
/// waits for one.
pub(super) fn finish<T: 'static>(fakes: &Fakes, work: impl Future<Output = T> + 'static) -> T {
    let mut driver = Driver::default();
    driver.begin(0, work);
    loop {
        if let Some((_, answer)) = driver.run().pop() {
            return answer;
        }
        assert!(
            fakes.time.fire_next(i64::MAX),
            "work waits on nothing a test can end"
        );
    }
}

/// An answer of `status` with `body`, which a peer on loopback gives at once.
pub(super) fn served(status: u16, body: &[u8]) -> Served {
    Served::Head {
        status,
        body: Sent::Now(body.to_vec()),
    }
}

/// What the plugin says the window shows: `session`, none on its home
/// screen, and what OpenCode is doing.
pub(super) fn shows(session: Option<&str>, status: Value) -> Served {
    let said = json!({ "launchId": LAUNCH, "sessionId": session, "status": status });
    served(200, said.to_string().as_bytes())
}

pub(super) fn pane() -> Pane {
    Pane {
        id: "s1-zeus".to_owned(),
        generation: 2,
    }
}

/// The value of the variable `name` a plan sets for the window.
pub(super) fn planned<'a>(plan: &'a Prepared, name: &str) -> Option<&'a str> {
    let found = plan.env.iter().find(|(held, _)| held == name);
    found.map(|(_, value)| value.as_str())
}

/// Where a plan's plugin is asked `route`.
pub(super) fn planned_bridge(plan: &Prepared, route: &str) -> String {
    let bridge: Value =
        serde_json::from_str(planned(plan, "CF_OPENCODE_SESSION_BRIDGE").unwrap()).unwrap();
    format!("http://127.0.0.1:{}{route}", bridge["port"])
}

pub(super) fn words(words: &[&str]) -> Vec<String> {
    words.iter().map(|&word| word.to_owned()).collect()
}

/// A record that says what the test wants of it, and what it was asked.
pub(super) struct Stand {
    pub(super) asked: RefCell<Vec<(Harness, String)>>,
    reading: RefCell<Arc<Reading>>,
}

impl Stand {
    pub(super) fn saying(reading: Reading) -> Rc<Self> {
        Rc::new(Self {
            asked: RefCell::new(Vec::new()),
            reading: RefCell::new(Arc::new(reading)),
        })
    }

    pub(super) fn say(&self, reading: Reading) {
        *self.reading.borrow_mut() = Arc::new(reading);
    }
}

impl Records for Stand {
    fn look<'a>(
        &'a self,
        harness: Harness,
        session: &'a str,
        _options: &'a Options,
    ) -> Work<'a, Arc<Reading>> {
        self.asked.borrow_mut().push((harness, session.to_owned()));
        Box::pin(async { Arc::clone(&self.reading.borrow()) })
    }

    fn has_transcript<'a>(
        &'a self,
        _harness: Harness,
        _session: &'a str,
    ) -> Work<'a, Result<bool, String>> {
        Box::pin(async { Ok(false) })
    }
}

/// The record of a conversation with nothing said in it yet.
pub(super) fn empty_record() -> Reading {
    Reading::Known(Record {
        items: Vec::new(),
        in_flight: false,
        asking: false,
        failed: false,
        quota: None,
        settlement: Settlement::Unknown,
    })
}

/// The record of a conversation a turn of which is in flight.
pub(super) fn working_record() -> Record {
    Record {
        items: vec![Item {
            id: Arc::from("u"),
            role: Role::User,
            text: Arc::from(""),
            complete: true,
            at: None,
            commentary: false,
        }],
        in_flight: true,
        asking: false,
        failed: false,
        quota: None,
        settlement: Settlement::InFlight,
    }
}

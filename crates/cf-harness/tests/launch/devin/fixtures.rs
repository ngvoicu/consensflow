//! What the ported tests of Devin's adapter are made of: a throwaway home with a
//! stand-in `devin`, the launch to prepare, and a record that says what a test
//! says it does.

use std::cell::RefCell;
use std::fs;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

use cf_base::env::Env;
use cf_base::path;
use cf_harness::contract::{
    Adapter, Agent, HostError, Launch, LaunchId, Pane, Prepared, Records, Window, Work,
};
use cf_harness::devin::DevinAdapter;
use cf_harness::records::{Item, Options, Reading, Record, Role, Settlement};
use cf_harness::testing::{called, fake_executable, finished, named, AnsweringHost, Fakes};
use cf_proto::agents::Harness;
use serde_json::{json, Value};
use tempfile::TempDir;

/// The launch's id: a uuid, as the engine mints one (Node's tests took any
/// filename-safe word).
pub(super) const LAUNCH: &str = "2c3d4e5f-6071-4283-9cad-1e2f3a4b5c6d";
pub(super) const TASK: &str = "[ConsensFlow m-1 · T-1 · task from @chief]\nWrite the parser";

/// A throwaway home, Devin config folder and a stand-in `devin` on PATH that
/// says its version.
pub(super) struct Home {
    pub(super) _dir: TempDir,
    pub(super) root: String,
    pub(super) vars: Vec<(String, String)>,
    pub(super) executable: String,
    pub(super) fakes: Fakes,
}

impl Home {
    pub(super) fn new() -> Self {
        Self::on(&[], "devin")
    }

    /// A machine whose environment says Windows, where Devin is a program with
    /// an extension Windows starts, which a POSIX machine finds in its place.
    pub(super) fn windows() -> Self {
        Self::on(
            &[("OS", "Windows_NT")],
            if cfg!(windows) { "devin" } else { "devin.exe" },
        )
    }

    pub(super) fn on(more: &[(&str, &str)], stand_in: &str) -> Self {
        let dir = tempfile::Builder::new()
            .prefix("cf-devin-adapter-")
            .tempdir()
            .unwrap();
        let root = dir.path().to_string_lossy().into_owned();
        let mut vars: Vec<(String, String)> = [
            ("HOME", "home"),
            ("CONSENSFLOW_HOME", "consensflow"),
            ("XDG_CONFIG_HOME", "config"),
            ("PATH", "bin"),
        ]
        .into_iter()
        .map(|(name, folder)| (name.to_owned(), path::join(&[&root, folder])))
        .collect();
        vars.extend(
            more.iter()
                .map(|&(name, value)| (name.to_owned(), value.to_owned())),
        );
        fs::create_dir_all(dir.path().join("bin")).unwrap();
        let executable = fake_executable(&dir.path().join("bin").join(stand_in));
        let fakes = Fakes::new(&Env::from_vars(vars.iter().cloned()));
        let home = Self {
            _dir: dir,
            root,
            vars,
            executable: executable.to_string_lossy().into_owned(),
            fakes,
        };
        home.says("devin 3000.10.22");
        home
    }

    /// What the stand-in `devin` says to `--version`, its line end with it.
    pub(super) fn says(&self, version: &str) {
        let line_end = if cfg!(windows) { "\r\n" } else { "\n" };
        self.fakes.processes.every_answer(
            &called(Path::new(&self.executable)),
            Ok(format!("{version}{line_end}")),
        );
    }

    /// The programs run since the last time this asked, each as a test names
    /// it (`devin --version`).
    pub(super) fn asked(&self) -> Vec<String> {
        self.fakes
            .processes
            .take_ran()
            .iter()
            .map(|(program, _)| named(program))
            .collect()
    }

    pub(super) fn env(&self) -> Env {
        Env::from_vars(self.vars.iter().cloned())
    }

    /// An adapter of windows that run in this home, the engine's record read as `records`.
    pub(super) fn adapter_over(&self, records: Option<Rc<dyn Records>>) -> DevinAdapter {
        let env = self.env();
        let mut services = self.fakes.services(&env, Path::new(&self.root));
        if let Some(records) = records {
            services.records = records;
        }
        DevinAdapter::new(&services)
    }

    pub(super) fn adapter(&self) -> DevinAdapter {
        self.adapter_over(None)
    }

    /// Where the launch's own files are.
    pub(super) fn folder(&self) -> String {
        path::join(&[&self.root, "consensflow", "integrations", "devin", LAUNCH])
    }

    pub(super) fn wire(&self) -> String {
        path::join(&[&self.folder(), "wire.jsonl"])
    }

    /// Devin's own log says its window now shows `session`.
    pub(super) fn selects(&self, session: &str) {
        fs::write(self.wire(), shows(session)).unwrap();
    }

    pub(super) fn appends(&self, text: &str) {
        let mut log = fs::read(self.wire()).unwrap();
        log.extend_from_slice(text.as_bytes());
        fs::write(self.wire(), log).unwrap();
    }
}

/// The line the log gains when its window configures a conversation it opens.
pub(super) fn shows(session: &str) -> String {
    let record = json!({
        "sessionId": session,
        "update": { "sessionUpdate": "config_option_update", "configOptions": [{ "id": "mode" }] },
    });
    format!("{record}\n")
}

/// A launch to prepare: a worker's, unless a test changes it.
pub(super) struct Request {
    pub(super) role: &'static str,
    pub(super) handle: &'static str,
    pub(super) instructions: String,
    pub(super) resume: Option<String>,
    pub(super) message: Option<String>,
    pub(super) agent: Option<Agent<'static>>,
}

impl Default for Request {
    fn default() -> Self {
        Self {
            role: "worker",
            handle: "zeus",
            instructions: "# ConsensFlow worker\n\nRole text for the test.".to_owned(),
            resume: None,
            message: Some(TASK.to_owned()),
            agent: Some(Agent {
                model: Some("swe-1-6-slow"),
                ..Agent::default()
            }),
        }
    }
}

impl Request {
    /// A chief's launch: no model of its own, no first message.
    pub(super) fn chief() -> Self {
        Self {
            role: "chief",
            handle: "chief",
            agent: None,
            message: None,
            ..Self::default()
        }
    }

    /// A window on the conversation `session`, opened again.
    pub(super) fn resumed(session: &str) -> Self {
        Self {
            resume: Some(session.to_owned()),
            message: None,
            ..Self::default()
        }
    }
}

pub(super) fn prepare_as(
    adapter: &DevinAdapter,
    request: &Request,
    id: &str,
) -> Result<Prepared, String> {
    let id = LaunchId::new(id).unwrap();
    let launch = Launch {
        id: &id,
        project: 1,
        handle: request.handle,
        role: request.role,
        directory: "/work/app",
        resume: request.resume.as_deref(),
        message: request.message.as_deref(),
        agent: request.agent,
        instructions: &request.instructions,
    };
    finished(adapter.prepare(&launch))
}

pub(super) fn prepare(adapter: &DevinAdapter, request: &Request) -> Result<Prepared, String> {
    prepare_as(adapter, request, LAUNCH)
}

/// The window of a prepared launch.
pub(super) fn window(adapter: &DevinAdapter, request: &Request) -> Rc<dyn Window> {
    prepare(adapter, request).unwrap().window
}

pub(super) fn words(words: &[&str]) -> Vec<String> {
    words.iter().map(|&word| word.to_owned()).collect()
}

pub(super) fn pane() -> Pane {
    Pane {
        id: "s1-zeus".to_owned(),
        generation: 2,
    }
}

/// A host whose window can be read, with no paste on its way, and takes every paste.
pub(super) fn readable() -> AnsweringHost<impl Fn(&str) -> Result<Value, HostError>> {
    AnsweringHost::new(|op| {
        Ok(if op == "pane.snapshot" {
            json!({ "ok": true, "pasteInFlight": false })
        } else {
            json!({ "ok": true })
        })
    })
}

/// What Devin's record of a conversation says, as the test says it, and
/// every conversation it was asked of.
pub(super) struct Said {
    pub(super) record: Record,
    pub(super) looked: RefCell<Vec<String>>,
}

impl Said {
    pub(super) fn new(record: Record) -> Rc<Self> {
        Rc::new(Self {
            record,
            looked: RefCell::new(Vec::new()),
        })
    }
}

/// A record that settled, holding one message of the conversation's own.
pub(super) fn settled(session: &str) -> Record {
    Record {
        items: vec![Item {
            id: Arc::from(format!("{session}-1").as_str()),
            role: Role::User,
            text: Arc::from("hello"),
            complete: true,
            at: None,
            commentary: false,
        }],
        in_flight: false,
        asking: false,
        failed: false,
        quota: None,
        settlement: Settlement::Settled,
    }
}

impl Records for Said {
    fn look<'a>(
        &'a self,
        _harness: Harness,
        session: &'a str,
        _options: &'a Options,
    ) -> Work<'a, Arc<Reading>> {
        self.looked.borrow_mut().push(session.to_owned());
        let mut record = self.record.clone();
        for item in &mut record.items {
            item.id = Arc::from(format!("{session}-1").as_str());
        }
        Box::pin(async move { Arc::new(Reading::Known(record)) })
    }

    fn has_transcript<'a>(
        &'a self,
        _harness: Harness,
        _session: &'a str,
    ) -> Work<'a, Result<bool, String>> {
        Box::pin(async { Ok(false) })
    }
}

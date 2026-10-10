//! What the candidate's tests are run on: a machine of the test's own in a
//! temporary tree (the checkout, a home, the system's applications with a live
//! app installed), and a system that is a script: the programs it is asked to
//! run answer as the test says, leave in the tree what they would leave, and
//! are written down in the order they were run.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use cf_base::env::Env;
use cf_base::time::Clock;

use crate::candidate::system::System;
use crate::candidate::{plan, Error, Plan};
use crate::context::Context;
use crate::process::{self, Captured, Invocation};
use crate::updater_smoke::processes::Row;

/// The `Info.plist` of a bundle that is `identifier`, at `version`.
pub fn plist(identifier: &str, version: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
         \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\"><dict>\
         <key>CFBundleIdentifier</key><string>{identifier}</string>\
         <key>CFBundleShortVersionString</key><string>{version}</string>\
         </dict></plist>\n"
    )
}

/// Writes `text` to `path`, in folders made for it.
pub fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

/// A bundle at `app` that is `identifier` at `version`, with a program, a `cf`
/// and a resource in it.
pub fn bundle(app: &Path, identifier: &str, version: &str) {
    write(
        &app.join("Contents").join("Info.plist"),
        &plist(identifier, version),
    );
    write(&app.join("Contents").join("MacOS").join("app"), "program");
    write(
        &app.join("Contents")
            .join("Resources")
            .join("cli")
            .join("bin")
            .join("cf"),
        "cf",
    );
}

/// Copies the tree at `from` to `to`.
fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// The time it is, which a test sets.
pub struct Fixed(pub i64);

impl Clock for Fixed {
    fn now_ms(&mut self) -> i64 {
        self.0
    }
}

/// A user's machine in a temporary tree: a checkout, a home, and the system's
/// applications with the live app installed, which has a roster in the live home.
pub struct World {
    pub temp: tempfile::TempDir,
    pub context: Context,
    pub applications: PathBuf,
}

impl World {
    pub fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("checkout");
        let home = temp.path().join("home");
        fs::create_dir_all(root.join("app")).unwrap();
        let context = Context {
            root,
            env: Env::from_vars([("HOME", home.into_os_string())]),
        };
        let world = Self {
            applications: temp.path().join("Applications"),
            temp,
            context,
        };
        let live = world.plan().paths;
        bundle(&live.live_app, "dev.ngvoicu.consensflow", "3.0.0-alpha.82");
        write(&live.live_roster, "{\"schemaVersion\":1,\"agents\":[]}\n");
        world
    }

    /// What a run on this machine does.
    pub fn plan(&self) -> Plan {
        plan(&self.context, &self.applications).unwrap()
    }

    /// A row of the table of processes that runs `command`.
    pub fn row(pid: u32, command: &str) -> Row {
        Row {
            pid,
            ppid: 1,
            state: "S".to_owned(),
            command: command.to_owned(),
        }
    }
}

/// What a hook does when its program is run.
type Hook = Box<dyn FnMut()>;

/// The system a candidate run is given: scripted.
pub struct Fake {
    paths: crate::candidate::Paths,
    /// The processes that are running.
    pub table: Vec<Row>,
    /// The pids that are alive.
    pub alive: Vec<u32>,
    /// Every program run or asked of, in order.
    pub ran: Vec<Invocation>,
    /// What the build leaves as its bundle's identity and version, if it leaves a bundle.
    pub identity: String,
    pub version: String,
    pub leaves_a_bundle: bool,
    /// The status a program ends with, by the name it is started by; 0 where none is named.
    pub statuses: BTreeMap<&'static str, i32>,
    /// Programs that cannot be started.
    pub missing: Vec<&'static str>,
    /// What `git rev-parse HEAD` and `git status --porcelain` say, and their status.
    pub head: String,
    pub porcelain: String,
    pub git_status: i32,
    /// What happens when a program is run, by the name it is started by.
    hooks: Vec<(&'static str, Hook)>,
}

impl Fake {
    pub fn new(plan: &Plan) -> Self {
        Self {
            paths: plan.paths.clone(),
            table: Vec::new(),
            alive: Vec::new(),
            ran: Vec::new(),
            identity: crate::candidate::IDENTIFIER.to_owned(),
            version: "3.0.0-alpha.83".to_owned(),
            leaves_a_bundle: true,
            statuses: BTreeMap::new(),
            missing: Vec::new(),
            head: "0123456789abcdef0123456789abcdef01234567".to_owned(),
            porcelain: String::new(),
            git_status: 0,
            hooks: Vec::new(),
        }
    }

    /// Something that happens when `program` is run, before it ends.
    pub fn on(&mut self, program: &'static str, hook: impl FnMut() + 'static) {
        self.hooks.push((program, Box::new(hook)));
    }

    /// The lines of what was run, as a person reads each.
    pub fn lines(&self) -> Vec<String> {
        self.ran.iter().map(Invocation::display).collect()
    }
}

impl System for Fake {
    fn run(&mut self, invocation: &Invocation) -> Result<i32, Error> {
        self.ran.push(invocation.clone());
        let program = invocation.program.to_string_lossy().into_owned();
        if self.missing.contains(&program.as_str()) {
            return Err(process::Failure::NotFound { program }.into());
        }
        for (named, hook) in &mut self.hooks {
            if *named == program {
                hook();
            }
        }
        let status = self.statuses.get(program.as_str()).copied().unwrap_or(0);
        if status == 0 {
            match program.as_str() {
                "npm" if self.leaves_a_bundle => {
                    bundle(&self.paths.built, &self.identity, &self.version);
                }
                "/usr/bin/ditto" => copy_tree(
                    Path::new(&invocation.args[0]),
                    Path::new(&invocation.args[1]),
                ),
                _ => {}
            }
        }
        Ok(status)
    }

    fn capture(&mut self, invocation: &Invocation) -> Result<Captured, Error> {
        self.ran.push(invocation.clone());
        let stdout = match invocation.args.first().and_then(|word| word.to_str()) {
            Some("rev-parse") => format!("{}\n", self.head),
            Some("status") => self.porcelain.clone(),
            _ => String::new(),
        };
        Ok(Captured {
            code: self.git_status,
            stdout,
            stderr: if self.git_status == 0 {
                String::new()
            } else {
                "fatal: not a git repository\n".to_owned()
            },
        })
    }

    fn table(&mut self) -> Result<Vec<Row>, Error> {
        Ok(self.table.clone())
    }

    fn alive(&mut self, pid: u32) -> bool {
        self.alive.contains(&pid)
    }
}

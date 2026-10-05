//! A trace of the page's operations, played as `FORMAT.md` says: the world put
//! in place, the stand-ins set up from what each operation's `seams` say, the
//! operation asked over a bridge as the app asks it, and its reply (bytes), its
//! kicks and the events its own ledger calls logged compared with Node's; the
//! ledger left as Node left it.

use std::cell::Cell;
use std::path::Path;
use std::rc::Rc;
use std::time::Duration;

use cf_bridge::local::Bridge;
use cf_daemon::errors::Errors;
use cf_daemon::files::{Log, Trace};
use cf_daemon::host::daemon_bridge;
use cf_daemon::page::{self, Engine, Page};
use cf_daemon::seams::DaemonSpawn;
use rusqlite::Connection;
use serde_json::Value;
use tokio::io::{duplex, split};
use tokio::task::{JoinHandle, LocalSet};

use crate::ledger::{differs, Rig};
use crate::standin::Standin;
use crate::trace;
use crate::world::World;

/// The daemon's end of a bridge with the page's operations on it, and the
/// app's end that asks them.
struct Connected {
    app: Bridge,
    tasks: [JoinHandle<()>; 2],
}

impl Drop for Connected {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}

/// What a trace needs to be played.
struct Player {
    rig: Rc<Rig>,
    /// The ledger the operations read: the same one, but for a trace whose
    /// test gave the page a ledger of its own.
    page_ledger: Rc<Rig>,
    standin: Rc<Standin>,
    spawn: Rc<DaemonSpawn>,
    kicks: Rc<Cell<u32>>,
    world: World,
    connected: Option<Connected>,
    problems: Vec<String>,
}

/// Plays the trace `name`: why it was not answered as Node's was; none when it was.
pub fn play(name: &str) -> Vec<String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    LocalSet::new().block_on(&runtime, replay(name))
}

async fn replay(name: &str) -> Vec<String> {
    let root = tempfile::tempdir().unwrap();
    let daemon_home = tempfile::tempdir().unwrap();
    let ledger = root.path().join("consensflow.db");
    let trace = trace::load(name, root.path(), &ledger);
    let steps = trace["steps"].as_array().cloned().unwrap_or_default();

    let errors = Rc::new(Errors::new(
        Rc::new(Log::new(daemon_home.path())),
        Rc::new(Trace::new(daemon_home.path())),
    ));
    let spawn = Rc::new(DaemonSpawn::new(errors));
    spawn.drive();

    let rig = Rc::new(Rig::open(&ledger));
    let page_ledger = match page_staff(&steps) {
        Some(staff) => Rc::new(seeded(&root.path().join("page.db"), &staff)),
        None => Rc::clone(&rig),
    };
    let mut player = Player {
        standin: Rc::new(Standin::new(Rc::clone(&rig))),
        rig,
        page_ledger,
        spawn,
        kicks: Rc::new(Cell::new(0)),
        world: World::new(root.path()),
        connected: None,
        problems: Vec::new(),
    };
    let mut closed = false;
    for (at, step) in steps.iter().enumerate() {
        let before = player.problems.len();
        match step["kind"].as_str().unwrap_or_default() {
            "world" => {
                if player.world.put(step) {
                    player.connected = None;
                }
            }
            "ledger" if step["method"] == "close" => {
                player.rig.close_into(&trace["ledger"]["final"]);
                closed = true;
            }
            "ledger" => player.rig.apply(step),
            "operation" => player.operation(step).await,
            other => player.problems.push(format!(
                "a step of kind {other}, which a page trace has not"
            )),
        }
        let noted = player.rig.problems();
        player.problems.extend(noted);
        for problem in &mut player.problems[before..] {
            *problem = format!("step {at}: {problem}");
        }
    }
    if !closed {
        player
            .problems
            .push("the trace never closed its ledger".to_owned());
    }
    if !Rc::ptr_eq(&player.rig, &player.page_ledger) {
        let _ = player.page_ledger.ledger.borrow_mut().close_in_place();
    }
    player.problems
}

impl Player {
    /// The page, over the environment the world has now.
    fn connect(&mut self) -> &Bridge {
        let kicks = Rc::clone(&self.kicks);
        let engine: Rc<dyn Engine> = Rc::clone(&self.standin) as _;
        let page = Rc::new(Page {
            ledger: Rc::clone(&self.page_ledger.ledger),
            engine,
            env: self.world.env(),
            kick: Rc::new(move || kicks.set(kicks.get() + 1)),
        });
        let (daemon_end, app_end) = duplex(256 * 1024);
        let (daemon_input, daemon_output) = split(daemon_end);
        let (app_input, app_output) = split(app_end);
        let (daemon, daemon_connection) =
            daemon_bridge(&self.spawn).connect(daemon_input, daemon_output);
        let (app, app_connection) =
            cf_bridge::local::BridgeBuilder::new(cf_proto::bridge::Role::Host)
                .connect(app_input, app_output);
        page::register(&daemon, &page, &self.spawn);
        let tasks = [
            tokio::task::spawn_local(daemon_connection),
            tokio::task::spawn_local(app_connection),
        ];
        // The daemon's end is kept alive by its connection.
        drop(daemon);
        &self.connected.insert(Connected { app, tasks }).app
    }

    /// One operation, asked as the app asks it.
    async fn operation(&mut self, step: &Value) {
        let name = step["name"].as_str().unwrap_or_default().to_owned();
        let what = format!("operation {} ({name})", step["id"]);
        self.rig.queue(step);
        // The stand-ins of the test: what the daemon would give, but for the
        // page's own ledger, which the page reads itself.
        self.standin.expect(
            step["seams"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|seam| seam["seam"] != "ledger")
                .cloned()
                .collect(),
        );
        self.kicks.set(0);
        let app = match &self.connected {
            Some(connected) => connected.app.clone(),
            None => self.connect().clone(),
        };
        let answered = tokio::time::timeout(
            Duration::from_secs(10),
            app.request(&name, step["body"].clone(), Some(Duration::from_secs(10))),
        )
        .await;
        match answered {
            Ok(Ok(reply)) => {
                if let Some(why) = differs(
                    &format!("{what}'s reply"),
                    &reply.to_string(),
                    step["reply"].as_str().unwrap_or_default(),
                ) {
                    self.problems.push(why);
                }
            }
            Ok(Err(error)) => self
                .problems
                .push(format!("{what} was not answered: {error}")),
            Err(_) => self
                .problems
                .push(format!("{what} was not answered in time")),
        }
        let kicks = u64::from(self.kicks.get());
        if step["kicks"] != kicks {
            self.problems.push(format!(
                "{what} woke the dispatcher {kicks} times, Node {}",
                step["kicks"]
            ));
        }
        self.rig.settle(&what, step);
        let logged = Value::Array(self.rig.logged());
        self.rig
            .compare(&format!("{what} logged"), &logged, &step["events"]);
        self.problems.extend(self.standin.leftover());
    }
}

/// What the test's own ledger said the last staff was, when the test gave the
/// operations a ledger that only knows that (`core-page.test.mjs:399`).
fn page_staff(steps: &[Value]) -> Option<Vec<Value>> {
    steps
        .iter()
        .filter(|step| step["kind"] == "operation")
        .flat_map(|step| step["seams"].as_array().into_iter().flatten())
        .find(|seam| seam["seam"] == "ledger" && seam["method"] == "lastStaff")
        .and_then(|seam| seam["result"].as_array().cloned())
}

/// A ledger of its own whose newest staff is `staff`, as it stood from before
/// an image designer had to be an image agent: a role no agent fits cannot be
/// given through the ledger today, so the rows are written as they were.
fn seeded(file: &Path, staff: &[Value]) -> Rig {
    let migrated = Rig::open(file);
    migrated.ledger.borrow_mut().close_in_place().unwrap();
    drop(migrated);
    let db = Connection::open(file).unwrap();
    let at = "2026-01-01T00:00:00.000Z";
    db.execute(
        "INSERT INTO project (directory, name, state, created_at, updated_at) VALUES ('/seed', 'seed', 'open', ?1, ?1)",
        [at],
    )
    .unwrap();
    for member in staff {
        let roles = &member["roles"];
        db.execute(
            "INSERT INTO participant (project_id, handle, role, roles, agent, harness, created_at)
             VALUES (1, ?1, ?2, ?3, ?1, 'claude-code', ?4)",
            (
                member["agent"].as_str().unwrap(),
                roles[0].as_str().unwrap(),
                roles.to_string(),
                at,
            ),
        )
        .unwrap();
    }
    drop(db);
    Rig::open(file)
}

//! A trace of the page's operations, played as `FORMAT.md` says: the world put
//! in place, the stand-ins set up from what each operation's `seams` say, the
//! operation asked over a bridge as the app asks it, and its reply (the bytes
//! the bridge carried), its kicks, the files it wrote and the events its own
//! ledger calls logged compared with Node's; the ledger left as Node left it.

use std::path::Path;
use std::rc::Rc;
use std::time::Duration;

use cf_daemon::host::daemon_bridge;
use cf_daemon::page::{self, Engine, Page};
use cf_daemon::seams::DaemonSpawn;
use rusqlite::Connection;
use serde_json::{json, Value};
use tokio::io::{duplex, split};
use tokio::task::JoinHandle;

use crate::ledger::Rig;
use crate::notes::Notes;
use crate::standin::Standin;
use crate::support::compare::{compare, differs};
use crate::support::daemon::{executor, Kicks};
use crate::support::trace::{self, Tally};
use crate::wire::Wire;
use crate::world::World;
use crate::wrote::{held, snapshot};

/// The daemon's end of a bridge with the page's operations on it, and the
/// app's end that asks them.
struct Connected {
    wire: Wire,
    daemon: JoinHandle<()>,
}

impl Drop for Connected {
    fn drop(&mut self) {
        self.daemon.abort();
    }
}

/// What a trace needs to be played.
struct Player {
    rig: Rc<Rig>,
    /// The ledger the operations read: the same one, but for a trace whose
    /// test gave the page a ledger of its own.
    page_ledger: Rc<Rig>,
    standin: Rc<Standin>,
    notes: Rc<Notes>,
    spawn: Rc<DaemonSpawn>,
    kicks: Kicks,
    world: World,
    connected: Option<Connected>,
    problems: Vec<String>,
    /// What has been compared so far.
    tally: Tally,
}

/// Plays the trace `name`: what it held of it, or why it was not answered as
/// Node's was.
pub fn play(name: &str) -> Result<Tally, Vec<String>> {
    trace::locally(replay(name))
}

async fn replay(name: &str) -> Result<Tally, Vec<String>> {
    // The daemon's log and trace, and the ledgers: the world is a folder of its own.
    let home = tempfile::tempdir().unwrap();
    let trace = trace::load(name);
    let steps = trace["steps"].as_array().cloned().unwrap_or_default();

    let (spawn, _, _) = executor(home.path());
    let rig = Rc::new(Rig::open(&home.path().join("consensflow.db")));
    let page_ledger = match page_staff(&steps) {
        Some(staff) => Rc::new(seeded(&home.path().join("page.db"), &staff)),
        None => Rc::clone(&rig),
    };
    let notes = Rc::new(Notes::default());
    let mut player = Player {
        standin: Rc::new(Standin::new(Rc::clone(&rig), Rc::clone(&notes))),
        rig,
        page_ledger,
        notes,
        spawn,
        kicks: Kicks::new(),
        world: World::new(),
        connected: None,
        problems: Vec::new(),
        tally: Tally::default(),
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
                match player.rig.close(step, &trace["ledger"]["final"]) {
                    Some(why) => player.problems.push(why),
                    None => player.tally.databases += 1,
                }
                closed = true;
            }
            "ledger" => player.problems.extend(player.rig.apply(step)),
            "operation" => player.operation(step).await,
            other => player.problems.push(format!(
                "a step of kind {other}, which a page trace has not"
            )),
        }
        let noted = player.notes.take();
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
    if !player.problems.is_empty() {
        return Err(player.problems);
    }
    player.tally.traces += 1;
    Ok(player.tally)
}

impl Player {
    /// The page, over the environment the world has now, asked on a bridge of
    /// its own.
    fn connect(&mut self) -> Connected {
        let engine: Rc<dyn Engine> = Rc::clone(&self.standin) as _;
        let page = Rc::new(Page {
            ledger: Rc::clone(&self.page_ledger.ledger),
            engine,
            env: self.world.env(),
            kick: self.kicks.waker(),
            // No trace of Node's opens a file: none is opened here either.
            specs: page::Specs::opening_with(Rc::new(|_, _| {
                Box::pin(async { Err("the trace player opens no file".to_owned()) })
            })),
            // No trace of Node's asks of computer use: a machine with none, which opens nothing.
            computer_use: page::ComputerUsePage {
                machine: Rc::new(cf_daemon::computer_use::MachineComputerUse::granting(
                    self.world.env(),
                    Rc::new(|| None),
                )),
                open: Rc::new(|_| {
                    Box::pin(async { Err("the trace player opens nothing".to_owned()) })
                }),
                grants_of: None,
            },
            // No trace of Node's asks for models: a page with no adapter to read them from.
            models: Rc::new(cf_daemon::models::ModelLists::new(
                Rc::new(NoAdapters),
                self.world.env(),
            )),
        });
        let (daemon_end, app_end) = duplex(256 * 1024);
        let (daemon_input, daemon_output) = split(daemon_end);
        let (app_input, app_output) = split(app_end);
        let (daemon, connection) = daemon_bridge(&self.spawn).connect(daemon_input, daemon_output);
        page::register(&daemon, &page, &self.spawn);
        // The daemon's end is kept alive by its connection.
        drop(daemon);
        Connected {
            wire: Wire::new(app_input, app_output),
            daemon: tokio::task::spawn_local(connection),
        }
    }

    /// The app's end of the bridge to the page.
    fn wire(&mut self) -> &mut Wire {
        if self.connected.is_none() {
            self.connected = Some(self.connect());
        }
        &mut self.connected.as_mut().expect("connected just now").wire
    }

    /// One operation, asked as the app asks it.
    async fn operation(&mut self, step: &Value) {
        let name = step["name"].as_str().unwrap_or_default().to_owned();
        let what = format!("operation {} ({name})", step["id"]);
        self.rig.give(step);
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
        self.kicks.take();
        let body = modelled(&name, step);
        let before = snapshot(&self.world);
        let answered =
            tokio::time::timeout(Duration::from_secs(10), self.wire().ask(&name, &body)).await;
        let after = snapshot(&self.world);
        match answered {
            Ok(Ok(reply)) => {
                let recorded = step["reply"].as_str().unwrap_or_default();
                let (ours, theirs) = (projected(reply.as_bytes()), projected(recorded.as_bytes()));
                if let Some(why) = differs(&format!("{what}'s reply"), ours, theirs) {
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
        self.problems.extend(
            held(step, &before, &after)
                .into_iter()
                .map(|why| format!("{what}: {why}")),
        );
        let kicks = self.kicks.take();
        if step["kicks"] != kicks {
            self.problems.push(format!(
                "{what} woke the dispatcher {kicks} times, Node {}",
                step["kicks"]
            ));
        }
        if let Some(why) = self.rig.settle(&[step]) {
            self.problems.push(format!("{what} {why}"));
        }
        let logged = Value::Array(self.rig.take_events());
        self.problems
            .extend(compare(&format!("{what} logged"), &logged, &step["events"]));
        self.problems.extend(self.standin.leftover());
        self.tally.operations += 1;
    }
}

/// The body of operation `name` as this daemon takes it (harness-models): Node's page named
/// a member by its ready-made agent (`{agent: 'zeus'}`), this one by its harness, a model
/// and the tier the human picks. A member Add member added, or New project started with,
/// is given what Node's ledger recorded of it (its harness and tier, in the reply, or in
/// the dispatcher's `openProject`), and the agent's name for its model, which names it as
/// Node's handle did; one Node refused before its ledger, which recorded none, is given
/// none, and says so in this daemon's words (a departure).
fn modelled(name: &str, step: &Value) -> Value {
    let mut body = step["body"].clone();
    match name {
        "member.add" => {
            let reply: Value = serde_json::from_str(step["reply"].as_str().unwrap_or_default())
                .unwrap_or_default();
            let member = &reply["member"];
            if let (Some(fields), Some(agent)) =
                (body.as_object_mut(), step["body"]["agent"].as_str())
            {
                fields.shift_remove("agent");
                fields.insert("model".to_owned(), json!(agent));
                for key in ["harness", "tier"] {
                    if !member[key].is_null() {
                        fields.insert(key.to_owned(), member[key].clone());
                    }
                }
            }
        }
        "project.open" => {
            // Node's page knew no compaction: a project it opened is on each harness's own
            // point, which this daemon reads from a request that says `null` (one that says
            // nothing of it starts on the default size, T-1400).
            if let Some(fields) = body.as_object_mut() {
                fields.entry("compaction").or_insert(Value::Null);
            }
            let opened = step["seams"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|seam| seam["method"] == "openProject")
                .map(|seam| seam["args"][0]["staff"].clone());
            if let (Some(Value::Array(staff)), Some(Value::Array(recorded))) =
                (body.get_mut("staff"), opened)
            {
                for member in staff.iter_mut() {
                    let Some(agent) = member["agent"].as_str().map(str::to_owned) else {
                        continue;
                    };
                    let kept = recorded.iter().find(|kept| kept["agent"] == agent.as_str());
                    if let Some(fields) = member.as_object_mut() {
                        fields.shift_remove("agent");
                        fields.insert("model".to_owned(), json!(agent));
                        if let Some(kept) = kept {
                            fields.insert("harness".to_owned(), kept["harness"].clone());
                            fields.insert("tier".to_owned(), kept["tier"].clone());
                        }
                    }
                }
            }
        }
        _ => {}
    }
    body
}

/// A reply as both sides are compared: what a participant runs on held apart
/// (`cf_ledger::testing::hold_apart_what_members_run_on`), the marks of a lane whose
/// saved agent was gone, which Node's board said and this one's has none of, and the roles
/// `board.get` answers beside its board, which Node's had no table of (`cf_ledger::roles`;
/// the daemon's own test `page::tests::roles` holds them).
fn projected(reply: &[u8]) -> String {
    let Ok(mut value) = serde_json::from_slice::<Value>(reply) else {
        return String::from_utf8_lossy(reply).into_owned();
    };
    cf_ledger::testing::hold_apart_what_members_run_on(&mut value);
    if let Some(fields) = value.as_object_mut() {
        if fields.contains_key("board") {
            fields.shift_remove("roles");
        }
    }
    if let Some(Value::Array(lanes)) = value
        .get_mut("board")
        .and_then(|board| board.get_mut("lanes"))
    {
        for lane in lanes {
            if let Some(fields) = lane.as_object_mut() {
                fields.shift_remove("agentMissing");
                fields.shift_remove("agentRetired");
            }
        }
    }
    value.to_string()
}

/// What the test's own ledger said the last staff was, when the test gave the
/// operations a ledger that only knows that.
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

/// The adapters of a page that reads no model: none.
struct NoAdapters;

impl cf_engine::seams::Adapters for NoAdapters {
    fn adapter(&self, _harness: &str) -> Option<Rc<dyn cf_harness::contract::Adapter>> {
        None
    }
}

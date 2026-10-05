//! The scenarios of `tests/goldens/launch/scenarios.<platform>.json`, played
//! against the Rust adapters step by step as `tests/goldens/launch/runner.mjs`
//! played them against Node's, each step's record held to Node's. The
//! runner's comment says what a step is, how its record is written so that
//! it is the same on every run, and what a step's `kept` says: a difference
//! Rust keeps on purpose, where Rust's settlement is held to it and must
//! still differ from Node's.
//!
//! Every wait an adapter makes is on a fake of `cf_harness::testing`: the
//! clock moves only when a step advances it, a look or a host request a
//! step holds waits until a step releases it, and the work begun is run by
//! hand until nothing moves.

use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use cf_base::env::Env;
use cf_base::path;
use cf_harness::claude::ClaudeAdapter;
use cf_harness::contract::{
    Adapter, Admission, Agent, HostError, Launch, LaunchId, Observed, Pane, Readiness, Window,
};
use cf_harness::forget_launch;
use cf_harness::seams::{Services, Time};
use cf_harness::testing::{fake_executable, Answer, Driver, Fakes, OtherProcess, ScriptedHost};
use serde_json::{json, Map, Value};
use tempfile::TempDir;

/// A process id no process has (`DEAD`, runner.mjs).
const DEAD: u32 = 999_999;

/// The platform's name as Node's `process.platform` says it.
fn platform() -> &'static str {
    if cfg!(windows) {
        "win32"
    } else if cfg!(target_os = "macos") {
        "darwin"
    } else {
        "linux"
    }
}

/// How a scenario writes what is particular to its run: its root, and its
/// processes.
struct Names {
    root: String,
    other: Option<u32>,
}

impl Names {
    /// The process ids a scenario names, by their names.
    fn pids(&self) -> Vec<(&'static str, u32)> {
        let mut pids = vec![("$PID", std::process::id()), ("$DEAD", DEAD)];
        pids.extend(self.other.map(|other| ("$OTHER", other)));
        pids
    }

    /// `$ROOT/a/b` as a path under the root, a named process as its id
    /// (`real`).
    fn real(&self, value: &Value) -> Value {
        match value {
            Value::Array(items) => items.iter().map(|item| self.real(item)).collect(),
            Value::Object(fields) => Value::Object(
                fields
                    .iter()
                    .map(|(key, item)| (key.clone(), self.real(item)))
                    .collect(),
            ),
            Value::String(text) => {
                if let Some((_, pid)) = self.pids().into_iter().find(|(name, _)| name == text) {
                    return json!(pid);
                }
                match text.strip_prefix("$ROOT") {
                    Some(rest) => {
                        let mut parts = vec![self.root.as_str()];
                        parts.extend(rest.split('/').filter(|part| !part.is_empty()));
                        json!(path::join(&parts))
                    }
                    None => json!(text),
                }
            }
            other => other.clone(),
        }
    }

    /// A file's text with the named processes in it (`realText`).
    fn real_text(&self, text: &str) -> String {
        let mut filled = text.to_owned();
        for (name, pid) in self.pids() {
            filled = filled.replace(name, &pid.to_string());
        }
        filled
    }

    /// What a step recorded, with the root and the live processes written
    /// as the scenario writes them (`written`).
    fn written(&self, value: &Value) -> Value {
        match value {
            Value::Array(items) => items.iter().map(|item| self.written(item)).collect(),
            Value::Object(fields) => Value::Object(
                fields
                    .iter()
                    .map(|(key, item)| (key.clone(), self.written(item)))
                    .collect(),
            ),
            Value::Number(number) => {
                let live = self.pids().into_iter().find(|&(name, pid)| {
                    name != "$DEAD" && number.as_u64() == Some(u64::from(pid))
                });
                live.map_or_else(|| value.clone(), |(name, _)| json!(name))
            }
            Value::String(text) => {
                let mut text = text.replace(&self.root, "$ROOT");
                if text.starts_with("$ROOT") {
                    text = text.replace('\\', "/");
                }
                json!(text)
            }
            other => other.clone(),
        }
    }
}

/// What a begun piece of work settles with: its record, and the window a
/// prepare opens.
type Settled = (Value, Option<Rc<dyn Window>>);

/// A scenario being played.
struct Played {
    _dir: TempDir,
    _other: Option<OtherProcess>,
    names: Names,
    env: Map<String, Value>,
    fakes: Fakes,
    host: Rc<ScriptedHost>,
    adapter: Rc<dyn Adapter>,
    window: Option<Rc<dyn Window>>,
    launch: Option<LaunchId>,
    driver: Driver<Settled>,
}

/// A scenario's answer to a host request: at once, failing, or held.
fn answer(names: &Names, given: &Value) -> Answer {
    if given.get("held") == Some(&json!(true)) {
        return Answer::Held;
    }
    Answer::Now(response(names, given))
}

/// A host's response as a scenario writes it: `{throws: message}` for a
/// request that fails.
fn response(names: &Names, given: &Value) -> Result<Value, HostError> {
    match given.get("throws") {
        Some(thrown) => Err(HostError {
            error: given
                .get("error")
                .and_then(Value::as_str)
                .map(str::to_owned),
            message: thrown.as_str().unwrap().to_owned(),
        }),
        None => Ok(names.real(given)),
    }
}

/// Every file, folder and link under `root`, by its path there: a file's
/// and a folder's mode (none on Windows), a file's text, a link's target,
/// never followed.
fn tree(root: &Path) -> Vec<(String, Value)> {
    fn walk(root: &Path, folder: &Path, found: &mut Vec<(String, Value)>) {
        for entry in fs::read_dir(folder).unwrap() {
            let full = entry.unwrap().path();
            let relative = full
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            if fs::symlink_metadata(&full).unwrap().is_symlink() {
                let target = fs::read_link(&full).unwrap();
                found.push((relative, json!({ "link": target.to_string_lossy() })));
                continue;
            }
            let metadata = fs::metadata(&full).unwrap();
            #[cfg(unix)]
            let mode =
                json!(std::os::unix::fs::PermissionsExt::mode(&metadata.permissions()) & 0o777);
            #[cfg(not(unix))]
            let mode = Value::Null;
            if metadata.is_dir() {
                found.push((relative, json!({ "mode": mode })));
                walk(root, &full, found);
            } else {
                let text = String::from_utf8_lossy(&fs::read(&full).unwrap()).into_owned();
                found.push((relative, json!({ "mode": mode, "text": text })));
            }
        }
    }
    let mut found = Vec::new();
    walk(root, root, &mut found);
    found
}

/// What a step did to the tree: each path made, changed or removed, in
/// JavaScript's order of their texts.
fn changes(before: &[(String, Value)], after: &[(String, Value)]) -> Value {
    let find = |entries: &[(String, Value)], relative: &str| {
        entries
            .iter()
            .find(|(held, _)| held == relative)
            .map(|(_, entry)| entry.clone())
    };
    let mut paths: Vec<&String> = before.iter().chain(after).map(|(path, _)| path).collect();
    paths.sort_by(|left, right| left.encode_utf16().cmp(right.encode_utf16()));
    paths.dedup();
    paths
        .into_iter()
        .filter_map(|relative| {
            let mut fields = Map::new();
            fields.insert("path".to_owned(), json!(format!("$ROOT/{relative}")));
            match (find(before, relative), find(after, relative)) {
                (_, None) => {
                    fields.insert("removed".to_owned(), json!(true));
                }
                (Some(was), Some(entry)) if was == entry => return None,
                (_, Some(entry)) => fields.extend(entry.as_object().unwrap().clone()),
            }
            Some(Value::Object(fields))
        })
        .collect()
}

/// What a look found, as Node's adapter answered it.
fn observed(observed: &Observed) -> Value {
    let mut fields = Map::new();
    fields.insert("items".to_owned(), json!(observed.items()));
    fields.insert("settled".to_owned(), json!(observed.settled));
    let waiting = observed
        .waiting
        .as_ref()
        .map_or(Value::Null, |waiting| json!({ "reason": waiting.reason }));
    fields.insert("waiting".to_owned(), waiting);
    fields.insert("failed".to_owned(), json!(observed.failed));
    fields.insert("quota".to_owned(), json!(observed.quota));
    if let Some(session) = &observed.switched {
        fields.insert("switched".to_owned(), json!({ "nativeSession": session }));
    }
    if observed.unnamed {
        fields.insert("unnamed".to_owned(), json!(true));
    }
    Value::Object(fields)
}

/// Whether a window is ready, as Node's adapter answered it: `true`, or why not.
fn readiness(readiness: &Readiness) -> Value {
    match readiness {
        Readiness::Ready => json!(true),
        Readiness::Held(held) => json!(held.sentence()),
    }
}

/// What became of a delivery, as Node's `admission` wrote it.
fn admission(admission: &Admission) -> Value {
    match admission {
        Admission::Admitted { queued: false } => json!({ "admitted": true }),
        Admission::Admitted { queued: true } => json!({ "admitted": true, "queued": true }),
        Admission::Refused { reason } => json!({ "admitted": false, "reason": reason }),
        Admission::Uncertain { reason } => json!({ "admitted": null, "reason": reason }),
    }
}

/// A settlement as Node's runner writes one: what the work answered, or the
/// sentence it threw.
fn settled<T>(outcome: Result<T, String>, written: impl FnOnce(T) -> Value) -> Value {
    match outcome {
        Ok(value) => json!({ "answer": written(value) }),
        Err(thrown) => json!({ "throws": thrown }),
    }
}

/// A text field of a step, none for null and for a field not there.
fn text(value: &Value) -> Option<String> {
    value.as_str().map(str::to_owned)
}

/// Begins a prepare of the launch `given` names.
fn begin_prepare(played: &mut Played, id: usize, given: &Value) {
    let launch_id = LaunchId::new(&text(&given["launchId"]).unwrap()).expect("a launch id");
    played.launch = Some(launch_id.clone());
    let adapter = Rc::clone(&played.adapter);
    let agent = given["agent"].clone();
    let (project, handle) = (
        given["participant"]["projectId"].as_i64().unwrap(),
        text(&given["participant"]["handle"]).unwrap(),
    );
    let role = text(&given["role"]).unwrap();
    let directory = text(&given["directory"]).unwrap();
    let (resume, message) = (text(&given["resume"]), text(&given["message"]));
    let instructions = text(&given["instructions"]).unwrap();
    played.driver.begin(id, async move {
        let agent = agent.as_object().map(|agent| Agent {
            model: agent.get("model").and_then(Value::as_str),
            effort: agent.get("effort").and_then(Value::as_str),
            thinking: agent.get("thinking").and_then(Value::as_str),
            designer: agent.get("designer") == Some(&json!(true)),
        });
        let launch = Launch {
            id: &launch_id,
            project,
            handle: &handle,
            role: &role,
            directory: &directory,
            resume: resume.as_deref(),
            message: message.as_deref(),
            agent,
            instructions: &instructions,
        };
        match adapter.prepare(&launch).await {
            Ok(prepared) => {
                let record = json!({ "answer": {
                    "argv": prepared.argv,
                    "env": prepared.env,
                    "dropEnv": prepared.drop_env,
                    "nativeSession": prepared.native_session,
                }});
                (record, Some(prepared.window))
            }
            Err(refused) => (json!({ "throws": refused }), None),
        }
    });
}

/// Begins what a step asks the window.
fn begin_asking(played: &mut Played, id: usize, step: &Value) {
    for (op, answers) in step["answers"].as_object().into_iter().flatten() {
        let answers: Vec<Answer> = answers
            .as_array()
            .unwrap()
            .iter()
            .map(|given| answer(&played.names, given))
            .collect();
        played.host.answer(op, answers);
    }
    let window = Rc::clone(played.window.as_ref().expect("a window prepared"));
    let host = Rc::clone(&played.host);
    let pane = Pane {
        id: "p1-zeus".to_owned(),
        generation: 1,
    };
    if step.get("observe").is_some() {
        played.driver.begin(id, async move {
            (
                settled(window.observe().await, |found| observed(&found)),
                None,
            )
        });
    } else if step.get("ready").is_some() {
        played.driver.begin(id, async move {
            let ready = window.ready(&*host, &pane).await;
            (settled(ready, |ready| readiness(&ready)), None)
        });
    } else if let Some(text) = text(&step["deliver"]) {
        played.driver.begin(id, async move {
            let outcome = window.deliver(&*host, &pane, &text).await;
            (settled(outcome, |outcome| admission(&outcome)), None)
        });
    } else if step.get("started").is_some() {
        played.driver.begin(id, async move {
            let started = window.started().await;
            (settled(started, |session| json!(session)), None)
        });
    } else {
        panic!("a step of no kind: {step}");
    }
}

/// Runs the work begun until nothing moves, the clock moved by `advance`
/// first, a timer at a time: what settled.
fn run(played: &mut Played, advance: Option<i64>) -> Vec<Value> {
    let mut settled = played.driver.run();
    if let Some(millis) = advance {
        let until = played.fakes.time.wall_ms() + millis;
        while played.fakes.time.fire_next(until) {
            settled.extend(played.driver.run());
        }
        played.fakes.time.settle_at(until);
    }
    // In the order the work was begun, as Node's runner writes it.
    settled.sort_by_key(|(op, _)| *op);
    settled
        .into_iter()
        .map(|(op, (mut record, window))| {
            if let Some(window) = window {
                played.window = Some(window);
            }
            record["op"] = json!(op);
            record
        })
        .collect()
}

/// The work still waiting, each with what it waits on: its timers (how long
/// until each is due), its held requests, its held looks. Work waiting on
/// nothing a step controls waits on what no step can release: a defect of
/// the adapter or the scenario, said at once.
fn pending(played: &Played) -> Vec<Value> {
    played
        .driver
        .pending()
        .into_iter()
        .map(|op| {
            let mut waits: Vec<Value> = played
                .fakes
                .time
                .waits(op)
                .into_iter()
                .map(|millis| json!({ "timer": millis }))
                .collect();
            waits.extend(
                played
                    .host
                    .waits(op)
                    .into_iter()
                    .map(|request| json!({ "request": request })),
            );
            waits.extend((0..played.fakes.records.waits(op)).map(|_| json!({ "look": true })));
            assert!(
                !waits.is_empty(),
                "work {op} waits on nothing a step controls"
            );
            json!({ "op": op, "waits": waits })
        })
        .collect()
}

/// Sets the root up as a step says: false for a step that is recorded.
fn set_up(played: &mut Played, step: &Value) -> bool {
    let names = &played.names;
    if let Some(name) = step["executable"].as_str() {
        fake_executable(Path::new(&path::join(&[&names.root, "bin", name])));
        return true;
    }
    if step.get("write").is_some() {
        let file = names.real(&step["write"]);
        let file = Path::new(file.as_str().unwrap());
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, names.real(&step["text"]).as_str().unwrap()).unwrap();
        return true;
    }
    if step.get("remove").is_some() {
        let file = names.real(&step["remove"]);
        let _ = fs::remove_file(file.as_str().unwrap());
        return true;
    }
    if step.get("status").is_some() || step.get("statusText").is_some() {
        // Claude's own status of a live process (`sessions/<pid>.json`), or
        // a file of its by another name, or its text where JSON cannot hold it.
        let folder = path::join(&[
            played.env["CLAUDE_CONFIG_DIR"].as_str().unwrap(),
            "sessions",
        ]);
        fs::create_dir_all(&folder).unwrap();
        if let Some(text) = step["statusText"].as_str() {
            let file = path::join(&[&folder, step["file"].as_str().unwrap()]);
            fs::write(file, names.real_text(text)).unwrap();
            return true;
        }
        let given = names.real(&step["status"]);
        let pid = given["pid"].clone();
        let mut row = Map::new();
        row.insert("pid".to_owned(), pid.clone());
        row.extend(
            given
                .as_object()
                .unwrap()
                .iter()
                .filter(|(key, _)| *key != "pid")
                .map(|(key, value)| (key.clone(), value.clone())),
        );
        let named = step["file"]
            .as_str()
            .map_or_else(|| format!("{pid}.json"), str::to_owned);
        let file = path::join(&[&folder, &named]);
        fs::write(file, serde_json::to_string(&Value::Object(row)).unwrap()).unwrap();
        return true;
    }
    if step.get("opened").is_some() {
        // What the engine tells a window once its pane opened.
        let pid = names.real(&step["opened"])["pid"].as_u64();
        let window = played.window.as_ref().expect("a window prepared");
        window.opened(pid.map(|pid| u32::try_from(pid).unwrap()));
        return true;
    }
    if step.get("follow").is_some() {
        let session = names.real(&step["follow"]);
        let window = played.window.as_ref().expect("a window prepared");
        window.follow(session.as_str().unwrap());
        return true;
    }
    if let Some(hold) = step.get("holdLooks") {
        played.fakes.records.hold(hold == &json!(true));
        return true;
    }
    false
}

/// A recorded step, played: its record.
fn record(played: &mut Played, index: usize, step: &Value) -> Value {
    let root = PathBuf::from(&played.names.root);
    let before = tree(&root);
    let mut advance = None;
    if step.get("prepare").is_some() {
        let given = played.names.real(&step["prepare"]);
        begin_prepare(played, index, &given);
    } else if let Some(op) = step["release"].as_str() {
        let released = if op == "look" {
            played.fakes.records.release()
        } else {
            let given = response(&played.names, &step["answer"]);
            played.host.release(op, given)
        };
        assert!(released, "{op}: nothing held to release");
    } else if let Some(millis) = step["advance"].as_i64() {
        advance = Some(millis);
    } else if step.get("close").is_some() {
        // The engine closes the window: its launch's files go, and what it
        // still waits on keeps its own hold of the window.
        let launch = played.launch.as_ref().expect("a launch prepared");
        let home = played.env["CONSENSFLOW_HOME"].as_str().unwrap();
        forget_launch(home, launch).unwrap();
        played.window = None;
    } else {
        begin_asking(played, index, step);
    }
    let settled = run(played, advance);
    let mut fields = Map::new();
    fields.insert("step".to_owned(), json!(index));
    fields.insert("settled".to_owned(), json!(settled));
    fields.insert("pending".to_owned(), json!(pending(played)));
    let asked: Vec<Value> = played
        .host
        .take_asked()
        .into_iter()
        .map(|(op, body)| json!({ "op": op, "body": body }))
        .collect();
    fields.insert("requests".to_owned(), json!(asked));
    let unused = played.host.unused();
    let optional = step["optional"].as_array().cloned().unwrap_or_default();
    let unused: Vec<String> = unused
        .into_iter()
        .filter(|op| !optional.contains(&json!(op)))
        .collect();
    if !unused.is_empty() {
        fields.insert("unused".to_owned(), json!(unused));
    }
    fields.insert("draws".to_owned(), json!(played.fakes.entropy.take_draws()));
    fields.insert("tree".to_owned(), changes(&before, &tree(&root)));
    played.names.written(&Value::Object(fields))
}

/// The adapter a scenario plays against.
fn adapter(harness: &str, services: &Services) -> Rc<dyn Adapter> {
    match harness {
        "claude-code" => Rc::new(ClaudeAdapter::new(services)),
        other => panic!("no adapter for {other}"),
    }
}

/// Plays `scenario` in a root of its own: each recorded step's record.
fn play(scenario: &Value) -> Vec<Value> {
    let dir = tempfile::Builder::new()
        .prefix("cf-launch-golden-")
        .tempdir()
        .unwrap();
    fs::create_dir(dir.path().join("bin")).unwrap();
    let other = scenario["steps"]
        .to_string()
        .contains("$OTHER")
        .then(OtherProcess::start);
    let names = Names {
        root: dir.path().to_string_lossy().into_owned(),
        other: other.as_ref().map(OtherProcess::pid),
    };
    let env = names.real(&scenario["env"]).as_object().unwrap().clone();
    let vars = env
        .iter()
        .map(|(name, value)| (name.clone(), value.as_str().unwrap().to_owned()));
    let env_vars = Env::from_vars(vars);
    let fakes = Fakes::new(&env_vars);
    let services = fakes.services(&env_vars, dir.path());
    let adapter = adapter(scenario["harness"].as_str().unwrap(), &services);
    let mut played = Played {
        _dir: dir,
        _other: other,
        names,
        env,
        fakes,
        host: Rc::new(ScriptedHost::default()),
        adapter,
        window: None,
        launch: None,
        driver: Driver::default(),
    };
    let mut records = Vec::new();
    for (index, each) in scenario["steps"].as_array().unwrap().iter().enumerate() {
        if !set_up(&mut played, each) {
            records.push(record(&mut played, index, each));
        }
    }
    records
}

/// Whether Rust's `record` of a step is what it should be: Node's, but
/// where the step keeps a difference in how its own work settles, which
/// must still be one.
fn differs(step: &Value, index: u64, record: &Value, node: &Value) -> Option<String> {
    let Some(kept) = step.get("kept") else {
        return (record != node).then(|| format!("\n  rust {record}\n  node {node}"));
    };
    let own = |record: &Value| {
        record["settled"]
            .as_array()
            .and_then(|settled| settled.iter().find(|entry| entry["op"] == json!(index)))
            .cloned()
    };
    let (Some(rust), Some(node_own)) = (own(record), own(node)) else {
        return Some(format!(
            " keeps a difference in a step that did not settle: {record}"
        ));
    };
    let mut held = kept["answer"].clone();
    held["op"] = json!(index);
    if rust != held {
        return Some(format!(" (kept: {}):\n  rust {rust}", kept["why"]));
    }
    if rust == node_own {
        return Some(format!(" keeps a difference that is none: {}", kept["why"]));
    }
    let without = |record: &Value| {
        let mut record = record.clone();
        record["settled"] = json!(record["settled"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|entry| entry["op"] != json!(index))
            .cloned()
            .collect::<Vec<_>>());
        record
    };
    let (rest, node_rest) = (without(record), without(node));
    (rest != node_rest).then(|| format!("\n  rust {rest}\n  node {node_rest}"))
}

#[test]
fn every_scenario_plays_as_node_played_it() {
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
        "tests/goldens/launch/scenarios.{}.json",
        platform()
    ));
    let text = fs::read_to_string(&file).unwrap_or_else(|_| {
        panic!(
            "{}: npm run goldens:launch on this platform",
            file.display()
        )
    });
    let scenarios: Vec<Value> = serde_json::from_str(&text).unwrap();
    assert!(!scenarios.is_empty());
    let mut differ = Vec::new();
    for scenario in &scenarios {
        let name = scenario["name"].as_str().unwrap();
        let records = play(scenario);
        let expected = scenario["records"].as_array().unwrap();
        if records.len() != expected.len() {
            differ.push(format!(
                "{name}: {} records, Node {}",
                records.len(),
                expected.len()
            ));
        }
        for (record, node) in records.iter().zip(expected) {
            let index = node["step"].as_u64().unwrap();
            let step = &scenario["steps"][usize::try_from(index).unwrap()];
            if let Some(how) = differs(step, index, record, node) {
                differ.push(format!("{name}, step {index}{how}"));
            }
        }
    }
    assert!(
        differ.is_empty(),
        "{} differ:\n{}",
        differ.len(),
        differ.join("\n")
    );
}

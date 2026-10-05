//! The scenarios of `tests/goldens/launch/scenarios.<platform>.json`, played
//! against the Rust adapters step by step as `tests/goldens/launch/runner.mjs`
//! played them against Node's, each step's answer held to Node's. The
//! runner's comment says how a step's answer is written so that it is the
//! same on every run: `$ROOT`, the values a step drew, `$PID`.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::fs;
use std::path::Path;
use std::rc::Rc;

use cf_base::env::Env;
use cf_base::path;
use cf_harness::claude::ClaudeAdapter;
use cf_harness::contract::{
    Adapter, Admission, Agent, HostError, Launch, LaunchId, Observed, Pane, PaneHost, Readiness,
    Window, Work,
};
use serde_json::{json, Map, Value};
use tempfile::TempDir;

use crate::fakes::{done, fake_executable, Local};

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

/// How a scenario writes what is particular to its run: its root, and the
/// values its steps drew, by the names it gives them.
struct Names {
    root: String,
    named: Vec<(String, String)>,
}

impl Names {
    /// `$ROOT/a/b` as a path under the root, `$PID` and `$DEAD` as process
    /// ids, the named values as drawn (`real`).
    fn real(&self, value: &Value) -> Value {
        match value {
            Value::Array(items) => items.iter().map(|item| self.real(item)).collect(),
            Value::Object(fields) => Value::Object(
                fields
                    .iter()
                    .map(|(key, item)| (key.clone(), self.real(item)))
                    .collect(),
            ),
            Value::String(text) if text == "$PID" => json!(std::process::id()),
            Value::String(text) if text == "$DEAD" => json!(DEAD),
            Value::String(text) => {
                let mut text = text.clone();
                for (name, drawn) in &self.named {
                    text = text.replace(name.as_str(), drawn);
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

    /// What a step answered, with the root, the named values and this
    /// process written as the scenario writes them (`written`).
    fn written(&self, value: &Value) -> Value {
        match value {
            Value::Array(items) => items.iter().map(|item| self.written(item)).collect(),
            Value::Object(fields) => Value::Object(
                fields
                    .iter()
                    .map(|(key, item)| (key.clone(), self.written(item)))
                    .collect(),
            ),
            Value::Number(number) if number.as_u64() == Some(u64::from(std::process::id())) => {
                json!("$PID")
            }
            Value::String(text) => {
                let mut text = text.clone();
                for (name, drawn) in &self.named {
                    text = text.replace(drawn.as_str(), name);
                }
                text = text.replace(&self.root, "$ROOT");
                if text.starts_with("$ROOT") {
                    text = text.replace('\\', "/");
                }
                json!(text)
            }
            other => other.clone(),
        }
    }
}

/// A scenario being played: its root and names, the environment and the
/// adapter it plays against, and the window it prepared.
struct Played {
    _dir: TempDir,
    names: Names,
    env: Map<String, Value>,
    adapter: Box<dyn Adapter>,
    window: Option<Rc<dyn Window>>,
}

impl Played {
    fn window(&self) -> &dyn Window {
        self.window.as_deref().expect("a window prepared")
    }
}

/// A pane host that answers each request as the scenario says, the next
/// answer for its operation each time (`{throws: message}` for one that
/// fails), and writes down what it was asked.
struct Scripted<'p> {
    names: &'p Names,
    answers: RefCell<HashMap<String, VecDeque<Value>>>,
    requests: RefCell<Vec<Value>>,
}

impl PaneHost for Scripted<'_> {
    fn request<'a>(&'a self, op: &'a str, body: Value) -> Work<'a, Result<Value, HostError>> {
        Box::pin(async move {
            let asked = self.names.written(&json!({ "op": op, "body": body }));
            self.requests.borrow_mut().push(asked);
            let answer = self
                .answers
                .borrow_mut()
                .get_mut(op)
                .and_then(VecDeque::pop_front);
            let Some(answer) = answer else {
                return Err(HostError {
                    error: None,
                    message: format!("no answer for {op}"),
                });
            };
            match answer.get("throws") {
                Some(thrown) => Err(HostError {
                    error: answer
                        .get("error")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                    message: thrown.as_str().unwrap().to_owned(),
                }),
                None => Ok(self.names.real(&answer)),
            }
        })
    }
}

/// Every file and folder under `root`, by its path there, with its mode
/// (none on Windows) and a file's text.
fn tree(root: &Path) -> Vec<(String, Value)> {
    fn walk(root: &Path, folder: &Path, found: &mut Vec<(String, Value)>) {
        for entry in fs::read_dir(folder).unwrap() {
            let full = entry.unwrap().path();
            let relative = full
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
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

/// What a step changed of the tree: each path made or changed, in
/// JavaScript's order of their texts.
fn changes(before: &[(String, Value)], after: Vec<(String, Value)>) -> Value {
    let mut made: Vec<(String, Value)> = after
        .into_iter()
        .filter(|(relative, entry)| {
            before
                .iter()
                .find(|(held, _)| held == relative)
                .is_none_or(|(_, was)| was != entry)
        })
        .collect();
    made.sort_by(|(left, _), (right, _)| left.encode_utf16().cmp(right.encode_utf16()));
    made.into_iter()
        .map(|(relative, entry)| {
            let mut fields = Map::new();
            fields.insert("path".to_owned(), json!(format!("$ROOT/{relative}")));
            fields.extend(entry.as_object().unwrap().clone());
            Value::Object(fields)
        })
        .collect()
}

/// A text field of a step, none for null and for a field not there.
fn text(value: &Value) -> Option<&str> {
    value.as_str()
}

/// The launch a prepare step names, its participant's fields as the engine
/// gives them.
fn prepare(played: &mut Played, step: &Value) -> Value {
    let given = played.names.real(&step["prepare"]);
    let id = LaunchId::new(text(&given["launchId"]).unwrap()).expect("a launch id");
    let agent = given["agent"].as_object().map(|agent| Agent {
        model: agent.get("model").and_then(Value::as_str),
        effort: agent.get("effort").and_then(Value::as_str),
        thinking: agent.get("thinking").and_then(Value::as_str),
    });
    let launch = Launch {
        id: &id,
        project: given["participant"]["projectId"].as_i64().unwrap(),
        handle: text(&given["participant"]["handle"]).unwrap(),
        role: text(&given["role"]).unwrap(),
        directory: text(&given["directory"]).unwrap(),
        resume: text(&given["resume"]),
        message: text(&given["message"]),
        agent,
        instructions: text(&given["instructions"]).unwrap(),
    };
    let root = Path::new(&played.names.root).to_path_buf();
    let before = tree(&root);
    let mut answer = match done(played.adapter.prepare(&launch)) {
        Ok(prepared) => {
            let env: Map<String, Value> = prepared
                .env
                .iter()
                .map(|(name, value)| (name.clone(), json!(value)))
                .collect();
            let answer = json!({
                "argv": prepared.argv,
                "env": env,
                "dropEnv": prepared.drop_env,
                "nativeSession": prepared.native_session,
            });
            for (name, field) in step["draws"].as_object().into_iter().flatten() {
                assert_eq!(field, "nativeSession", "a value a prepare draws");
                let drawn = prepared.native_session.clone().expect("a session drawn");
                played.names.named.push((name.clone(), drawn));
            }
            played.window = Some(prepared.window);
            answer
        }
        Err(refused) => json!({ "refused": refused }),
    };
    answer["tree"] = changes(&before, tree(&root));
    played.names.written(&answer)
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

/// A step that asks the window something, through a host scripted with the
/// step's answers.
fn ask(played: &Played, step: &Value) -> Value {
    let answers = step["answers"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(op, list)| {
            (
                op.clone(),
                list.as_array().unwrap().iter().cloned().collect(),
            )
        })
        .collect();
    let host = Scripted {
        names: &played.names,
        answers: RefCell::new(answers),
        requests: RefCell::new(Vec::new()),
    };
    let pane = Pane {
        id: "p1-zeus".to_owned(),
        generation: 1,
    };
    let window = played.window();
    let answer = if step.get("observe").is_some() {
        match done(window.observe()) {
            Ok(found) => json!({ "answer": observed(&found) }),
            Err(thrown) => json!({ "throws": thrown }),
        }
    } else if step.get("ready").is_some() {
        match done(window.ready(&host, &pane)) {
            Ok(ready) => json!({ "answer": readiness(&ready) }),
            Err(thrown) => json!({ "throws": thrown.message }),
        }
    } else if let Some(text) = step["deliver"].as_str() {
        json!({ "answer": admission(&done(window.deliver(&host, &pane, text))) })
    } else {
        panic!("a step of no kind: {step}");
    };
    let mut answer = played.names.written(&answer);
    answer["requests"] = json!(host.requests.into_inner());
    answer
}

/// One step, played: what it answered, written as the scenario writes it.
fn step(played: &mut Played, step: &Value) -> Option<Value> {
    if let Some(name) = step["executable"].as_str() {
        fake_executable(Path::new(&path::join(&[&played.names.root, "bin", name])));
        return None;
    }
    if step.get("write").is_some() {
        let file = played.names.real(&step["write"]);
        let file = Path::new(file.as_str().unwrap());
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, played.names.real(&step["text"]).as_str().unwrap()).unwrap();
        return None;
    }
    if step.get("status").is_some() {
        // Claude's own status of a live process (`sessions/<pid>.json`).
        let given = played.names.real(&step["status"]);
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
        let folder = path::join(&[
            played.env["CLAUDE_CONFIG_DIR"].as_str().unwrap(),
            "sessions",
        ]);
        fs::create_dir_all(&folder).unwrap();
        let file = path::join(&[&folder, &format!("{pid}.json")]);
        fs::write(file, serde_json::to_string(&Value::Object(row)).unwrap()).unwrap();
        return None;
    }
    if step.get("prepare").is_some() {
        return Some(prepare(played, step));
    }
    if step.get("opened").is_some() {
        // What the engine tells a window once its pane opened.
        let pid = played.names.real(&step["opened"])["pid"].as_u64();
        played
            .window()
            .opened(pid.map(|pid| u32::try_from(pid).unwrap()));
        return None;
    }
    if step.get("follow").is_some() {
        let session = played.names.real(&step["follow"]);
        played.window().follow(session.as_str().unwrap());
        return None;
    }
    Some(ask(played, step))
}

/// The adapter a scenario plays against.
fn adapter(harness: &str, env: Env) -> Box<dyn Adapter> {
    let records = Rc::new(Local::new(env.clone()));
    match harness {
        "claude-code" => Box::new(ClaudeAdapter::new(env, records)),
        other => panic!("no adapter for {other}"),
    }
}

/// Plays `scenario` in a root of its own: what each step answered, by its index.
fn play(scenario: &Value) -> Vec<Value> {
    let dir = tempfile::Builder::new()
        .prefix("cf-launch-golden-")
        .tempdir()
        .unwrap();
    fs::create_dir(dir.path().join("bin")).unwrap();
    let names = Names {
        root: dir.path().to_string_lossy().into_owned(),
        named: Vec::new(),
    };
    let env = names.real(&scenario["env"]).as_object().unwrap().clone();
    let vars = env
        .iter()
        .map(|(name, value)| (name.clone(), value.as_str().unwrap().to_owned()));
    let adapter = adapter(scenario["harness"].as_str().unwrap(), Env::from_vars(vars));
    let mut played = Played {
        _dir: dir,
        names,
        env,
        adapter,
        window: None,
    };
    let mut answers = Vec::new();
    for (index, each) in scenario["steps"].as_array().unwrap().iter().enumerate() {
        if let Some(answer) = step(&mut played, each) {
            let mut fields = Map::new();
            fields.insert("step".to_owned(), json!(index));
            fields.extend(answer.as_object().unwrap().clone());
            answers.push(Value::Object(fields));
        }
    }
    answers
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
        let answers = play(scenario);
        let expected = scenario["answers"].as_array().unwrap();
        if answers.len() != expected.len() {
            differ.push(format!(
                "{name}: {} answers, Node {}",
                answers.len(),
                expected.len()
            ));
        }
        for (answer, node) in answers.iter().zip(expected) {
            if answer != node {
                differ.push(format!(
                    "{name}, step {}:\n  rust {answer}\n  node {node}",
                    node["step"]
                ));
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

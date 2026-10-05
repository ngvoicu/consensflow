//! The scenarios of `tests/goldens/launch/scenarios.<platform>.json`, played
//! against the Rust adapters step by step as `tests/goldens/launch/runner.mjs`
//! played them against Node's, each step's answer held to Node's. The
//! runner's comment says how a step's answer is written so that it is the
//! same on every run (`$ROOT`, the values a step drew, the named processes),
//! and what a step's `kept` says: a difference Rust keeps on purpose, where
//! Rust's answer is held to it and must still differ from Node's.

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

use crate::fakes::{done, fake_executable, Local, Other};

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

/// How a scenario writes what is particular to its run: its root, the
/// values its steps drew by the names it gives them, and its processes.
struct Names {
    root: String,
    named: Vec<(String, String)>,
    other: Option<u32>,
}

impl Names {
    /// The process ids a scenario names, by their names.
    fn pids(&self) -> Vec<(&'static str, u32)> {
        let mut pids = vec![("$PID", std::process::id()), ("$DEAD", DEAD)];
        pids.extend(self.other.map(|other| ("$OTHER", other)));
        pids
    }

    /// `$ROOT/a/b` as a path under the root, a named process as its id,
    /// the named values as drawn (`real`).
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

    /// A file's text with the named processes and values in it
    /// (`realText`).
    fn real_text(&self, text: &str) -> String {
        let mut filled = text.to_owned();
        for (name, pid) in self.pids() {
            filled = filled.replace(name, &pid.to_string());
        }
        for (name, drawn) in &self.named {
            filled = filled.replace(name.as_str(), drawn);
        }
        filled
    }

    /// What a step answered, with the root, the named values and the live
    /// processes written as the scenario writes them (`written`).
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
/// adapter it plays against, the window it prepared, and its second live
/// process when it names one.
struct Played {
    _dir: TempDir,
    _other: Option<Other>,
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

impl Scripted<'_> {
    /// The answers left unasked, by operation, but the `optional` ones.
    fn unused(&self, optional: &Value) -> Vec<String> {
        let optional = optional.as_array().cloned().unwrap_or_default();
        let mut unused: Vec<String> = self
            .answers
            .borrow()
            .iter()
            .filter(|(op, left)| !left.is_empty() && !optional.contains(&json!(op)))
            .map(|(op, _)| op.clone())
            .collect();
        unused.sort();
        unused
    }
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

/// A text field of a step, none for null and for a field not there.
fn text(value: &Value) -> Option<&str> {
    value.as_str()
}

/// A prepare step, played: the launch it names, as the engine gives it.
fn prepare(played: &mut Played, step: &Value) -> Value {
    let given = played.names.real(&step["prepare"]);
    let id = LaunchId::new(text(&given["launchId"]).unwrap()).expect("a launch id");
    let agent = given["agent"].as_object().map(|agent| Agent {
        model: agent.get("model").and_then(Value::as_str),
        effort: agent.get("effort").and_then(Value::as_str),
        thinking: agent.get("thinking").and_then(Value::as_str),
        designer: agent.get("designer") == Some(&Value::Bool(true)),
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
    match done(played.adapter.prepare(&launch)) {
        Ok(prepared) => {
            for (name, field) in step["draws"].as_object().into_iter().flatten() {
                assert_eq!(field, "nativeSession", "a value a prepare draws");
                let drawn = prepared.native_session.clone().expect("a session drawn");
                played.names.named.push((name.clone(), drawn));
            }
            let answer = json!({
                "argv": prepared.argv,
                "env": prepared.env,
                "dropEnv": prepared.drop_env,
                "nativeSession": prepared.native_session,
            });
            played.window = Some(prepared.window);
            answer
        }
        Err(refused) => json!({ "refused": refused }),
    }
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
/// step's answers. Answers left unasked are said, where Node's runner
/// refused the scenario.
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
    let mut answer = if step.get("observe").is_some() {
        match done(window.observe()) {
            Ok(found) => json!({ "answer": observed(&found) }),
            Err(thrown) => json!({ "throws": thrown }),
        }
    } else if step.get("ready").is_some() {
        match done(window.ready(&host, &pane)) {
            Ok(ready) => json!({ "answer": readiness(&ready) }),
            Err(thrown) => json!({ "throws": thrown }),
        }
    } else if let Some(text) = step["deliver"].as_str() {
        match done(window.deliver(&host, &pane, text)) {
            Ok(outcome) => json!({ "answer": admission(&outcome) }),
            Err(thrown) => json!({ "throws": thrown }),
        }
    } else {
        panic!("a step of no kind: {step}");
    };
    let unused = host.unused(&step["optional"]);
    if !unused.is_empty() {
        answer["unused"] = json!(unused);
    }
    answer["requests"] = json!(host.requests.into_inner());
    answer
}

/// Sets the root up as a step says: false for a step that asks the adapter.
fn set_up(played: &Played, step: &Value) -> bool {
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
        played
            .window()
            .opened(pid.map(|pid| u32::try_from(pid).unwrap()));
        return true;
    }
    if step.get("follow").is_some() {
        let session = names.real(&step["follow"]);
        played.window().follow(session.as_str().unwrap());
        return true;
    }
    false
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
    let other = scenario["steps"]
        .to_string()
        .contains("$OTHER")
        .then(Other::start);
    let names = Names {
        root: dir.path().to_string_lossy().into_owned(),
        named: Vec::new(),
        other: other.as_ref().map(Other::pid),
    };
    let env = names.real(&scenario["env"]).as_object().unwrap().clone();
    let vars = env
        .iter()
        .map(|(name, value)| (name.clone(), value.as_str().unwrap().to_owned()));
    let adapter = adapter(scenario["harness"].as_str().unwrap(), Env::from_vars(vars));
    let mut played = Played {
        _dir: dir,
        _other: other,
        names,
        env,
        adapter,
        window: None,
    };
    let root = Path::new(&played.names.root).to_path_buf();
    let mut answers = Vec::new();
    for (index, each) in scenario["steps"].as_array().unwrap().iter().enumerate() {
        if set_up(&played, each) {
            continue;
        }
        let before = tree(&root);
        let answer = if each.get("prepare").is_some() {
            prepare(&mut played, each)
        } else {
            ask(&played, each)
        };
        let mut fields = Map::new();
        fields.insert("step".to_owned(), json!(index));
        fields.extend(answer.as_object().unwrap().clone());
        fields.insert("tree".to_owned(), changes(&before, &tree(&root)));
        answers.push(played.names.written(&Value::Object(fields)));
    }
    answers
}

/// Whether Rust's `answer` to a step is what it should be: Node's, but
/// where the step keeps a difference, which must still be one.
fn differs(step: &Value, answer: &Value, node: &Value) -> Option<String> {
    let Some(kept) = step.get("kept") else {
        return (answer != node).then(|| format!("\n  rust {answer}\n  node {node}"));
    };
    let held = kept["answer"].as_object().unwrap();
    let fields = answer.as_object().unwrap();
    let wrong: Vec<String> = fields
        .iter()
        .filter(|(key, value)| match held.get(*key) {
            Some(kept) => kept != *value,
            None => node.get(key.as_str()) != Some(*value),
        })
        .map(|(key, value)| format!("{key}: {value}"))
        .chain(
            held.keys()
                .filter(|key| !fields.contains_key(*key))
                .map(|key| format!("{key}: not answered")),
        )
        .collect();
    if !wrong.is_empty() {
        return Some(format!(
            " (kept: {}):\n  rust {}",
            kept["why"],
            wrong.join(", ")
        ));
    }
    (answer == node).then(|| format!(" keeps a difference that is none: {}", kept["why"]))
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
            let step = &scenario["steps"][node["step"].as_u64().unwrap() as usize];
            if let Some(how) = differs(step, answer, node) {
                differ.push(format!("{name}, step {}{how}", node["step"]));
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

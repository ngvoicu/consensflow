//! The scenarios of `tests/goldens/launch/scenarios.<platform>.json`, played
//! against the Rust adapters step by step as the recorder played them against
//! Node's, each step's record held to Node's. The goldens' README
//! (`tests/goldens/README.md`) says what a step is, how its record is written
//! so that it is the same on every run, and what a step's `kept` says: a
//! difference Rust keeps on purpose, where Rust's settlement is held to it and
//! must still differ from Node's.
//!
//! Every wait an adapter makes is on a fake of `cf_harness::testing`: the
//! clock moves only when a step advances it, a look or a host request a
//! step holds waits until a step releases it, and the work begun is run by
//! hand until nothing moves.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use cf_base::env::Env;
use cf_base::{js, path};
use cf_harness::contract::{
    Adapter, Admission, Agent, Held, HostError, Launch, LaunchId, Observed, Pane, Readiness, Window,
};
use cf_harness::forget_launch;
use cf_harness::launch;
use cf_harness::seams::loopback::BodyFailed;
use cf_harness::seams::processes::{Failed, Program, Streams};
use cf_harness::seams::{Services, Time};
use cf_harness::testing::{
    called, fake_executable, name, route, Answer, ChildScript, Driver, Ends, Fakes, OtherProcess,
    ScriptedHost, Sent, Served,
};
use cf_proto::agents::Harness;
use regex::Regex;
use rusqlite::types::Value as Bound;
use rusqlite::Connection;
use serde_json::{json, Map, Value};
use tempfile::TempDir;
use url::form_urlencoded::byte_serialize;

/// A process id no process has (`DEAD` of the recorder).
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
    forms: RootForms,
    bundle: Regex,
    /// OpenCode's bundle hashes, in the order each first appeared.
    hashes: RefCell<Vec<String>>,
}

/// The ways the root reads in the text a step records, each written as a name
/// of its own (`rootForms` of the recorder): itself, a file URL, as JSON writes
/// it, and as a URL's query holds it.
struct RootForms {
    file_url: String,
    /// Each form and its name, a form spelled as one before it left out.
    plain: Vec<(String, &'static str)>,
}

/// `encodeURIComponent(text)`.
pub(crate) fn component(text: &str) -> String {
    text.bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' => char::from(byte).to_string(),
            b'-' | b'_' | b'.' | b'!' | b'~' | b'*' | b'\'' | b'(' | b')' => {
                char::from(byte).to_string()
            }
            other => format!("%{other:02X}"),
        })
        .collect()
}

impl RootForms {
    fn of(root: &str) -> Self {
        let component = component(root);
        let json = serde_json::to_string(root).unwrap();
        let forms = [
            (root.to_owned(), "$ROOT"),
            (json[1..json.len() - 1].to_owned(), "$JSON_ROOT"),
            (component.clone(), "$URI_ROOT"),
            (component.replace('\'', "%27"), "$URI27_ROOT"),
            (byte_serialize(root.as_bytes()).collect(), "$FORM_ROOT"),
        ];
        let mut plain: Vec<(String, &'static str)> = Vec::new();
        for (form, name) in forms {
            if !plain.iter().any(|(other, _)| *other == form) {
                plain.push((form, name));
            }
        }
        Self {
            file_url: path::to_file_url(root, cfg!(windows)).unwrap(),
            plain,
        }
    }
}

impl Names {
    /// The names of a run whose root is `root`, as the system names it.
    fn new(root: String, other: Option<u32>) -> Self {
        Self {
            forms: RootForms::of(&root),
            // OpenCode's bundle is named by a hash of the folder it is
            // published in, which the root is among the parents of: the hash
            // is not the same in two runs, and is written `$HASH`.
            bundle: Regex::new(r"(extensions[\\/]opencode[\\/])([0-9a-f]{64})").unwrap(),
            hashes: RefCell::new(Vec::new()),
            root,
            other,
        }
    }

    /// `text` with each OpenCode bundle hash named by the order it first
    /// appeared in among the scenario's (`$HASH1`, `$HASH2`, …).
    fn hashed(&self, text: &str) -> String {
        let mut hashes = self.hashes.borrow_mut();
        self.bundle
            .replace_all(text, |found: &regex::Captures| {
                let hash = &found[2];
                let at = hashes
                    .iter()
                    .position(|seen| seen == hash)
                    .unwrap_or_else(|| {
                        hashes.push(hash.to_owned());
                        hashes.len() - 1
                    });
                format!("{}$HASH{}", &found[1], at + 1)
            })
            .into_owned()
    }

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
            // A key beginning with `$` gets another, as Node's runner
            // writes it: the runner's own (`$utf16`) stand apart.
            Value::Object(fields) => Value::Object(
                fields
                    .iter()
                    .map(|(key, item)| {
                        let key = if key.starts_with('$') {
                            format!("${key}")
                        } else {
                            key.clone()
                        };
                        (key, self.written(item))
                    })
                    .collect(),
            ),
            Value::Number(number) => {
                let live = self.pids().into_iter().find(|&(name, pid)| {
                    name != "$DEAD" && number.as_u64() == Some(u64::from(pid))
                });
                live.map_or_else(|| value.clone(), |(name, _)| json!(name))
            }
            Value::String(text) => {
                // The root in each way a record may spell it, and as a window
                // names a program of the bundle's, with forward slashes.
                let mut text = text.replace(&self.forms.file_url, "file://$ROOT");
                for (form, name) in &self.forms.plain {
                    text = text.replace(form, name);
                }
                text = text.replace(&self.root.replace('\\', "/"), "$ROOT");
                if text.starts_with("$ROOT") {
                    text = posix(text);
                }
                json!(self.hashed(&text))
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
    /// The SQLite stores a scenario writes, by their names.
    dbs: HashMap<String, Connection>,
}

/// A scenario's answer to a host request: at once, failing, or held.
fn answer(names: &Names, given: &Value) -> Answer {
    if given.get("held") == Some(&json!(true)) {
        return Answer::Held;
    }
    Answer::Now(response(names, given))
}

/// A program's path as a test names it (`programOf` of the recorder): the
/// extension a Windows stand-in has taken off.
fn path_of(program: &Program) -> String {
    let path = program.executable.to_string_lossy();
    let lower = path.to_ascii_lowercase();
    [".mjs", ".cmd", ".bat", ".exe"]
        .iter()
        .find(|extension| lower.ends_with(*extension))
        .map_or_else(
            || path.to_string(),
            |extension| path[..path.len() - extension.len()].to_owned(),
        )
}

/// How a program was started, as Node's runner writes it down where an
/// adapter asks for it (`invocation`): its name and its path, its arguments,
/// its folder,
/// and the variables its environment adds to or changes in the scenario's, a
/// variable taken away `null`, each by name. Windows' names are of no case,
/// and written upper.
fn invocation(scenario: &Map<String, Value>, program: &Program) -> Value {
    let spelled = |name: &str| {
        if cfg!(windows) {
            name.to_ascii_uppercase()
        } else {
            name.to_owned()
        }
    };
    let given: BTreeMap<String, String> = program
        .env
        .iter()
        .map(|(name, value)| {
            (
                spelled(&name.to_string_lossy()),
                value.to_string_lossy().into_owned(),
            )
        })
        .collect();
    let scenario: BTreeMap<String, &str> = scenario
        .iter()
        .map(|(name, value)| (spelled(name), value.as_str().unwrap()))
        .collect();
    let mut changed: BTreeMap<String, Value> = BTreeMap::new();
    for (key, value) in &given {
        if scenario.get(key) != Some(&value.as_str()) {
            changed.insert(key.clone(), json!(value));
        }
    }
    for key in scenario.keys() {
        if !given.contains_key(key) {
            changed.insert(key.clone(), Value::Null);
        }
    }
    let env: Vec<Value> = changed
        .into_iter()
        .map(|(key, value)| json!([key, value]))
        .collect();
    json!({
        "program": name(program),
        "path": path_of(program),
        "args": program.args,
        "cwd": program.cwd.as_ref().map(|cwd| cwd.to_string_lossy().into_owned()),
        "env": env,
    })
}

/// A peer's answer as a scenario writes it: `{held: true}`, `{noHead:
/// true}`, or a head's status and its body (its text, `{json: value}` as
/// JSON with the paths in it under the root made whole, `{repeat: text,
/// times}` as that text that many times, `{held: true}`, `{cut: true}`).
fn served(names: &Names, given: &Value) -> Served {
    if given.get("held") == Some(&json!(true)) {
        return Served::Held;
    }
    if given.get("noHead") == Some(&json!(true)) {
        return Served::NoHead;
    }
    let status = u16::try_from(given["status"].as_u64().unwrap()).unwrap();
    let body = match &given["body"] {
        Value::String(text) => Sent::Now(text.as_bytes().to_vec()),
        whole if whole.get("json").is_some() => {
            Sent::Now(js::stringify(&names.real(&whole["json"])).into_bytes())
        }
        repeated if repeated.get("repeat").is_some() => {
            let times = usize::try_from(repeated["times"].as_u64().unwrap()).unwrap();
            Sent::Now(
                repeated["repeat"]
                    .as_str()
                    .unwrap()
                    .repeat(times)
                    .into_bytes(),
            )
        }
        held if held.get("held") == Some(&json!(true)) => Sent::Held,
        cut if cut.get("cut") == Some(&json!(true)) => Sent::Cut,
        _ => Sent::Now(Vec::new()),
    };
    Served::Head { status, body }
}

/// A child as a scenario scripts it: the lines it writes, and how it ends
/// (`itself`, `asked`, the default, `forced`, `never`).
fn child(given: &Value) -> ChildScript {
    let lines = given["lines"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|line| line.as_str().unwrap().to_owned())
        .collect();
    let ends = match given["ends"].as_str() {
        Some("itself") => Ends::Itself,
        Some("forced") => Ends::Forced,
        Some("never") => Ends::Never,
        _ => Ends::Asked,
    };
    ChildScript { lines, ends }
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

/// A path under the root with Windows' separators as POSIX's; on POSIX a
/// backslash is a name's own.
fn posix(path: String) -> String {
    if cfg!(windows) {
        path.replace('\\', "/")
    } else {
        path
    }
}

/// Every file, folder and link under `root`, by its path there: a file's
/// and a folder's mode (none on Windows), a file's text, a link's target,
/// never followed.
fn tree(root: &Path) -> Vec<(String, Value)> {
    fn walk(root: &Path, folder: &Path, found: &mut Vec<(String, Value)>) {
        for entry in fs::read_dir(folder).unwrap() {
            let full = entry.unwrap().path();
            let relative = posix(
                full.strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
            );
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
        // Devin's `false`, which the engine says as `Unsaid`'s sentence.
        Readiness::Held(Held::Unsaid) => json!(false),
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

/// Scripts the programs a step has `spawn` start.
fn script_children(played: &Played, step: &Value) {
    for (name, scripts) in step["children"].as_object().into_iter().flatten() {
        for script in scripts.as_array().unwrap() {
            played.fakes.processes.child(name, child(script));
        }
    }
}

/// Scripts what a step's work is answered by: the peer's routes, the
/// programs started, and the host's operations, a prepare's as well as the
/// others'.
fn script(played: &mut Played, step: &Value) {
    for (route, answers) in step["served"].as_object().into_iter().flatten() {
        let answers = answers
            .as_array()
            .unwrap()
            .iter()
            .map(|given| served(&played.names, given));
        played.fakes.loopback.serve(route, answers);
    }
    script_children(played, step);
    for (op, answers) in step["answers"].as_object().into_iter().flatten() {
        let answers: Vec<Answer> = answers
            .as_array()
            .unwrap()
            .iter()
            .map(|given| answer(&played.names, given))
            .collect();
        played.host.answer(op, answers);
    }
}

/// Begins what a step asks the window.
fn begin_asking(played: &mut Played, id: usize, step: &Value) {
    let window = Rc::clone(played.window.as_ref().expect("a window prepared"));
    let host = Rc::clone(&played.host);
    let pane = step.get("pane").map_or_else(
        || Pane {
            id: "p1-zeus".to_owned(),
            generation: 1,
        },
        |given| Pane {
            id: text(&given["id"]).unwrap(),
            generation: given["generation"].as_u64().unwrap(),
        },
    );
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
    in_order_begun(settled, &mut played.window)
}

/// The work settled, in the order it settled, written in the order begun.
/// A window prepared is the engine's as its preparation finishes, as Node's
/// runner keeps it: of two, the one that finished last.
fn in_order_begun<W>(
    settled: Vec<(usize, (Value, Option<W>))>,
    window: &mut Option<W>,
) -> Vec<Value> {
    let mut records: Vec<(usize, Value)> = settled
        .into_iter()
        .map(|(op, (mut record, opened))| {
            if let Some(opened) = opened {
                *window = Some(opened);
            }
            record["op"] = json!(op);
            (op, record)
        })
        .collect();
    records.sort_by_key(|(op, _)| *op);
    records.into_iter().map(|(_, record)| record).collect()
}

#[test]
fn a_key_beginning_with_a_dollar_is_written_with_another_as_node_s_runner_writes_it() {
    // A root the platform holds absolute, as a run's is.
    let root = if cfg!(windows) {
        r"C:\nowhere"
    } else {
        "/nowhere"
    };
    let names = Names::new(root.to_owned(), None);
    assert_eq!(
        names.written(&json!({"$utf16": [97], "plain": {"$ROOT": "$ROOT"}})),
        json!({"$$utf16": [97], "plain": {"$$ROOT": "$ROOT"}})
    );
}

#[cfg(unix)]
#[test]
fn the_root_is_named_by_how_a_record_spells_it_and_each_bundle_hash_by_its_order() {
    let names = Names::new("/tmp/cf-launch-golden-X y's".to_owned(), None);
    let written = |text: &str| names.written(&json!(text));
    assert_eq!(
        written("/tmp/cf-launch-golden-X y's/home"),
        json!("$ROOT/home")
    );
    assert_eq!(
        written("file:///tmp/cf-launch-golden-X%20y's/a"),
        json!("file://$ROOT/a")
    );
    // A query holds it as `encodeURIComponent` does, with the parser's `%27`
    // for the quote, or as a form does, with `+` for the space: each its own
    // name, so that one spelled the wrong way is seen.
    assert_eq!(
        written("http://127.0.0.1:41000/x?directory=%2Ftmp%2Fcf-launch-golden-X%20y's%2Fwork"),
        json!("http://127.0.0.1:41000/x?directory=$URI_ROOT%2Fwork")
    );
    assert_eq!(
        written("?directory=%2Ftmp%2Fcf-launch-golden-X%20y%27s"),
        json!("?directory=$URI27_ROOT")
    );
    assert_eq!(
        written("?directory=%2Ftmp%2Fcf-launch-golden-X+y%27s%2Fwork"),
        json!("?directory=$FORM_ROOT%2Fwork")
    );
    let (hash, other) = ("0123456789abcdef".repeat(4), "fedcba9876543210".repeat(4));
    let bundle = |hash: &str| format!("/x/extensions/opencode/{hash}/hosts");
    assert_eq!(
        written(&bundle(&hash)),
        json!("/x/extensions/opencode/$HASH1/hosts")
    );
    assert_eq!(
        written(&bundle(&other)),
        json!("/x/extensions/opencode/$HASH2/hosts"),
        "another bundle is another name"
    );
    assert_eq!(
        written(&bundle(&hash)),
        json!("/x/extensions/opencode/$HASH1/hosts")
    );
    // Pi's bundle is named by the same kind of hash, which does not change.
    let pi = format!("/x/extensions/pi/{hash}/hosts");
    assert_eq!(written(&pi), json!(pi));
}

#[test]
fn the_window_kept_is_the_one_prepared_last_and_records_go_in_the_order_begun() {
    // A begun first and B second, B finished first.
    let settled = vec![(1, (json!({}), Some("B"))), (0, (json!({}), Some("A")))];
    let mut window = None;
    let records = in_order_begun(settled, &mut window);
    assert_eq!(window, Some("A"));
    assert_eq!(records, [json!({"op": 0}), json!({"op": 1})]);
}

#[test]
fn a_stand_in_prints_what_it_says_and_fails_in_the_words_execfile_fails_in() {
    let file = "/root/bin/codex";
    let answer = |said: Value| stand_in_answer(file, "login status", &said);
    assert_eq!(answer(json!("[]\n")), Ok("[]\n".to_owned()));
    assert_eq!(answer(json!({"stdout": "[]\n"})), Ok("[]\n".to_owned()));
    assert_eq!(
        answer(json!({"exit": 0, "stderr": "ignored"})),
        Ok(String::new())
    );
    let program = if cfg!(windows) {
        "$NODE /root/bin/codex.mjs"
    } else {
        "/root/bin/codex"
    };
    assert_eq!(
        answer(json!({"stdout": "half", "stderr": "boom\n", "exit": 3})),
        Err(Failed {
            message: format!("Command failed: {program} login status\nboom\n"),
            code: Some(3),
            killed: false,
            stdout: "half".to_owned(),
        })
    );
    assert_eq!(
        answer(json!({"overflows": true})),
        Err(Failed {
            message: "stdout maxBuffer length exceeded".to_owned(),
            code: None,
            killed: false,
            stdout: String::new(),
        })
    );
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
            waits.extend(
                played
                    .fakes
                    .loopback
                    .waits(op)
                    .into_iter()
                    .map(|fetch| json!({ "fetch": fetch })),
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

/// What the stand-in at `file` answers to `args` (`standIn`'s `answers` of the
/// recorder): its output; a failure in `execFile`'s sentence for the program
/// Node runs (the stand-in itself, or on Windows Node and the stand-in's
/// script, `$NODE`, as Node's runner writes it), when it names an exit other
/// than 0; or more output than any buffer holds.
fn stand_in_answer(file: &str, args: &str, answer: &Value) -> Result<String, Failed> {
    let text = |field: &str| answer[field].as_str().unwrap_or_default().to_owned();
    if let Value::String(stdout) = answer {
        return Ok(stdout.clone());
    }
    if answer["overflows"] == json!(true) {
        return Err(Failed {
            message: "stdout maxBuffer length exceeded".to_owned(),
            code: None,
            killed: false,
            stdout: String::new(),
        });
    }
    let Some(code) = answer["exit"].as_i64().filter(|&code| code != 0) else {
        return Ok(text("stdout"));
    };
    let program = if cfg!(windows) {
        format!("$NODE {file}.mjs")
    } else {
        file.to_owned()
    };
    Err(Failed {
        message: format!("Command failed: {program} {args}\n{}", text("stderr")),
        code: Some(i32::try_from(code).unwrap()),
        killed: false,
        stdout: text("stdout"),
    })
}

/// A stand-in CLI that answers by its arguments (`standIn: {name, answers}` of
/// the recorder): its file there to be found and probed, and each answer
/// scripted, as often as it is asked.
fn stand_in(played: &mut Played, given: &Value) {
    let file = path::join(&[&played.names.root, "bin", given["name"].as_str().unwrap()]);
    let found = fake_executable(Path::new(&file));
    // The stand-in holds its answers, as Node's does: one that answers
    // otherwise is another file, of another size.
    let mut stand_in = fs::OpenOptions::new().append(true).open(&found).unwrap();
    writeln!(
        stand_in,
        "{} {}",
        if cfg!(windows) { "rem" } else { "#" },
        given["answers"]
    )
    .unwrap();
    let name = called(&found);
    for (args, answer) in given["answers"].as_object().into_iter().flatten() {
        let answered = stand_in_answer(&file, args, answer);
        if args == "*" {
            played.fakes.processes.every_answer(&name, answered);
        } else {
            played
                .fakes
                .processes
                .always_answer(&format!("{name} {args}"), answered);
        }
    }
}

/// A parameter as `node:sqlite` binds the JavaScript value: a number as a
/// double, whatever it holds, text as text, null as NULL.
fn bound(param: &Value) -> Bound {
    match param {
        Value::Null => Bound::Null,
        Value::Number(number) => Bound::Real(number.as_f64().unwrap()),
        Value::String(text) => Bound::Text(text.clone()),
        other => panic!("a parameter no step binds: {other}"),
    }
}

/// A step on a SQLite store of the scenario's: `new DatabaseSync(file)`,
/// `.exec(sql)`, `.prepare(sql).run(...params)` or `.close()`.
fn write_store(played: &mut Played, name: &str, step: &Value) {
    if let Some(file) = step.get("open") {
        let file = played.names.real(file);
        let store = Connection::open(file.as_str().unwrap()).unwrap();
        played.dbs.insert(name.to_owned(), store);
    } else if let Some(sql) = step["exec"].as_str() {
        played.dbs[name].execute_batch(sql).unwrap();
    } else if let Some(sql) = step["run"].as_str() {
        let params = step["params"].as_array().into_iter().flatten().map(bound);
        played.dbs[name]
            .execute(sql, rusqlite::params_from_iter(params))
            .unwrap();
    } else if step["close"] == json!(true) {
        played.dbs.remove(name).unwrap().close().unwrap();
    } else {
        panic!("a db step with nothing to do: {step}");
    }
}

/// Sets the root up as a step says: false for a step that is recorded.
fn set_up(played: &mut Played, step: &Value) -> bool {
    if let Some(name) = step["executable"].as_str() {
        fake_executable(Path::new(&path::join(&[&played.names.root, "bin", name])));
        return true;
    }
    if let Some(given) = step.get("standIn") {
        stand_in(played, given);
        return true;
    }
    if let Some(name) = step["db"].as_str() {
        write_store(played, name, step);
        return true;
    }
    let names = &played.names;
    if step.get("write").is_some() {
        let file = names.real(&step["write"]);
        let file = Path::new(file.as_str().unwrap());
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, names.real(&step["text"]).as_str().unwrap()).unwrap();
        return true;
    }
    if step.get("append").is_some() {
        let file = names.real(&step["append"]);
        let file = Path::new(file.as_str().unwrap());
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(file)
            .unwrap()
            .write_all(names.real(&step["text"]).as_str().unwrap().as_bytes())
            .unwrap();
        return true;
    }
    if step.get("mkdir").is_some() {
        let folder = names.real(&step["mkdir"]);
        fs::create_dir_all(folder.as_str().unwrap()).unwrap();
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
        script(played, step);
        begin_prepare(played, index, &given);
    } else if let Some(op) = step["release"].as_str() {
        // A route has a space in it (`GET /session`), a host's operation none.
        let released = if op == "look" {
            played.fakes.records.release()
        } else if op.contains(' ') {
            let given = served(&played.names, &step["answer"]);
            played.fakes.loopback.release(op, given)
        } else {
            let given = response(&played.names, &step["answer"]);
            played.host.release(op, given)
        };
        assert!(released, "{op}: nothing held to release");
    } else if let Some(route) = step["releaseBody"].as_str() {
        let body = match &step["body"] {
            Value::String(text) => Ok(text.as_bytes().to_vec()),
            _ => Err(BodyFailed::Cut),
        };
        assert!(
            played.fakes.loopback.release_body(route, body),
            "{route}: no body held to end"
        );
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
        script(played, step);
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
    let fetches: Vec<Value> = played
        .fakes
        .loopback
        .take_asked()
        .into_iter()
        .map(|request| {
            let headers: Vec<Value> = request
                .headers
                .iter()
                .map(|(name, value)| json!([name, value]))
                .collect();
            let body = request
                .body
                .as_deref()
                .map(|bytes| String::from_utf8_lossy(bytes).into_owned());
            json!({ "route": route(&request), "url": request.url, "headers": headers, "body": body })
        })
        .collect();
    if !fetches.is_empty() {
        fields.insert("fetches".to_owned(), json!(fetches));
    }
    // The stand-ins' runs, then the children started, as Node's runner
    // writes them down.
    let ran: Vec<Value> = played
        .fakes
        .processes
        .take_ran()
        .iter()
        .map(|(program, limits)| {
            let mut ran = invocation(&played.env, program);
            ran["limits"] = json!({
                "timeout": limits.timeout.as_millis(),
                "maxBuffer": limits.max_buffer,
            });
            ran
        })
        .collect();
    if !ran.is_empty() {
        fields.insert("ran".to_owned(), json!(ran));
    }
    let spawned: Vec<Value> = played
        .fakes
        .processes
        .take_spawned()
        .iter()
        .map(|(program, streams)| {
            let mut started = invocation(&played.env, program);
            started["streams"] = json!(match streams {
                Streams::Lines => "lines",
                Streams::Quiet => "quiet",
            });
            started
        })
        .collect();
    if !spawned.is_empty() {
        fields.insert("spawned".to_owned(), json!(spawned));
    }
    let written = played.fakes.processes.take_written();
    if !written.is_empty() {
        fields.insert("written".to_owned(), json!(written));
    }
    // The host's operations and then the peer's routes, each in order, as
    // Node's runner lists them.
    let optional = step["optional"].as_array().cloned().unwrap_or_default();
    let unused: Vec<String> = played
        .host
        .unused()
        .into_iter()
        .chain(played.fakes.loopback.unused())
        .filter(|op| !optional.contains(&json!(op)))
        .chain(played.fakes.processes.unused())
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
    let harness = Harness::from_kind(harness).unwrap_or_else(|| panic!("no harness {harness}"));
    launch::adapter(harness, services)
}

/// `path` as the system names it, a Windows name as libuv writes it: the root a
/// scenario is played in (`fs.realpath`, as the recorder had it), so that a
/// folder's real name is under it.
pub(crate) fn real_name(path: &Path) -> PathBuf {
    let real = fs::canonicalize(path).unwrap();
    let named = real.to_string_lossy();
    if cfg!(windows) {
        if let Some(rest) = named.strip_prefix(r"\\?\UNC\") {
            return PathBuf::from(format!(r"\\{rest}"));
        }
        if let Some(rest) = named.strip_prefix(r"\\?\") {
            if rest.as_bytes().get(1) == Some(&b':') {
                return PathBuf::from(rest);
            }
        }
    }
    real
}

/// The mask the files of a scenario are made under (`UMASK` of the recorder),
/// held for as long as this is: what a default mode comes out as is then the
/// same on every machine. Windows has no mask.
struct Umask(#[cfg(unix)] nix::sys::stat::Mode);

impl Umask {
    fn pin() -> Self {
        #[cfg(unix)]
        {
            use nix::sys::stat::{umask, Mode};
            Self(umask(Mode::from_bits_truncate(0o022)))
        }
        #[cfg(not(unix))]
        Self()
    }
}

impl Drop for Umask {
    fn drop(&mut self) {
        #[cfg(unix)]
        nix::sys::stat::umask(self.0);
    }
}

/// Plays `scenario` in a root of its own: each recorded step's record.
fn play(scenario: &Value) -> Vec<Value> {
    let _mask = Umask::pin();
    let dir = tempfile::Builder::new()
        .prefix("cf-launch-golden-")
        .tempdir()
        .unwrap();
    let root = real_name(dir.path());
    fs::create_dir(root.join("bin")).unwrap();
    let other = scenario["steps"]
        .to_string()
        .contains("$OTHER")
        .then(OtherProcess::start);
    let names = Names::new(
        root.to_string_lossy().into_owned(),
        other.as_ref().map(OtherProcess::pid),
    );
    let env = names.real(&scenario["env"]).as_object().unwrap().clone();
    let vars = env
        .iter()
        .map(|(name, value)| (name.clone(), value.as_str().unwrap().to_owned()));
    let env_vars = Env::from_vars(vars);
    let fakes = Fakes::new(&env_vars);
    let services = fakes.services(&env_vars, &root);
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
        dbs: HashMap::new(),
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
            "{}: the launch goldens are fixed recordings of macOS and Windows (tests/goldens/README.md)",
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

//! Each scenario of `tests/goldens/admin/scenarios.<platform>.json` played
//! against the Rust admin and held to what Node answered, step by step: the
//! files the scenario makes and the environment it gives, the answers of the
//! programs it runs and of the feeds it asks, and the clock it moves; and what
//! each step settled, left waiting and called.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::rc::Rc;

use cf_base::env::Env;
use cf_harness::admin::feed::{Feed, Network, FETCH_FAILED};
use cf_harness::admin::{release_source, Capture, HarnessAdmin, Latest};
use cf_harness::seams::Time;
use cf_harness::testing::{
    named, BodyEnding, Delivery, Driver, ManualTime, Response, Said, ScriptedCapture,
    ScriptedLatest, ScriptedNetwork, Told, EPOCH_MS,
};
use cf_process::{CaptureFailed, Captured};
use cf_proto::agents::Harness;
use serde_json::{json, Map, Value};

use crate::shape::{
    detection_json, field, golden, normalize, pretty, root_of, source_json, substitute, text,
    PLATFORM,
};

/// How many scenarios there are at least: a file with fewer has lost some.
/// Windows records nine fewer, those that make a link.
const AT_LEAST: usize = if cfg!(windows) { 85 } else { 94 };

/// What a begun step settled with.
type Done = Result<Value, String>;

/// The entries of a field that is an object, or none where it is absent.
fn entries<'a>(value: &'a Value, name: &str) -> impl Iterator<Item = (&'a String, &'a Value)> {
    value[name].as_object().into_iter().flatten()
}

/// What a program the scenario scripts answers: what it wrote, how it
/// failed, or when it is let to answer.
fn run_response(answer: &Value) -> Response<Said> {
    if answer["held"].as_bool() == Some(true) {
        return Response::Held;
    }
    if let Some(error) = answer.get("error") {
        return Response::Now(Err(CaptureFailed {
            message: field(error, "message").to_owned(),
            code: None,
            killed: error["killed"].as_bool() == Some(true),
            stdout: field(error, "stdout").to_owned(),
            stderr: field(error, "stderr").to_owned(),
        }));
    }
    Response::Now(Ok(Captured {
        stdout: field(answer, "stdout").to_owned(),
        stderr: field(answer, "stderr").to_owned(),
    }))
}

/// What a feed the scenario scripts answers.
fn latest_response(answer: &Value) -> Response<Told> {
    if answer["held"].as_bool() == Some(true) {
        return Response::Held;
    }
    match answer.get("error") {
        Some(error) => Response::Now(Err(error.as_str().unwrap().to_owned())),
        None => Response::Now(Ok(field(answer, "value").to_owned())),
    }
}

/// The bytes of a chunk the scenario writes: text, bytes, or text repeated.
fn bytes_of(chunk: &Value) -> Vec<u8> {
    if let Some(text) = chunk.as_str() {
        return text.as_bytes().to_vec();
    }
    if let Some(bytes) = chunk["bytes"].as_array() {
        return bytes
            .iter()
            .map(|byte| u8::try_from(byte.as_u64().unwrap()).unwrap())
            .collect();
    }
    let count = usize::try_from(chunk["count"].as_u64().unwrap()).unwrap();
    field(chunk, "text").repeat(count).into_bytes()
}

/// What the network does for a request, as the scenario scripts it. A
/// redirect is a status, which the feed refuses.
fn delivery(script: &Value) -> Delivery {
    let chunks = || {
        script["chunks"]
            .as_array()
            .into_iter()
            .flatten()
            .map(bytes_of)
            .collect()
    };
    match script["failure"].as_str() {
        Some("refused") => Delivery::Failure(FETCH_FAILED.to_owned()),
        Some("stall") => Delivery::Silence,
        Some("stall-body") => Delivery::Answer {
            status: 200,
            chunks: Vec::new(),
            ending: BodyEnding::Never,
        },
        Some("cut") => Delivery::Answer {
            status: 200,
            chunks: Vec::new(),
            ending: BodyEnding::Cut,
        },
        _ => Delivery::Answer {
            status: u16::try_from(script["status"].as_u64().unwrap_or(200)).unwrap(),
            chunks: chunks(),
            ending: BodyEnding::Whole,
        },
    }
}

/// Makes the files a scenario starts with.
fn build(files: &Value) {
    for file in files.as_array().unwrap() {
        if let Some(dir) = file["dir"].as_str() {
            fs::create_dir_all(dir).unwrap();
            continue;
        }
        let target = Path::new(field(file, "path"));
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        if let Some(link) = file["link"].as_str() {
            link_to(link, target);
            continue;
        }
        fs::write(target, field(file, "text")).unwrap();
        if file["executable"].as_bool() == Some(true) {
            set_mode(target, 0o755);
        } else if let Some(mode) = file["mode"].as_u64() {
            set_mode(target, u32::try_from(mode).unwrap());
        }
    }
}

#[cfg(unix)]
fn link_to(link: &str, target: &Path) {
    std::os::unix::fs::symlink(link, target).unwrap();
}

#[cfg(not(unix))]
fn link_to(_link: &str, _target: &Path) {
    panic!("a link is made on Unix alone: the recorder makes none on Windows");
}

#[cfg(unix)]
fn set_mode(file: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(file, fs::Permissions::from_mode(mode)).unwrap();
}

#[cfg(not(unix))]
fn set_mode(_file: &Path, _mode: u32) {}

/// One scenario being played.
struct Play {
    root: String,
    env: Env,
    time: Rc<ManualTime>,
    admin: Rc<HarnessAdmin>,
    capture: Rc<ScriptedCapture>,
    latest: Rc<ScriptedLatest>,
    network: Rc<ScriptedNetwork>,
    driver: Driver<Done>,
    /// The names of the steps begun, by the place each is held at.
    names: Vec<String>,
}

impl Play {
    /// A scenario's world: its files made, its programs and feeds scripted.
    fn new(scenario: &Value, root: String) -> Self {
        build(&substitute(&scenario["files"], &root));
        let env_vars = substitute(&scenario["env"], &root);
        let env = Env::from_vars(
            env_vars
                .as_object()
                .unwrap()
                .iter()
                .map(|(name, value)| (name.clone(), field_text(value))),
        );
        let effects = &scenario["effects"];
        let capture = Rc::new(ScriptedCapture::default());
        for (key, answers) in entries(effects, "run") {
            capture.answer(key, answers.as_array().unwrap().iter().map(run_response));
        }
        let latest = Rc::new(ScriptedLatest::default());
        for (id, answers) in entries(effects, "latest") {
            let harness = Harness::from_name(id).unwrap();
            latest.answer(
                harness,
                answers.as_array().unwrap().iter().map(latest_response),
            );
        }
        let network = Rc::new(ScriptedNetwork::default());
        for script in effects["fetch"].as_array().into_iter().flatten() {
            network.serve(delivery(script));
        }
        let time = Rc::new(ManualTime::new(EPOCH_MS));
        let asks: Rc<dyn Latest> = if effects.get("fetch").is_some() {
            Rc::new(Feed::new(
                Rc::clone(&time) as Rc<dyn Time>,
                Rc::clone(&network) as Rc<dyn Network>,
            ))
        } else {
            Rc::clone(&latest) as Rc<dyn Latest>
        };
        let admin = Rc::new(HarnessAdmin::new(
            env.clone(),
            Rc::clone(&time) as Rc<dyn Time>,
            asks,
            Rc::clone(&capture) as Rc<dyn Capture>,
        ));
        Self {
            root,
            env,
            time,
            admin,
            capture,
            latest,
            network,
            driver: Driver::default(),
            names: Vec::new(),
        }
    }

    /// Begins `work` under `name`.
    fn begin(&mut self, name: String, work: impl std::future::Future<Output = Done> + 'static) {
        self.driver.begin(self.names.len(), work);
        self.names.push(name);
    }

    /// One step: what it did, as Node's record writes it.
    fn step(&mut self, index: usize, step: &Value) -> Value {
        let name = step["name"]
            .as_str()
            .map_or_else(|| index.to_string(), str::to_owned);
        let id = step["id"].as_str().map(str::to_owned);
        let mut settled: Vec<(usize, Done)> = Vec::new();
        match field(step, "op") {
            "check" => {
                let (admin, refresh) = (Rc::clone(&self.admin), step["refresh"] == true);
                self.begin(name, async move {
                    let rows = admin.check(id.as_deref(), refresh).await?;
                    Ok(Value::Array(
                        rows.iter()
                            .map(|row| serde_json::to_value(&**row).unwrap())
                            .collect(),
                    ))
                });
            }
            "update" => {
                let admin = Rc::clone(&self.admin);
                self.begin(name, async move {
                    let outcome = admin.update(id.as_deref().unwrap_or_default()).await?;
                    Ok(serde_json::to_value(&outcome).unwrap())
                });
            }
            "source" => {
                let harness = Harness::from_name(id.as_deref().unwrap()).unwrap();
                let executable = substitute(&step["executable"], &self.root);
                let source = release_source(harness, executable.as_str().unwrap(), &self.env);
                self.begin(name, async move { Ok(source_json(&source)) });
            }
            "detect" => {
                let said = detection_json(&self.env);
                self.begin(name, async move { Ok(said) });
            }
            "release" => self.release(step),
            "advance" => {
                let until = self.time.wall_ms() + step["ms"].as_i64().unwrap();
                settled.extend(self.driver.run());
                while self.time.fire_next(until) {
                    settled.extend(self.driver.run());
                }
                self.time.settle_at(until);
            }
            "write" | "remove" => self.change(step),
            other => panic!("no step {other}"),
        }
        settled.extend(self.driver.run());
        settled.sort_by_key(|(place, _)| *place);
        self.record(&settled)
    }

    /// Answers the call that was held longest.
    fn release(&self, step: &Value) {
        let (call, answer) = (field(step, "call"), &step["answer"]);
        let released = if field(step, "kind") == "run" {
            self.capture.release(
                call,
                match run_response(answer) {
                    Response::Now(said) => said,
                    Response::Held => panic!("a held answer cannot release"),
                },
            )
        } else {
            let Response::Now(told) = latest_response(answer) else {
                panic!("a held answer cannot release")
            };
            self.latest.release(Harness::from_name(call).unwrap(), told)
        };
        assert!(released, "nothing held for {call}");
    }

    /// Changes the files, as the step says.
    fn change(&self, step: &Value) {
        let target = substitute(&step["path"], &self.root);
        let target = Path::new(target.as_str().unwrap());
        if field(step, "op") == "remove" {
            fs::remove_file(target).unwrap();
            return;
        }
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(target, field(step, "text")).unwrap();
        if step["executable"].as_bool() == Some(true) {
            set_mode(target, 0o755);
        }
    }

    /// What settled, what waits and what was called, as the recorder writes it.
    fn record(&self, settled: &[(usize, Done)]) -> Value {
        let mut record = Map::new();
        let settled = settled.iter().map(|(place, done)| match done {
            Ok(result) => json!({ "name": self.names[*place], "result": result }),
            Err(error) => json!({ "name": self.names[*place], "error": error }),
        });
        record.insert("settled".to_owned(), Value::Array(settled.collect()));
        let waiting: Vec<&String> = self
            .driver
            .pending()
            .into_iter()
            .map(|place| &self.names[place])
            .collect();
        if !waiting.is_empty() {
            record.insert("waiting".to_owned(), json!(waiting));
        }
        let calls = self.calls();
        if !calls.is_empty() {
            record.insert("calls".to_owned(), Value::Object(calls));
        }
        normalize(&Value::Object(record), &self.root)
    }

    /// The calls since the last step, by program, by harness and by address.
    fn calls(&self) -> Map<String, Value> {
        let mut calls = Map::new();
        let mut programs: BTreeMap<String, Vec<Value>> = BTreeMap::new();
        for (program, limits) in self.capture.take_ran() {
            let mut call = Map::new();
            call.insert(
                "program".to_owned(),
                json!(program.executable.to_string_lossy()),
            );
            call.insert("args".to_owned(), json!(program.args));
            if let Some(cwd) = &program.cwd {
                call.insert("cwd".to_owned(), json!(cwd.to_string_lossy()));
            }
            let env = if program.env.iter().eq(self.env.iter()) {
                json!("same")
            } else {
                json!(program
                    .env
                    .iter()
                    .map(|(name, value)| (
                        name.to_string_lossy().into_owned(),
                        value.to_string_lossy().into_owned()
                    ))
                    .collect::<BTreeMap<_, _>>())
            };
            call.insert("env".to_owned(), env);
            call.insert(
                "limits".to_owned(),
                json!({
                    "timeoutMs": u64::try_from(limits.timeout.as_millis()).unwrap(),
                    "maxBuffer": limits.max_buffer,
                }),
            );
            programs
                .entry(named(&program))
                .or_default()
                .push(Value::Object(call));
        }
        keyed(&mut calls, "capture", programs);
        let mut feeds: BTreeMap<String, Vec<Value>> = BTreeMap::new();
        for (id, source) in self.latest.take_asked() {
            feeds
                .entry(id.as_str().to_owned())
                .or_default()
                .push(json!({ "source": source_json(&source) }));
        }
        keyed(&mut calls, "latest", feeds);
        let mut addresses: BTreeMap<String, Vec<Value>> = BTreeMap::new();
        for (url, timeout, follow) in self.network.take_asked() {
            addresses.entry(url).or_default().push(json!({
                "timeoutMs": u64::try_from(timeout.as_millis()).unwrap(),
                "redirect": if follow { "follow" } else { "error" },
            }));
        }
        keyed(&mut calls, "fetch", addresses);
        calls
    }
}

/// `calls` with what was called by `kind`, each name in order, left out when
/// nothing was called.
fn keyed(calls: &mut Map<String, Value>, kind: &str, by_name: BTreeMap<String, Vec<Value>>) {
    if by_name.is_empty() {
        return;
    }
    let object = by_name
        .into_iter()
        .map(|(name, each)| (name, Value::Array(each)))
        .collect::<Map<_, _>>();
    calls.insert(kind.to_owned(), Value::Object(object));
}

/// The text of a value that is one.
fn field_text(value: &Value) -> String {
    value.as_str().unwrap().to_owned()
}

/// The record Node wrote of step `index`, with what Rust keeps from Node on
/// purpose in place of what Node answered.
fn expected(scenario: &Value, index: usize) -> Value {
    let mut record = scenario["recorded"][index].clone();
    let kept = scenario["kept"].as_array().into_iter().flatten();
    for each in kept.filter(|each| each["step"] == index) {
        let settled = record["settled"].as_array_mut().unwrap();
        let entry = settled
            .iter_mut()
            .find(|entry| entry["name"] == each["name"])
            .unwrap();
        let entry = entry.as_object_mut().unwrap();
        let key = if entry.contains_key("result") {
            "result"
        } else {
            "error"
        };
        entry.insert(key.to_owned(), each["rust"].clone());
    }
    record
}

/// Plays one scenario, and says each step where Rust answers otherwise than
/// Node.
fn play(scenario: &Value) -> Vec<String> {
    let name = field(scenario, "name");
    let dir = tempfile::tempdir().unwrap();
    let mut play = Play::new(scenario, root_of(dir.path()));
    let mut differing = Vec::new();
    for (index, step) in scenario["steps"].as_array().unwrap().iter().enumerate() {
        let actual = play.step(index, step);
        let node = expected(scenario, index);
        if text(&actual) != text(&node) {
            differing.push(format!(
                "{name}, step {index} ({}):\nRust:\n{}\nNode:\n{}",
                field(step, "op"),
                pretty(&actual),
                pretty(&node)
            ));
        }
    }
    let unused = [play.capture.unused(), play.latest.unused()].concat();
    if !unused.is_empty() || play.network.unused() > 0 {
        differing.push(format!("{name}: scripted and never asked: {unused:?}"));
    }
    differing
}

#[test]
fn every_scenario_answers_as_node_answered_step_by_step() {
    let scenarios = golden(&format!("scenarios.{PLATFORM}.json"));
    let scenarios = scenarios.as_array().unwrap();
    assert!(scenarios.len() >= AT_LEAST, "{} scenarios", scenarios.len());
    let differing: Vec<String> = scenarios.iter().flat_map(play).collect();
    assert!(
        differing.is_empty(),
        "{} steps differ from Node's, of {} scenarios:\n\n{}",
        differing.len(),
        scenarios.len(),
        differing.join("\n\n")
    );
}

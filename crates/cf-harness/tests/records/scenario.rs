//! A scenario as the recording writes one: its steps, the readings its looks
//! read, and what tells a reason ConsensFlow's own from a platform's.

use std::io::Read;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::{Map, Value};

/// A scenario as the recording writes one.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scenario {
    pub name: String,
    pub env: Map<String, Value>,
    /// How long the cached reader keeps a conversation nobody reads, when not ten minutes.
    #[serde(default)]
    pub idle: Option<u64>,
    pub steps: Vec<Step>,
    /// Each item the looks read, once.
    pub items: Vec<Value>,
    /// Each reading, once, its items by their place in `items`.
    pub readings: Vec<Value>,
}

/// One step of a scenario: what it does to the files and stores under the
/// root, or a look.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum Step {
    Mkdir(Mkdir),
    Write(Write),
    Append(Append),
    Replace(Replace),
    Remove(Remove),
    Move(Move),
    Mtime(Mtime),
    Clock(Clock),
    Db(Db),
    Look(Look),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mkdir {
    pub mkdir: String,
}

/// A file written whole; or a fixture with line `line` written as `text`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Write {
    pub write: String,
    pub text: String,
    #[serde(default)]
    pub fixture: Option<String>,
    #[serde(default)]
    pub line: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Append {
    pub append: String,
    pub text: String,
}

/// Written beside the file, then renamed over it.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Replace {
    pub replace: String,
    pub text: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Remove {
    pub remove: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Move {
    #[serde(rename = "move")]
    pub from: String,
    pub to: String,
}

/// The file's times set `ago` milliseconds before the clock (after it, when negative).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mtime {
    pub mtime: String,
    pub ago: i64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Clock {
    pub clock: u64,
}

/// A step of a writer connection, kept open across looks.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Db {
    pub db: String,
    #[serde(default)]
    pub open: Option<String>,
    #[serde(default)]
    pub exec: Option<String>,
    #[serde(default)]
    pub run: Option<String>,
    #[serde(default)]
    pub params: Option<Vec<Value>>,
    #[serde(default)]
    pub close: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Reader {
    Cached,
    Fresh,
    Both,
}

/// A look and what it read: by the cached reader, a fresh one, or both.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Look {
    pub look: Reader,
    pub kind: String,
    pub session: String,
    #[serde(default)]
    pub options: Map<String, Value>,
    #[serde(default)]
    pub env: Option<Map<String, Value>>,
    /// Played between OpenCode's two snapshot reads.
    #[serde(default)]
    pub between: Option<Vec<Step>>,
    /// The cached reader's reading.
    #[serde(default)]
    pub read: Option<usize>,
    /// The fresh reader's, when it is not the cached one's.
    #[serde(default)]
    pub fresh: Option<usize>,
    /// The look whose reading object the cached reader handed back again.
    #[serde(default)]
    pub same_as: Option<usize>,
    /// The look whose quota object this reading holds again.
    #[serde(default)]
    pub quota_same_as: Option<usize>,
}

pub fn goldens() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("goldens")
        .join("records")
}

pub fn scenarios(group: &str) -> Vec<Scenario> {
    let file = goldens().join(format!("{group}.json.gz"));
    let mut text = String::new();
    flate2::read::GzDecoder::new(
        std::fs::File::open(&file)
            .expect("the goldens: they are fixed recordings (tests/goldens/README.md)"),
    )
    .read_to_string(&mut text)
    .unwrap();
    serde_json::from_str(&text).unwrap_or_else(|error| panic!("{group}: {error}"))
}

/// Whether a reading's reason is one it may have: ConsensFlow's own sentence
/// (`ours`, by how each begins, as tables.json lists them), or a platform's
/// failure, which only its prefix promises.
pub fn a_reason_it_may_have(reason: &str, ours: &[String]) -> bool {
    reason == "unreadable: «platform»" || ours.iter().any(|ours| reason.starts_with(ours.as_str()))
}

/// The beginnings of ConsensFlow's own reasons, from tables.json.
pub fn our_reasons() -> Vec<String> {
    let file = goldens().join("tables.json");
    let tables: Value = serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap();
    tables["reasons"]["ours"]
        .as_array()
        .unwrap()
        .iter()
        .map(|ours| ours.as_str().unwrap().to_owned())
        .collect()
}

/// Whether `path` is a place under the scenario's root.
pub fn under_root(path: &str) -> bool {
    path == "$ROOT" || path.starts_with("$ROOT/")
}

/// What the interpreter will need of a step, there: a path under the root,
/// a writer's step doing one thing, a fixture's line named with its text.
pub fn check(step: &Step, scenario: &str) {
    let ok = match step {
        Step::Mkdir(Mkdir { mkdir }) => under_root(mkdir),
        Step::Write(Write {
            write,
            text,
            fixture,
            line,
        }) => {
            under_root(write)
                && fixture.is_some() == line.is_some()
                && (fixture.is_none() || !text.contains('\n'))
        }
        Step::Append(Append { append, text }) => under_root(append) && !text.is_empty(),
        Step::Replace(Replace { replace, text }) => under_root(replace) && !text.is_empty(),
        Step::Remove(Remove { remove }) => under_root(remove),
        Step::Move(Move { from, to }) => under_root(from) && under_root(to),
        Step::Mtime(Mtime { mtime, ago }) => under_root(mtime) && *ago != 0,
        Step::Clock(Clock { clock }) => *clock > 0,
        Step::Db(Db {
            db,
            open,
            exec,
            run,
            params,
            close,
        }) => {
            let doing = [
                open.is_some(),
                exec.is_some(),
                run.is_some(),
                close.is_some(),
            ];
            !db.is_empty()
                && doing.iter().filter(|does| **does).count() == 1
                && open.as_deref().is_none_or(under_root)
                && (params.is_none() || run.is_some())
                && close.is_none_or(|close| close)
        }
        Step::Look(Look {
            kind,
            session,
            options,
            env,
            between,
            ..
        }) => {
            for inner in between.as_deref().unwrap_or_default() {
                check(inner, scenario);
            }
            ["codex", "claude-code", "pi", "opencode", "devin"].contains(&kind.as_str())
                && (session.is_empty() || !session.contains('/'))
                && options.values().all(|value| !value.is_null())
                && env
                    .as_ref()
                    .is_none_or(|env| env.values().all(Value::is_string))
        }
    };
    assert!(ok, "{scenario}: {step:?}");
}

/// Every look of `steps`, those played between snapshot reads included.
pub fn looks(steps: &[Step]) -> Vec<&Look> {
    steps
        .iter()
        .flat_map(|step| match step {
            Step::Look(look) => {
                let mut all = vec![look];
                all.extend(looks(look.between.as_deref().unwrap_or_default()));
                all
            }
            _ => Vec::new(),
        })
        .collect()
}

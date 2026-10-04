//! The harness records' goldens (`npm run goldens:records`, from
//! `tests/goldens/records/`): the scenarios Node played against its readers
//! and what each look read. Step 3.3 ports the readers that answer them. Until
//! then this reads every scenario into the steps the port will play and the
//! readings it will be held to, and counts the looks still to answer, harness
//! by harness, so a golden that shrinks or changes shape fails here.

// The goldens' own reading: a failure in it is the test's.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::{Map, Value};

/// A scenario as `tests/goldens/records/runner.mjs` writes one.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Scenario {
    name: String,
    env: Map<String, Value>,
    /// How long the cached reader keeps a conversation nobody reads, when not ten minutes.
    #[serde(default)]
    idle: Option<u64>,
    steps: Vec<Step>,
    /// Each item the looks read, once.
    items: Vec<Value>,
    /// Each reading, once, its items by their place in `items`.
    readings: Vec<Value>,
}

/// One step of a scenario: what it does to the files and stores under the
/// root, or a look.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum Step {
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
struct Mkdir {
    mkdir: String,
}

/// A file written whole; or a fixture with line `line` written as `text`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Write {
    write: String,
    text: String,
    #[serde(default)]
    fixture: Option<String>,
    #[serde(default)]
    line: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Append {
    append: String,
    text: String,
}

/// Written beside the file, then renamed over it.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Replace {
    replace: String,
    text: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Remove {
    remove: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Move {
    #[serde(rename = "move")]
    from: String,
    to: String,
}

/// The file's times set `ago` milliseconds before the clock (after it, when negative).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Mtime {
    mtime: String,
    ago: i64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Clock {
    clock: u64,
}

/// A step of a writer connection, kept open across looks.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Db {
    db: String,
    #[serde(default)]
    open: Option<String>,
    #[serde(default)]
    exec: Option<String>,
    #[serde(default)]
    run: Option<String>,
    #[serde(default)]
    params: Option<Vec<Value>>,
    #[serde(default)]
    close: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Reader {
    Cached,
    Fresh,
    Both,
}

/// A look and what it read: by the cached reader, a fresh one, or both.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Look {
    look: Reader,
    kind: String,
    session: String,
    #[serde(default)]
    options: Map<String, Value>,
    #[serde(default)]
    env: Option<Map<String, Value>>,
    /// Played between OpenCode's two snapshot reads.
    #[serde(default)]
    between: Option<Vec<Step>>,
    /// The cached reader's reading.
    #[serde(default)]
    read: Option<usize>,
    /// The fresh reader's, when it is not the cached one's.
    #[serde(default)]
    fresh: Option<usize>,
    /// The look whose reading object the cached reader handed back again.
    #[serde(default)]
    same_as: Option<usize>,
    /// The look whose quota object this reading holds again.
    #[serde(default)]
    quota_same_as: Option<usize>,
}

fn goldens() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("goldens")
        .join("records")
}

fn scenarios(group: &str) -> Vec<Scenario> {
    let file = goldens().join(format!("{group}.json.gz"));
    let mut text = String::new();
    flate2::read::GzDecoder::new(
        std::fs::File::open(&file).expect("the goldens: npm run goldens:records"),
    )
    .read_to_string(&mut text)
    .unwrap();
    serde_json::from_str(&text).unwrap_or_else(|error| panic!("{group}: {error}"))
}

/// The sentences a reading's reason may be: ConsensFlow's own, or a
/// platform's failure, which only its prefix promises.
fn a_reason_it_may_have(reason: &str) -> bool {
    const OURS: [&str; 17] = [
        "missing session id",
        "unreadable: no claude session ",
        "unreadable: no codex rollout for ",
        "unreadable: no pi session ",
        "unreadable: no opencode store for ",
        "unreadable: no opencode session ",
        "unreadable: missing home in env",
        "unreadable: missing native ",
        "unreadable: malformed JSONL at record ",
        "unreadable: empty ",
        "unreadable: malformed OpenCode ",
        "unreadable: missing OpenCode event ",
        "unreadable: missing Devin session",
        "unreadable: conflicting Devin completion evidence",
        "unreadable: cyclic Devin main chain",
        "unreadable: missing Devin main chain ancestor",
        "unreadable: invalid Devin message identity",
    ];
    reason == "unreadable: «platform»"
        || reason == "unreadable: unknown Devin message role"
        || OURS.iter().any(|ours| reason.starts_with(ours))
}

/// Whether `path` is a place under the scenario's root.
fn under_root(path: &str) -> bool {
    path == "$ROOT" || path.starts_with("$ROOT/")
}

/// What the interpreter will need of a step, there: a path under the root,
/// a writer's step doing one thing, a fixture's line named with its text.
fn check(step: &Step, scenario: &str) {
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
fn looks(steps: &[Step]) -> Vec<&Look> {
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

#[test]
fn every_look_node_took_is_read_whole_and_counted_until_the_readers_answer_it() {
    let mut by_kind = BTreeMap::<String, usize>::new();
    let mut counted = Vec::new();
    for group in ["sequences", "suite", "sweep"] {
        let scenarios = scenarios(group);
        let mut group_looks = 0;
        for scenario in &scenarios {
            assert!(!scenario.name.is_empty());
            assert!(
                scenario.idle.is_none_or(|idle| idle > 0),
                "{}",
                scenario.name
            );
            assert!(
                scenario.env.values().all(Value::is_string),
                "{}",
                scenario.name
            );
            for reading in &scenario.readings {
                if reading["unknown"] == Value::Bool(true) {
                    let reason = reading["reason"].as_str().unwrap();
                    assert!(a_reason_it_may_have(reason), "{}: {reason}", scenario.name);
                } else {
                    for item in reading["items"].as_array().unwrap() {
                        let at = item.as_u64().unwrap();
                        assert!(at < scenario.items.len() as u64, "{}", scenario.name);
                    }
                }
            }
            for step in &scenario.steps {
                check(step, &scenario.name);
            }
            for look in looks(&scenario.steps) {
                let read = look.read.or(look.fresh);
                assert!(
                    read.is_some_and(|read| read < scenario.readings.len()),
                    "{}: a look with no reading",
                    scenario.name
                );
                assert!(
                    matches!(look.look, Reader::Fresh) == look.read.is_none(),
                    "{}: a fresh look holds no cached reading, any other one does",
                    scenario.name
                );
                assert!(look.same_as.is_none() || look.quota_same_as.is_none());
                *by_kind.entry(look.kind.clone()).or_default() += 1;
                group_looks += 1;
            }
        }
        counted.push((group, scenarios.len(), group_looks));
    }
    assert_eq!(
        counted,
        [
            ("sequences", 46, 1131),
            ("suite", 173, 201),
            ("sweep", 180, 180)
        ]
    );
    assert_eq!(
        by_kind.into_iter().collect::<Vec<_>>(),
        [
            ("claude-code".to_owned(), 650),
            ("codex".to_owned(), 262),
            ("devin".to_owned(), 273),
            ("opencode".to_owned(), 100),
            ("pi".to_owned(), 227),
        ]
    );
    println!("1512 looks, none answered yet: the readers come with step 3.3");
}

#[test]
fn the_tables_hold_every_quota_case_and_the_collation_of_ascii() {
    let file = goldens().join("tables.json");
    let tables: Value = serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap();
    let quota = &tables["quota"];
    assert_eq!(quota["defaultZone"], "America/Los_Angeles");
    let instants = quota["instants"].as_array().unwrap().len();
    let resets = quota["resets"].as_array().unwrap();
    assert!(resets
        .iter()
        .all(|row| row["at"].as_array().unwrap().len() == instants));
    assert_eq!((resets.len(), instants), (39, 16));
    let collation = &tables["collation"];
    let characters = collation["characters"].as_str().unwrap();
    assert_eq!(characters.len(), 95);
    let matrix = collation["matrix"].as_array().unwrap();
    assert!(matrix
        .iter()
        .all(|row| row.as_str().unwrap().len() == characters.len()));
    let words = collation["words"].as_array().unwrap().len();
    assert!(collation["pairs"]
        .as_array()
        .unwrap()
        .iter()
        .all(|row| row.as_str().unwrap().len() == words));
}

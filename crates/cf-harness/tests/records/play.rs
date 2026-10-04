//! The player: a scenario's steps played against the Rust readers in a
//! temporary root, as `runner.mjs` played them against Node's, each look
//! held to the reading Node's look read.
//!
//! Like the runner, the player owns time: the clock starts at [`START`] and
//! moves only by a step, and each file step sets the file's times to the
//! clock, a millisecond after the step before it. A store's writer is a
//! connection of the player's, kept open across looks as a harness keeps its
//! own.

use std::collections::HashMap;
use std::fs::{self, File, FileTimes};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};
use std::time::{Duration, UNIX_EPOCH};

use cf_base::env::Env;
use cf_base::js;
use cf_harness::records::{Cache, Look, Options, PiSettlement, Quota, Reading, IDLE_MS};
use jiff::tz::{TimeZone, TimeZoneDatabase};
use rusqlite::types::Value as Bound;
use rusqlite::Connection;
use serde_json::{json, Map, Value};
use tempfile::TempDir;

use crate::scenario::{
    goldens, Append, Clock, Db, Mkdir, Move, Mtime, Reader, Remove, Replace, Scenario, Step, Write,
};

/// Where every scenario's clock starts: 2026-09-21T12:26:40.000Z.
const START: i64 = 1_790_000_000_000;

/// The zone the machine was in when Node made the goldens, which a reset that
/// names none is read in: the generator sets `TZ` to the one tables.json names.
static LOCAL_ZONE: LazyLock<TimeZone> = LazyLock::new(|| {
    let tables: Value =
        serde_json::from_str(&fs::read_to_string(goldens().join("tables.json")).unwrap()).unwrap();
    let name = tables["quota"]["defaultZone"].as_str().unwrap();
    TimeZoneDatabase::bundled().get(name).unwrap()
});

/// Whether this build reads what `look` looks at: a harness whose reader is
/// ported, and a session to read. The others wait for the readers and the
/// switch still to come.
pub fn ported(look: &crate::scenario::Look) -> bool {
    matches!(look.kind.as_str(), "codex" | "pi" | "devin") && !look.session.is_empty()
}

/// A ported harness's reader of `session`, made afresh.
fn reader(kind: &str, session: &str, env: &Env) -> Box<dyn Look + Send> {
    match kind {
        "codex" => cf_harness::codex::record::reader(session, env),
        "pi" => cf_harness::pi::record::reader(session, env, &LOCAL_ZONE),
        "devin" => cf_harness::devin::record::reader(session, env),
        other => panic!("no reader of {other} yet"),
    }
}

/// What playing a scenario did with its looks.
#[derive(Debug, Default)]
pub struct Played {
    /// Held to what Node read.
    pub answered: usize,
    /// Left for the readers still to come.
    pub pending: usize,
}

/// Plays `scenario`, holding each look of a ported harness to Node's.
/// `ours` are the beginnings of ConsensFlow's own reasons.
pub fn play(scenario: &Scenario, ours: &[String]) -> Played {
    let root = tempfile::tempdir().unwrap();
    let env = environment(root.path(), &scenario.env);
    let idle = scenario
        .idle
        .map_or(IDLE_MS, |idle| i64::try_from(idle).unwrap());
    let mut stage = Stage {
        scenario,
        ours,
        writers: HashMap::new(),
        root,
        clock: START,
        env,
        cache: Cache::new(
            Box::new(|kind, session, env| Ok(reader(kind, session, env))),
            idle,
            START,
        ),
        handed_out: Vec::new(),
        quotas: Vec::new(),
        played: Played::default(),
    };
    for (index, step) in scenario.steps.iter().enumerate() {
        stage.step(step, index);
    }
    stage.played
}

/// A scenario being played.
struct Stage<'a> {
    scenario: &'a Scenario,
    ours: &'a [String],
    /// The stores' writers, by the scenario's name for each: closed before
    /// the root is removed.
    writers: HashMap<String, Connection>,
    root: TempDir,
    clock: i64,
    env: Env,
    cache: Cache,
    /// Each reading the cached reader handed out, and the step it first went to.
    handed_out: Vec<(Arc<Reading>, usize)>,
    /// Each quota a new reading held, and the step it first went to.
    quotas: Vec<(Arc<Quota>, usize)>,
    played: Played,
}

impl Stage<'_> {
    fn step(&mut self, step: &Step, index: usize) {
        match step {
            Step::Mkdir(Mkdir { mkdir }) => fs::create_dir_all(self.path(mkdir)).unwrap(),
            Step::Write(Write {
                write,
                text,
                fixture,
                line,
            }) => {
                self.clock += 1;
                let text = match (fixture, line) {
                    (Some(fixture), Some(line)) => fixture_text(fixture, *line, text),
                    _ => text.clone(),
                };
                fs::write(self.path(write), text).unwrap();
                stamp(&self.path(write), self.clock);
            }
            Step::Append(Append { append, text }) => {
                self.clock += 1;
                File::options()
                    .append(true)
                    .create(true)
                    .open(self.path(append))
                    .unwrap()
                    .write_all(text.as_bytes())
                    .unwrap();
                stamp(&self.path(append), self.clock);
            }
            Step::Replace(Replace { replace, text }) => {
                self.clock += 1;
                let file = self.path(replace);
                let mut beside = file.clone().into_os_string();
                beside.push(".new");
                fs::write(&beside, text).unwrap();
                stamp(Path::new(&beside), self.clock);
                fs::rename(&beside, &file).unwrap();
            }
            // `fs.rm(path, { recursive: true, force: true })`.
            Step::Remove(Remove { remove }) => {
                let path = self.path(remove);
                match fs::symlink_metadata(&path) {
                    Ok(metadata) if metadata.is_dir() => fs::remove_dir_all(&path).unwrap(),
                    Ok(_) => fs::remove_file(&path).unwrap(),
                    Err(_) => {}
                }
            }
            Step::Move(Move { from, to }) => fs::rename(self.path(from), self.path(to)).unwrap(),
            Step::Mtime(Mtime { mtime, ago }) => stamp(&self.path(mtime), self.clock - ago),
            Step::Clock(Clock { clock }) => self.clock += i64::try_from(*clock).unwrap(),
            Step::Db(db) => self.db(db),
            Step::Look(look) => self.look(look, index),
        }
    }

    fn look(&mut self, look: &crate::scenario::Look, index: usize) {
        if !ported(look) {
            self.played.pending += 1;
            return;
        }
        let name = format!("{}, step {index}", self.scenario.name);
        assert!(
            look.between.is_none(),
            "{name}: a look between snapshot reads, which no ported reader takes"
        );
        let env = look.env.as_ref().map_or_else(
            || self.env.clone(),
            |env| environment(self.root.path(), env),
        );
        let options = options(self.root.path(), &look.options);
        if matches!(look.look, Reader::Cached | Reader::Both) {
            let reading = self
                .cache
                .look(&look.kind, &look.session, &env, &options, self.clock);
            self.hold(&reading, look.read.unwrap(), &format!("{name}, cached"));
            self.identity(&reading, look, index, &name);
        }
        if matches!(look.look, Reader::Fresh | Reader::Both) {
            let reading = reader(&look.kind, &look.session, &env).look(&options, self.clock);
            // A look of both readers records the fresh reading only where it differs.
            let at = look.fresh.or(look.read).unwrap();
            self.hold(&reading, at, &format!("{name}, fresh"));
        }
        self.played.answered += 1;
    }

    /// A writer's step: `new DatabaseSync(file)`, `.exec(sql)`,
    /// `.prepare(sql).run(...params)` or `.close()`.
    fn db(&mut self, step: &Db) {
        let Db {
            db,
            open,
            exec,
            run,
            params,
            close,
        } = step;
        if let Some(open) = open {
            let writer = Connection::open(self.path(open)).unwrap();
            self.writers.insert(db.clone(), writer);
        } else if let Some(exec) = exec {
            self.writers[db].execute_batch(exec).unwrap();
        } else if let Some(run) = run {
            let params = params.iter().flatten().map(bound);
            self.writers[db]
                .execute(run, rusqlite::params_from_iter(params))
                .unwrap();
        } else if close.is_some() {
            self.writers.remove(db).unwrap().close().unwrap();
        }
    }

    /// Holds `reading` to the reading Node's look read, as text. A
    /// platform's reason is promised only as a class.
    fn hold(&self, reading: &Reading, at: usize, name: &str) {
        let read = match reading {
            Reading::Unknown(reason)
                if !self
                    .ours
                    .iter()
                    .any(|ours| reason.starts_with(ours.as_str())) =>
            {
                assert!(reason.starts_with("unreadable: "), "{name}: {reason}");
                json!({ "unknown": true, "reason": "unreadable: «platform»" })
            }
            reading => serde_json::to_value(reading).unwrap(),
        };
        // Text is compared below, where every number is written as
        // JavaScript writes it: first, every number of the reading itself is
        // one JavaScript holds, an integer past 2^53 a double.
        assert!(
            holds_js_numbers(&read),
            "{name}: a number JavaScript rounds"
        );
        let mut expected = self.scenario.readings[at].clone();
        if let Some(items) = expected.get_mut("items").and_then(Value::as_array_mut) {
            for item in items {
                let place = usize::try_from(item.as_u64().unwrap()).unwrap();
                *item = self.scenario.items[place].clone();
            }
        }
        assert_eq!(js::stringify(&read), js::stringify(&expected), "{name}");
    }

    /// Holds the cached reader to the objects Node's handed out: the reading
    /// of an earlier look again where Node's was (`sameAs`), and the quota of
    /// an earlier one where Node's was (`quotaSameAs`); another one where
    /// Node's was another.
    fn identity(
        &mut self,
        reading: &Arc<Reading>,
        look: &crate::scenario::Look,
        index: usize,
        name: &str,
    ) {
        let again = self
            .handed_out
            .iter()
            .find(|(handed, _)| Arc::ptr_eq(handed, reading))
            .map(|(_, first)| *first);
        assert_eq!(again, look.same_as, "{name}: the reading handed out before");
        if again.is_some() {
            return;
        }
        self.handed_out.push((Arc::clone(reading), index));
        let quota = match &**reading {
            Reading::Known(record) => record.quota.as_ref(),
            Reading::Unknown(_) => None,
        };
        let Some(quota) = quota else {
            assert_eq!(look.quota_same_as, None, "{name}");
            return;
        };
        let held = self
            .quotas
            .iter()
            .find(|(handed, _)| Arc::ptr_eq(handed, quota))
            .map(|(_, first)| *first);
        assert_eq!(
            held, look.quota_same_as,
            "{name}: the quota handed out before"
        );
        if held.is_none() {
            self.quotas.push((Arc::clone(quota), index));
        }
    }

    fn path(&self, at: &str) -> PathBuf {
        PathBuf::from(resolve(self.root.path(), at))
    }
}

/// Whether every number in `value` is one a JavaScript number holds as it
/// is: no integer past 2^53 kept whole.
fn holds_js_numbers(value: &Value) -> bool {
    const SAFE: u64 = 1 << 53;
    match value {
        Value::Number(number) => {
            number.as_u64().is_none_or(|whole| whole <= SAFE)
                && number
                    .as_i64()
                    .is_none_or(|whole| whole.unsigned_abs() <= SAFE)
        }
        Value::Array(items) => items.iter().all(holds_js_numbers),
        Value::Object(fields) => fields.values().all(holds_js_numbers),
        _ => true,
    }
}

/// `$ROOT/a/b` as a path under `root`, joined as this platform joins it;
/// any other text as it is.
fn resolve(root: &Path, value: &str) -> String {
    match value.strip_prefix("$ROOT") {
        Some(rest) => rest
            .split('/')
            .filter(|part| !part.is_empty())
            .fold(root.to_path_buf(), |path, part| path.join(part))
            .to_string_lossy()
            .into_owned(),
        None => value.to_owned(),
    }
}

/// A scenario's or a look's environment, its paths made real.
fn environment(root: &Path, vars: &Map<String, Value>) -> Env {
    Env::from_vars(
        vars.iter()
            .map(|(name, value)| (name.clone(), resolve(root, value.as_str().unwrap()))),
    )
}

/// `value` with every `$ROOT` path in it made real, at any depth, as
/// `runner.mjs`'s `resolveAll` makes them.
fn resolve_all(root: &Path, value: &Value) -> Value {
    match value {
        Value::Array(items) => {
            Value::Array(items.iter().map(|item| resolve_all(root, item)).collect())
        }
        Value::Object(fields) => Value::Object(
            fields
                .iter()
                .map(|(key, item)| (key.clone(), resolve_all(root, item)))
                .collect(),
        ),
        Value::String(text) => Value::String(resolve(root, text)),
        other => other.clone(),
    }
}

/// A look's options, its paths made real, as the type holds them. An option
/// the type cannot hold fails the scenario: none is dropped.
fn options(root: &Path, given: &Map<String, Value>) -> Options {
    let Value::Object(mut options) = resolve_all(root, &Value::Object(given.clone())) else {
        unreachable!("an object resolves to an object");
    };
    let pi_settlement = options.remove("piSettlement").map(|settlement| {
        let Value::Object(mut fields) = settlement else {
            panic!("piSettlement is no object: {settlement}");
        };
        let mut text = |name: &str| {
            fields.remove(name).map(|value| match value {
                Value::String(text) => text,
                other => panic!("{name} is no text: {other}"),
            })
        };
        let settlement = PiSettlement {
            directory: text("directory"),
            launch_id: text("launchId"),
        };
        assert!(
            fields.is_empty(),
            "piSettlement holds more than the type: {fields:?}"
        );
        settlement
    });
    assert!(
        options.is_empty(),
        "options the type cannot hold: {options:?}"
    );
    Options { pi_settlement }
}

/// A fixture's lines with line `line` written as `text`, each line ended by
/// a line break.
fn fixture_text(fixture: &str, line: usize, text: &str) -> String {
    let file = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/engine/fixtures/completion")
        .join(fixture);
    let whole = fs::read_to_string(file).unwrap();
    // `trimEnd()`: JavaScript's white space, which leaves U+0085 alone.
    let mut lines: Vec<&str> = whole
        .trim_end_matches(|character: char| {
            character == '\u{FEFF}' || (character != '\u{85}' && character.is_whitespace())
        })
        .split('\n')
        .collect();
    lines[line] = text;
    format!("{}\n", lines.join("\n"))
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

/// Sets a file's times to `ms`, the clock's reading.
fn stamp(file: &Path, ms: i64) {
    let at = UNIX_EPOCH + Duration::from_millis(u64::try_from(ms).unwrap());
    File::options()
        .write(true)
        .open(file)
        .unwrap()
        .set_times(FileTimes::new().set_accessed(at).set_modified(at))
        .unwrap();
}

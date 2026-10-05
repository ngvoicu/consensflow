//! What staging a fixture takes: its text, the pieces a harness writes it in,
//! a home to write them in, and the two ways of reading the home.

use std::fs::{self, File, FileTimes};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};

use cf_base::env::Env;
use cf_proto::agents::Harness;
use jiff::tz::TimeZone;
use rusqlite::types::Value as Bound;
use rusqlite::Connection;
use serde_json::Value;
use tempfile::TempDir;

use crate::contract::Records;
use crate::records::{Options, Reading, Thread};
use crate::seams::Time;
use crate::testing::{LocalRecords, ManualTime, EPOCH_MS};

/// How long Pi's record is quiet before its settlement is decided by that.
pub(super) const PI_QUIET_MS: i64 = 120_000;

/// How far the clock moves between one piece and the next.
pub(super) const STEP_MS: i64 = 2_000;

/// How far it moves for a last look, past any window a reader waits out.
pub(super) const LATE_MS: i64 = PI_QUIET_MS + 10_000;

/// A fixture's text, by its path under the fixtures folder.
pub(super) fn fixture(name: &str) -> String {
    fs::read_to_string(fixtures().join(name)).unwrap()
}

/// The fixtures folder.
pub(super) fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/engine/fixtures/completion")
}

pub(super) fn fixture_lines(name: &str) -> Vec<String> {
    fixture(name)
        .trim_end()
        .split('\n')
        .map(str::to_owned)
        .collect()
}

pub(super) fn fixture_json(name: &str) -> Value {
    serde_json::from_str(&fixture(name)).unwrap()
}

/// Where `text` is cut in half, at a character.
pub(super) fn half(text: &str) -> usize {
    text.char_indices()
        .nth(text.chars().count() / 2)
        .map_or(0, |(at, _)| at)
}

/// Each line in two halves and then its newline, so a look finds every kind
/// of last line.
pub(super) fn pieces(lines: &[String]) -> Vec<String> {
    lines
        .iter()
        .flat_map(|line| {
            let half = half(line);
            [
                line[..half].to_owned(),
                line[half..].to_owned(),
                "\n".to_owned(),
            ]
        })
        .filter(|piece| !piece.is_empty())
        .collect()
}

/// Sets a file's times to `ms`.
pub(super) fn stamp(file: &Path, ms: i64) {
    let at = UNIX_EPOCH + Duration::from_millis(u64::try_from(ms).unwrap());
    File::options()
        .write(true)
        .open(file)
        .unwrap()
        .set_times(FileTimes::new().set_accessed(at).set_modified(at))
        .unwrap();
}

pub(super) fn append(file: &Path, text: &str) {
    File::options()
        .append(true)
        .create(true)
        .open(file)
        .unwrap()
        .write_all(text.as_bytes())
        .unwrap();
}

/// An insert or replace of `row` into `table`, bound as `node:sqlite` binds
/// the JavaScript values: a number as a double.
pub(super) fn upsert(db: &Connection, table: &str, row: &Value) {
    let columns = row.as_object().unwrap();
    let names: Vec<String> = columns
        .keys()
        .map(|column| format!("\"{column}\""))
        .collect();
    let marks = vec!["?"; columns.len()].join(", ");
    let params = columns.values().map(|value| match value {
        Value::Null => Bound::Null,
        Value::Number(number) => Bound::Real(number.as_f64().unwrap()),
        Value::String(text) => Bound::Text(text.clone()),
        other => panic!("a column no store holds: {other}"),
    });
    db.execute(
        &format!(
            "insert or replace into {table} ({}) values ({marks})",
            names.join(", ")
        ),
        rusqlite::params_from_iter(params),
    )
    .unwrap();
}

/// A home that a conversation grows in, and the two ways of reading it, on
/// one clock.
pub(super) struct Rig {
    name: String,
    pub(super) root: TempDir,
    time: Rc<ManualTime>,
    pub(super) through: Thread,
    beside: LocalRecords,
    looks: usize,
    last: Option<Arc<Reading>>,
}

impl Rig {
    /// A home whose environment is `vars` over its root.
    pub(super) fn new(name: &str, vars: fn(&Path) -> Vec<(&'static str, PathBuf)>) -> Self {
        let root = tempfile::tempdir().unwrap();
        let env = Env::from_vars(vars(root.path()));
        let time = Rc::new(ManualTime::new(EPOCH_MS));
        let clock: Rc<dyn Time> = Rc::clone(&time) as Rc<dyn Time>;
        Self {
            name: name.to_owned(),
            through: Thread::new(env.clone(), TimeZone::UTC, Rc::clone(&clock)).unwrap(),
            beside: LocalRecords::new(env, clock),
            root,
            time,
            looks: 0,
            last: None,
        }
    }

    pub(super) fn path(&self, under: &[&str]) -> PathBuf {
        under
            .iter()
            .fold(self.root.path().to_path_buf(), |path, part| path.join(part))
    }

    pub(super) fn now(&self) -> i64 {
        self.time.wall_ms()
    }

    pub(super) fn tick(&self, ms: i64) {
        self.time.settle_at(self.now() + ms);
    }

    /// A look through the thread and one beside it, which must read the same.
    pub(super) async fn look(&mut self, harness: Harness, session: &str, options: &Options) {
        let through = self.through.look(harness, session, options).await;
        let beside = self.beside.look(harness, session, options).await;
        self.looks += 1;
        assert_eq!(*through, *beside, "{}, look {}", self.name, self.looks);
        self.last = Some(through);
    }

    /// How many looks were made, once the last of them read a record, as a
    /// harness that kept one would have it read.
    pub(super) fn finish(self) -> usize {
        let last = self.last.expect("a look");
        assert!(
            matches!(*last, Reading::Known(_)),
            "{}: the last look read {last:?}",
            self.name
        );
        self.looks
    }
}

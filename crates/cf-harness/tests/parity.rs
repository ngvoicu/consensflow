//! `npm run parity:records`, the Rust half: each conversation Node's half
//! (`tests/parity/records.mjs`) read on this machine, read again by the
//! Rust readers at the same instant and held to Node's digest of its
//! reading. Ignored by `cargo test`: it reads the machine's own harness
//! stores, and only the npm script, which writes the digests first, runs it.
//!
//! A conversation whose stamp changed between the two reads was written in
//! between: it is counted, not compared. A line of JSON this build cannot
//! hold is a difference kept on purpose: counted, as the records' decision
//! asks, and not failed.
//!
//! Each conversation's reads are timed too, for step 3.5 to decide where a
//! look runs: a first look, beside Node's, a look that finds nothing new,
//! and the locate of a JSONL harness's transcript. The slowest first look is
//! named with its transcript's size.

// The run's own scaffolding: a failure in it is the test's.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use cf_base::env::Env;
use cf_base::file::stat;
use cf_base::js;
use cf_base::text::utf16_len;
use cf_harness::records::{self, Options, Reading};
use cf_proto::agents::Harness;
use jiff::tz::TimeZoneDatabase;
use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

/// Where Node's half writes its digests.
const DIGESTS: &str = "consensflow-parity-records.jsonl";

/// A conversation as Node's half read it.
#[derive(Deserialize)]
struct Read {
    kind: String,
    session: String,
    now: i64,
    zone: String,
    env: Map<String, Value>,
    stamp: Value,
    /// How long Node's read took, in milliseconds.
    ms: f64,
    reading: Value,
}

/// What became of the conversations of one harness.
#[derive(Default)]
struct Tally {
    same: usize,
    /// Written between the two reads.
    changed: usize,
    /// Holding JSON this build cannot hold.
    kept: usize,
    first: Vec<Duration>,
    node: Vec<Duration>,
    unchanged: Vec<Duration>,
    locate: Vec<Duration>,
    /// The slowest first look, and the size of the transcript it read.
    slowest: Option<(Duration, Option<u64>)>,
}

#[test]
#[ignore = "reads this machine's harness stores: npm run parity:records"]
fn every_conversation_node_read_here_reads_the_same() {
    let digests = std::env::temp_dir().join(DIGESTS);
    let text = fs::read_to_string(&digests)
        .unwrap_or_else(|error| panic!("{}: {error}: npm run parity:records", digests.display()));
    let ours = our_reasons();
    let mut tallies = BTreeMap::<String, Tally>::new();
    let mut differences = Vec::new();
    for line in text.lines() {
        let read: Read = serde_json::from_str(line).unwrap();
        let harness = Harness::from_kind(&read.kind).unwrap();
        let env = Env::from_vars(
            read.env
                .iter()
                .map(|(name, value)| (name.clone(), value.as_str().unwrap().to_owned())),
        );
        let local = TimeZoneDatabase::bundled().get(&read.zone).unwrap();
        let tally = tallies.entry(read.kind.clone()).or_default();

        let started = Instant::now();
        let Ok(mut reader) = records::reader(harness, &read.session, &env, &local) else {
            panic!("{} {}: no reader", read.kind, read.session);
        };
        let reading = reader.look(&Options::default(), read.now);
        let first = started.elapsed();
        tally.first.push(first);
        tally.node.push(Duration::from_secs_f64(read.ms / 1000.0));
        if tally.slowest.is_none_or(|(slowest, _)| first > slowest) {
            tally.slowest = Some((first, read.stamp["size"].as_u64()));
        }
        let started = Instant::now();
        let again = reader.look(&Options::default(), read.now);
        tally.unchanged.push(started.elapsed());
        if matches!(harness, Harness::Claude | Harness::Codex | Harness::Pi) {
            let started = Instant::now();
            records::has_transcript(harness, &read.session, &env).unwrap();
            tally.locate.push(started.elapsed());
        }

        if js::stringify(&restamp(&read.stamp)) != js::stringify(&read.stamp) {
            tally.changed += 1;
            continue;
        }
        assert_eq!(
            again, reading,
            "{} {}: a look that finds nothing new",
            read.kind, read.session
        );
        if let Reading::Unknown(reason) = &*reading {
            if reason.contains("JSON this build cannot hold") {
                println!("kept: {} {}: {reason}", read.kind, read.session);
                tally.kept += 1;
                continue;
            }
        }
        let digest = digest(&reading, &ours);
        if js::stringify(&digest) == js::stringify(&read.reading) {
            tally.same += 1;
        } else {
            differences.push(format!(
                "{} {}:\n  node: {}\n  rust: {}",
                read.kind,
                read.session,
                js::stringify(&read.reading),
                js::stringify(&digest)
            ));
        }
    }
    println!("\n{}", report(&tallies));
    assert!(
        tallies.values().map(|tally| tally.same).sum::<usize>() > 0,
        "no conversation compared"
    );
    assert!(
        differences.is_empty(),
        "{} differ:\n{}",
        differences.len(),
        differences.join("\n")
    );
}

/// The beginnings of ConsensFlow's own reasons, which are compared whole: a
/// platform's is compared as its class.
fn our_reasons() -> Vec<String> {
    let tables = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/goldens/records/tables.json");
    let tables: Value = serde_json::from_str(&fs::read_to_string(tables).unwrap()).unwrap();
    serde_json::from_value(tables["reasons"]["ours"].clone()).unwrap()
}

/// What Node's half wrote of a reading (`digest`): each item's id, role,
/// completeness, time, its text's UTF-16 length and SHA-256, and whether it
/// is commentary; then the reading's flags, quota and settlement.
fn digest(reading: &Reading, ours: &[String]) -> Value {
    if let Reading::Unknown(reason) = reading {
        let own = ours.iter().any(|ours| reason.starts_with(ours.as_str()));
        let reason = if own {
            reason.as_str()
        } else {
            "unreadable: «platform»"
        };
        return json!({ "unknown": true, "reason": reason });
    }
    let read = serde_json::to_value(reading).unwrap();
    let items: Vec<Value> = read["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| {
            let text = item["text"].as_str().unwrap();
            json!([
                item["id"],
                item["role"],
                item["complete"],
                item.get("at").unwrap_or(&Value::Null),
                utf16_len(text),
                hex(&Sha256::digest(text.as_bytes())),
                item.get("commentary") == Some(&Value::Bool(true)),
            ])
        })
        .collect();
    json!({
        "items": items,
        "inFlight": read["inFlight"],
        "asking": read["asking"],
        "failed": read["failed"],
        "quota": read["quota"],
        "settlement": read["settlement"]["state"],
    })
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// A stamp as Node's half took it, taken again now: a transcript's size and
/// time of writing, or a store's sums over the session's rows.
fn restamp(stamp: &Value) -> Value {
    match stamp {
        Value::Array(stamps) => Value::Array(stamps.iter().map(restamp).collect()),
        Value::Object(fields) if fields.contains_key("sql") => {
            let mut taken = fields.clone();
            let rows = sums(
                fields["file"].as_str().unwrap(),
                fields["sql"].as_str().unwrap(),
                fields["session"].as_str().unwrap(),
            );
            taken.insert("rows".to_owned(), Value::Array(rows));
            Value::Object(taken)
        }
        Value::Object(fields) => {
            let file = fields["file"].as_str().unwrap();
            match stat(Path::new(file)) {
                Ok(found) => json!({ "file": file, "size": found.size, "mtimeMs": found.mtime_ms }),
                Err(_) => json!({ "file": file, "gone": true }),
            }
        }
        other => other.clone(),
    }
}

/// `sql`'s rows in the store `file`, read-only, `?1` the session: none for
/// a store that cannot be read, as Node's half answered.
fn sums(file: &str, sql: &str, session: &str) -> Vec<Value> {
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let Ok(store) = Connection::open_with_flags(file, flags) else {
        return Vec::new();
    };
    let Ok(mut statement) = store.prepare(sql) else {
        return Vec::new();
    };
    let names: Vec<String> = statement
        .column_names()
        .into_iter()
        .map(str::to_owned)
        .collect();
    let rows = statement.query_map([session], |row| {
        let mut fields = Map::new();
        for (index, name) in names.iter().enumerate() {
            let value = match row.get_ref(index)? {
                ValueRef::Null => Value::Null,
                ValueRef::Integer(whole) => json!(whole),
                ValueRef::Real(real) => json!(real),
                ValueRef::Text(text) => json!(String::from_utf8_lossy(text)),
                ValueRef::Blob(_) => unreachable!("a stamp sums no blob"),
            };
            fields.insert(name.clone(), value);
        }
        Ok(Value::Object(fields))
    });
    rows.and_then(Iterator::collect).unwrap_or_default()
}

/// What became of each harness's conversations, and how long its reads took.
fn report(tallies: &BTreeMap<String, Tally>) -> String {
    let mut lines = vec![format!(
        "{:<12} {:>5} {:>8} {:>5}   {:<26} {:<26} {:<26} {:<26} slowest",
        "harness",
        "same",
        "changed",
        "kept",
        "first look p50/p95/max",
        "node's p50/p95/max",
        "unchanged p50/p95/max",
        "locate p50/p95/max"
    )];
    for (kind, tally) in tallies {
        let slowest = match tally.slowest {
            Some((time, Some(size))) => format!(
                "{:.2} ms, {:.1} MB",
                time.as_secs_f64() * 1000.0,
                size as f64 / 1e6
            ),
            Some((time, None)) => format!("{:.2} ms", time.as_secs_f64() * 1000.0),
            None => "-".to_owned(),
        };
        lines.push(format!(
            "{kind:<12} {:>5} {:>8} {:>5}   {:<26} {:<26} {:<26} {:<26} {slowest}",
            tally.same,
            tally.changed,
            tally.kept,
            spread(&tally.first),
            spread(&tally.node),
            spread(&tally.unchanged),
            spread(&tally.locate),
        ));
    }
    lines.join("\n")
}

/// The median, the 95th percentile and the longest of `times`, in
/// milliseconds.
fn spread(times: &[Duration]) -> String {
    if times.is_empty() {
        return "-".to_owned();
    }
    let mut sorted = times.to_vec();
    sorted.sort();
    let at = |share: f64| {
        let place = ((sorted.len() - 1) as f64 * share).round() as usize;
        sorted[place].as_secs_f64() * 1000.0
    };
    format!("{:.2}/{:.2}/{:.2} ms", at(0.5), at(0.95), at(1.0))
}

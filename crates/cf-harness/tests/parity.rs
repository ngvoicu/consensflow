//! `npm run parity:records`, the Rust half: each conversation Node's half
//! (`tests/parity/records.mjs`) copied from this machine's stores into a
//! snapshot and read there twice, read twice here by the Rust readers from
//! the same snapshot at the same instant, each look held to Node's digest of
//! its reading. Ignored by `cargo test`: only the npm script, which writes
//! the snapshot and the digests first and names their file in
//! `CF_PARITY_RECORDS`, runs it.
//!
//! A line of JSON this build cannot hold is a difference kept on purpose:
//! counted, as the records' decision asks, and not failed.
//!
//! Each conversation's looks are timed too, for step 3.5 to decide where a
//! look runs: a first look, beside Node's, a look that finds nothing new,
//! and the locate of a JSONL harness's transcript in the live stores. The
//! slowest first look is named with its transcript's size.

// The run's own scaffolding: a failure in it is the test's.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use cf_base::env::Env;
use cf_base::js;
use cf_base::text::utf16_len;
use cf_harness::records::{self, Options, Reading};
use cf_proto::agents::Harness;
use jiff::tz::{Offset, TimeZone, TimeZoneDatabase};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

/// A conversation as Node's half read it.
#[derive(Deserialize)]
struct Read {
    kind: String,
    session: String,
    now: i64,
    zone: String,
    /// The environment that names the snapshot's copies.
    env: Map<String, Value>,
    /// The environment that names the live stores.
    live: Map<String, Value>,
    /// How long Node's first look took, in milliseconds.
    ms: f64,
    /// Node's first look, and its look after it.
    reading: Value,
    again: Value,
}

/// What became of the conversations of one harness.
#[derive(Default)]
struct Tally {
    same: usize,
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
    let process = Env::from_process();
    let digests = process
        .path("CF_PARITY_RECORDS")
        .expect("Node's digests, named by npm run parity:records");
    let text = fs::read_to_string(digests)
        .unwrap_or_else(|error| panic!("{}: {error}", digests.display()));
    let ours = our_reasons();
    let mut tallies = BTreeMap::<String, Tally>::new();
    let mut differences = Vec::new();
    for line in text.lines() {
        let read: Read = serde_json::from_str(line).unwrap();
        let harness = Harness::from_kind(&read.kind).unwrap();
        let env = environment(&read.env);
        let local = zone(&read.zone);
        let tally = tallies.entry(read.kind.clone()).or_default();

        let started = Instant::now();
        let Ok(mut reader) = records::reader(harness, &read.session, &env, &local) else {
            panic!("{} {}: no reader", read.kind, read.session);
        };
        let reading = reader.look(&Options::default(), read.now);
        let first = started.elapsed();
        let started = Instant::now();
        let again = reader.look(&Options::default(), read.now);
        tally.unchanged.push(started.elapsed());
        tally.first.push(first);
        tally.node.push(Duration::from_secs_f64(read.ms / 1000.0));
        if tally.slowest.is_none_or(|(slowest, _)| first > slowest) {
            tally.slowest = Some((first, transcript_size(harness, &read.session, &env)));
        }
        if matches!(harness, Harness::Claude | Harness::Codex | Harness::Pi) {
            let live = environment(&read.live);
            let started = Instant::now();
            records::has_transcript(harness, &read.session, &live).unwrap();
            tally.locate.push(started.elapsed());
        }

        if let Reading::Unknown(reason) = &*reading {
            if reason.contains("JSON this build cannot hold") {
                println!("kept: {} {}: {reason}", read.kind, read.session);
                tally.kept += 1;
                continue;
            }
        }
        let looks = [(&reading, &read.reading), (&again, &read.again)];
        let differ: Vec<String> = looks
            .iter()
            .zip(["first look", "look after it"])
            .filter_map(|((rust, node), which)| {
                let rust = js::stringify(&digest(rust, &ours));
                let node = js::stringify(node);
                (rust != node).then(|| format!("  {which}:\n    node: {node}\n    rust: {rust}"))
            })
            .collect();
        if differ.is_empty() {
            tally.same += 1;
        } else {
            differences.push(format!(
                "{} {}:\n{}",
                read.kind,
                read.session,
                differ.join("\n")
            ));
        }
    }
    println!("\n{}", report(&tallies));
    assert!(
        tallies.values().map(|tally| tally.same).sum::<usize>() > 0,
        "no conversation compared: Node's half found none here"
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

/// An environment Node's half wrote down.
fn environment(vars: &Map<String, Value>) -> Env {
    Env::from_vars(
        vars.iter()
            .map(|(name, value)| (name.clone(), value.as_str().unwrap().to_owned())),
    )
}

/// The zone Node's `Intl` named as the process's: a name, or an offset
/// (`+03:00`) where `TZ` gave one.
fn zone(name: &str) -> TimeZone {
    let Some(offset) = name.strip_prefix(['+', '-']) else {
        return TimeZoneDatabase::bundled().get(name).unwrap();
    };
    let (hours, minutes) = offset.split_once(':').unwrap();
    let seconds = (hours.parse::<i32>().unwrap() * 60 + minutes.parse::<i32>().unwrap()) * 60;
    let seconds = if name.starts_with('-') {
        -seconds
    } else {
        seconds
    };
    TimeZone::fixed(Offset::from_seconds(seconds).unwrap())
}

/// The size of the transcript a JSONL harness keeps of `session`, in the
/// snapshot `env` names; none for a store.
fn transcript_size(harness: Harness, session: &str, env: &Env) -> Option<u64> {
    let (folder, under) = match harness {
        Harness::Claude => ("CLAUDE_CONFIG_DIR", "projects"),
        Harness::Codex => ("CODEX_HOME", "sessions"),
        Harness::Pi => ("PI_CODING_AGENT_SESSION_DIR", ""),
        Harness::Opencode | Harness::Devin => return None,
    };
    sizes(&env.path(folder)?.join(under))
        .into_iter()
        .find(|(name, _)| name.contains(session))
        .map(|(_, size)| size)
}

/// Every file under `folder`, by name, with its size.
fn sizes(folder: &Path) -> Vec<(String, u64)> {
    let Ok(entries) = fs::read_dir(folder) else {
        return Vec::new();
    };
    entries
        .flatten()
        .flat_map(|entry| {
            let metadata = entry.metadata().unwrap();
            if metadata.is_dir() {
                sizes(&entry.path())
            } else {
                vec![(
                    entry.file_name().to_string_lossy().into_owned(),
                    metadata.len(),
                )]
            }
        })
        .collect()
}

/// What Node's half wrote of a reading (`digest`): each item's id, role,
/// completeness, time (none, or one), its text's UTF-16 length and SHA-256,
/// and whether it is commentary; then the reading's flags, quota and
/// settlement.
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
                item.get("at").map_or_else(|| json!([]), |at| json!([at])),
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

/// What became of each harness's conversations, and how long its looks took.
fn report(tallies: &BTreeMap<String, Tally>) -> String {
    let mut lines = vec![format!(
        "{:<12} {:>5} {:>5}   {:<26} {:<26} {:<26} {:<26} slowest",
        "harness",
        "same",
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
            "{kind:<12} {:>5} {:>5}   {:<26} {:<26} {:<26} {:<26} {slowest}",
            tally.same,
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

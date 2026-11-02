//! The long run's watch reads Claude Code's record with a reader of its own
//! (`src/long_run/record.rs`), on purpose: it holds the daemon to a reading that is not
//! the daemon's, and this crate depends on no crate of the product. A reader of its own
//! is a copy of the product's rules, and every change of the product's can leave the copy
//! behind: T-483 (a cancelled `/compact` read at rest, and `/mcp` and `/plugin` written
//! whole), T-550 (the synthetic items), and a successful `TaskStop` the product reads and
//! the watch did not (T-1069's four false breaks).
//!
//! This is how the copy is held to the product's reader. Every record the product keeps as
//! a fixture of Claude Code's (`crates/cf-harness/tests/fixtures/claude`), and the shapes
//! made here for the rules those do not hold, is read by both after each of its lines:
//! the product's reader through `harness-ask` (`claude_prefixes`, which reads the file the
//! way the dispatcher does), the watch's [`Record`] over the same lines. What a turn is
//! doing (at work, at rest, or nothing said), how many jobs it left running and whether
//! background agents run must be the same after every line. A disagreement is a failure
//! that names the first line it began at.

use std::path::Path;

use cf_evals::long_run::record::{Record, Stood};
use serde_json::{json, Value};

/// The session every fixture's `$SESSION` becomes.
const SESSION: &str = "s1";

/// What a reader says of a transcript after some of its lines: whether a turn is at work
/// (none where nothing says), how many jobs run, and whether agents do.
#[derive(Debug, PartialEq, Eq)]
struct Said {
    at_work: Option<bool>,
    jobs: usize,
    agents: bool,
}

/// What the product's reading `reading` says.
fn product(reading: &Value) -> Said {
    Said {
        at_work: match reading["settlement"]["state"].as_str() {
            Some("in-flight") => Some(true),
            Some("settled") => Some(false),
            _ => None,
        },
        jobs: usize::try_from(reading["jobsPending"].as_u64().unwrap_or(0)).unwrap(),
        agents: reading["agentsPending"].as_bool().unwrap_or(false)
            || reading["agentsPending"]
                .as_u64()
                .is_some_and(|count| count > 0),
    }
}

/// What the watch's record says.
fn watch(record: &Record) -> Said {
    Said {
        at_work: record
            .stood_at(i64::MAX)
            .map(|(stood, _)| stood == Stood::InFlight),
        jobs: record.jobs_pending_at(i64::MAX).len(),
        agents: record.agents_pending_at(i64::MAX) > 0,
    }
}

/// The product's reading after each prefix of `text`, by the number of lines.
fn product_readings(text: &str) -> Vec<Value> {
    let answered = cf_e2e::live::ask::question(&json!({
        "op": "claude_prefixes", "session": SESSION, "text": text,
    }))
    .unwrap();
    answered
        .as_array()
        .unwrap()
        .iter()
        .map(|prefix| prefix["reading"].clone())
        .collect()
}

/// The line `at` (1-based) of `text`, as the kind of record it is.
fn kind_of(text: &str, at: usize) -> String {
    let line: Value =
        serde_json::from_str(text.lines().nth(at - 1).unwrap_or("{}")).unwrap_or_default();
    let name = |field: &str| line[field].as_str().unwrap_or("").to_owned();
    match line["type"].as_str() {
        Some("system") => format!("system {}", name("subtype")),
        Some("queue-operation") => format!("queue-operation {}", name("operation")),
        Some("attachment") => format!(
            "attachment {}",
            line["attachment"]["type"].as_str().unwrap_or("")
        ),
        Some(other) => other.to_owned(),
        None => "no type".to_owned(),
    }
}

/// Where the two readers disagree about `text`, one line of words for each run of
/// prefixes they disagree over: the first line the disagreement began at and what each says.
fn disagreements(name: &str, text: &str) -> Vec<String> {
    let lines: Vec<&str> = text.lines().collect();
    let readings = product_readings(text);
    assert_eq!(
        readings.len(),
        lines.len(),
        "{name}: a reading for each line"
    );
    let mut found = Vec::new();
    let mut running: Option<(usize, Said, Said)> = None;
    for (index, reading) in readings.iter().enumerate() {
        let at = index + 1;
        let (ours, theirs) = (
            watch(&Record::claude(&lines[..at].join("\n"))),
            product(reading),
        );
        match (&running, ours == theirs) {
            (None, false) => running = Some((at, ours, theirs)),
            (Some(_), true) | (None, true) => {
                if let Some((from, ours, theirs)) = running.take() {
                    found.push(format!(
                        "{name}: from line {from} ({}) to line {}: the watch reads {ours:?}, the product {theirs:?}",
                        kind_of(text, from),
                        at - 1
                    ));
                }
            }
            (Some(_), false) => {}
        }
    }
    if let Some((from, ours, theirs)) = running {
        found.push(format!(
            "{name}: from line {from} ({}) to the end: the watch reads {ours:?}, the product {theirs:?}",
            kind_of(text, from)
        ));
    }
    found
}

/// The transcripts the product keeps as fixtures of Claude Code's, by name.
fn fixtures() -> Vec<(String, String)> {
    let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("../cf-harness/tests/fixtures/claude");
    let mut found: Vec<(String, String)> = std::fs::read_dir(&folder)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "jsonl"))
        .map(|entry| {
            (
                entry.file_name().to_string_lossy().into_owned(),
                std::fs::read_to_string(entry.path())
                    .unwrap()
                    .replace("$SESSION", SESSION),
            )
        })
        .collect();
    found.sort();
    assert!(found.len() >= 10, "the fixtures are found: {found:?}");
    found
}

#[test]
fn the_watch_reads_every_fixture_of_the_products_as_the_product_does_after_every_line() {
    let found: Vec<String> = fixtures()
        .iter()
        .flat_map(|(name, text)| disagreements(name, text))
        .collect();
    assert!(found.is_empty(), "\n{}", found.join("\n"));
}

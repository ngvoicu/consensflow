//! The launch's goldens counted against `tests/goldens/launch/coverage.json`:
//! every table and every harness's scenarios, by how many rows each holds,
//! answered by a Rust test or waiting for a landing it names.

use std::fs;
use std::path::Path;

use serde_json::Value;

fn golden(name: &str) -> Value {
    let file = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/goldens/launch")
        .join(name);
    serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap()
}

/// How many rows a table holds: a list's, or the lists' of a table of lists.
fn rows(table: &Value) -> usize {
    match table {
        Value::Array(rows) => rows.len(),
        Value::Object(parts) => parts
            .values()
            .filter_map(Value::as_array)
            .map(Vec::len)
            .sum(),
        _ => 0,
    }
}

/// Whether a manifest entry says who answers it.
fn accounted(entry: &Value) -> bool {
    entry.get("held").and_then(Value::as_str).is_some()
        || entry.get("deferred").and_then(Value::as_str).is_some()
}

#[test]
fn every_table_is_counted_and_answered_or_waits_for_a_landing() {
    let manifest = golden("coverage.json");
    let tables = golden("tables.json");
    let counted = manifest["tables"].as_object().unwrap();
    let recorded = tables.as_object().unwrap();
    let mut names: Vec<&String> = recorded.keys().collect();
    names.sort();
    let mut listed: Vec<&String> = counted.keys().collect();
    listed.sort();
    assert_eq!(
        names, listed,
        "the manifest lists every table, and no other"
    );
    for (name, table) in recorded {
        let entry = &counted[name];
        assert_eq!(entry["rows"], rows(table), "{name}");
        assert!(accounted(entry), "{name}: held or deferred");
    }
}

#[test]
fn every_harness_s_scenarios_are_counted_on_each_platform() {
    let manifest = golden("coverage.json");
    let counted = manifest["scenarios"].as_object().unwrap();
    for platform in ["darwin", "win32"] {
        let scenarios = golden(&format!("scenarios.{platform}.json"));
        let mut by_harness = std::collections::BTreeMap::<&str, u64>::new();
        for scenario in scenarios.as_array().unwrap() {
            *by_harness
                .entry(scenario["harness"].as_str().unwrap())
                .or_default() += 1;
        }
        for (harness, count) in &by_harness {
            let entry = counted
                .get(*harness)
                .unwrap_or_else(|| panic!("{harness}: not counted"));
            assert_eq!(entry[platform], *count, "{harness} on {platform}");
            assert!(accounted(entry), "{harness}: held or deferred");
        }
        assert_eq!(
            by_harness.len(),
            counted.len(),
            "{platform}: every counted harness recorded"
        );
    }
}

//! The goldens `npm run goldens:path` writes from Node, which the unit suite
//! holds equal to what Node computes now: every join and every normalize,
//! answered by both flavours as text, on whatever system this runs. A key in
//! the file that this does not read, or a case that is left out, fails the
//! counts.

// The goldens' own reading: a failure in it is the test's.
#![allow(clippy::unwrap_used)]

use cf_base::path::{posix, win32};
use serde_json::Value;

/// The segments `tests/goldens/path/goldens.mjs` builds its cases from.
const SEGMENTS: usize = 60;
/// The seeded sample of triples among its joins.
const TRIPLES: usize = 3000;
/// How many mismatches a failure shows: a wrong rule fails thousands.
const SHOWN: usize = 12;

fn golden() -> Value {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("goldens")
        .join("path.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// A case's list, by its key.
fn cases<'a>(golden: &'a Value, key: &str) -> &'a [Value] {
    golden[key].as_array().unwrap()
}

/// A case's text field.
fn text<'a>(case: &'a Value, field: &str) -> &'a str {
    case[field].as_str().unwrap()
}

/// Fails with the first mismatches, as text: `what` is the call and its input.
fn assert_no_mismatches(mismatches: &[String], checked: usize, what: &str) {
    assert!(
        mismatches.is_empty(),
        "{} of {checked} {what} differ from Node's answer; the first {}:\n{}",
        mismatches.len(),
        mismatches.len().min(SHOWN),
        mismatches
            .iter()
            .take(SHOWN)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn every_join_is_answered_as_node_answers_it_by_both_flavours() {
    let golden = golden();
    let joins = cases(&golden, "joins");
    let mut mismatches = Vec::new();
    for case in joins {
        let parts: Vec<&str> = case["parts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|part| part.as_str().unwrap())
            .collect();
        for (flavour, answer) in [
            ("posix", posix::join(&parts)),
            ("win32", win32::join(&parts)),
        ] {
            let node = text(case, flavour);
            if answer != node {
                mismatches.push(format!(
                    "{flavour}.join({parts:?}): Node {node:?}, here {answer:?}"
                ));
            }
        }
    }
    assert_no_mismatches(&mismatches, joins.len() * 2, "joins");
}

#[test]
fn every_normalize_is_answered_as_node_answers_it_by_both_flavours() {
    let golden = golden();
    let normalizes = cases(&golden, "normalizes");
    let mut mismatches = Vec::new();
    for case in normalizes {
        let path = text(case, "path");
        for (flavour, answer) in [
            ("posix", posix::normalize(path)),
            ("win32", win32::normalize(path)),
        ] {
            let node = text(case, flavour);
            if answer != node {
                mismatches.push(format!(
                    "{flavour}.normalize({path:?}): Node {node:?}, here {answer:?}"
                ));
            }
        }
    }
    assert_no_mismatches(&mismatches, normalizes.len() * 2, "normalizes");
}

#[test]
fn the_goldens_hold_every_segment_alone_every_pair_and_the_sampled_triples() {
    let golden = golden();
    let joins = cases(&golden, "joins");
    let arity = |parts: usize| {
        joins
            .iter()
            .filter(|case| case["parts"].as_array().unwrap().len() == parts)
            .count()
    };
    assert_eq!(arity(1), SEGMENTS, "segments alone");
    assert_eq!(arity(2), SEGMENTS * SEGMENTS, "ordered pairs");
    assert_eq!(arity(3), TRIPLES, "triples");
    assert_eq!(joins.len(), SEGMENTS + SEGMENTS * SEGMENTS + TRIPLES);
    assert_eq!(cases(&golden, "normalizes").len(), SEGMENTS);
    assert_eq!(golden.as_object().unwrap().len(), 2, "joins and normalizes");
}

#[test]
fn the_two_homes_a_review_found_wrong_are_among_the_goldens() {
    let golden = golden();
    let answer = |home: &str| {
        let case = cases(&golden, "joins")
            .iter()
            .find(|case| case["parts"] == serde_json::json!([home, "agents.json"]))
            .unwrap();
        (text(case, "posix"), text(case, "win32"))
    };
    assert_eq!(
        answer(r"C:..\cf"),
        (r"C:..\cf/agents.json", r"C:..\cf\agents.json")
    );
    assert_eq!(answer("C:"), ("C:/agents.json", r"C:\agents.json"));
}

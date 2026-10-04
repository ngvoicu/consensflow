//! The goldens `npm run goldens:catalog` writes from the JavaScript, which
//! the unit suite holds equal to what Node computes now: here, that each
//! reads whole, with the cases the ports of the catalog and the roster check.

// The goldens' own reading: a failure in it is the test's.
#![allow(clippy::unwrap_used)]

use serde_json::Value;

/// A golden, by its file name in `tests/goldens/`.
fn golden(name: &str) -> Value {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("goldens")
        .join(name);
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn the_catalog_golden_lists_every_harness_with_its_presets() {
    let catalog = golden("catalog.json");
    let harnesses = catalog["harnesses"].as_array().unwrap();
    assert_eq!(harnesses.len(), 5);
    let listed: usize = harnesses
        .iter()
        .map(|harness| {
            catalog["catalog"][harness.as_str().unwrap()]
                .as_array()
                .unwrap()
                .len()
        })
        .sum();
    assert_eq!(listed, 119, "every preset, under its harness");
}

#[test]
fn each_profile_case_answers_a_profile_or_a_refusal() {
    let cases = golden("profiles.json");
    let cases = cases.as_array().unwrap();
    assert_eq!(cases.len(), 1283);
    for case in cases {
        assert!(case["agent"].is_object(), "{case}");
        assert!(
            case.get("profile").is_some() != case.get("error").is_some(),
            "{case}"
        );
    }
}

#[test]
fn each_roster_case_starts_from_a_file_and_answers_once() {
    let roster = golden("roster.json");
    let documents = roster["documents"].as_object().unwrap();
    let cases = roster["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 1028);
    for case in cases {
        let from_a_document = case["document"]
            .as_str()
            .is_some_and(|document| documents.contains_key(document));
        let from_a_step = case["sequence"].is_string() && case["step"].is_u64();
        assert!(from_a_document != from_a_step, "{case}");
        assert!(
            case.get("result").is_some() != case.get("error").is_some(),
            "{case}"
        );
        assert!(
            case.get("unchanged").is_some() != case.get("after").is_some(),
            "{case}"
        );
    }
}

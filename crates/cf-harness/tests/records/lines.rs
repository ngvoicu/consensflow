//! `JSON.parse` over the bytes of a line, kept as a schema says (`tables.json`,
//! `lines`): what the reader that builds only some of a line holds of it, held
//! to what Node's parse holds of the same bytes.
//!
//! A case Node cannot parse the reader cannot either, and says it is no JSON.
//! A case Node reads and the reader does not, on purpose, is one nested past
//! 127 levels or with a number past a double's range: it fails, and says it
//! is JSON this build cannot hold. Every other case is read to the same
//! value, written as `JSON.stringify` writes it: the last of a key's
//! duplicates in the place of the first, keys in JavaScript's order, numbers
//! as the doubles Node reads, invalid UTF-8 and a lone surrogate as U+FFFD.

use std::collections::BTreeMap;

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use cf_base::js;
use cf_base::json::{from_slice_lossy_keeping, is_json_lossy, Keep};
use serde_json::Value;

use crate::scenario::goldens;

/// The schema `schema` names, as `Keep` holds one. Leaked: a test's schemas
/// live as long as the test does.
fn keep(schema: &Value) -> &'static Keep {
    let keep = match schema {
        Value::String(name) if name == "all" => Keep::All,
        Value::String(name) if name == "scalar" => Keep::Scalar,
        Value::Object(fields) if fields.contains_key("items") => {
            Keep::Items(keep(&fields["items"]))
        }
        Value::Object(fields) => {
            let members: Vec<(&'static str, &'static Keep)> = fields["members"]
                .as_object()
                .unwrap()
                .iter()
                .map(|(name, schema)| (&*Box::leak(name.clone().into_boxed_str()), keep(schema)))
                .collect();
            Keep::Members(Box::leak(members.into_boxed_slice()))
        }
        other => panic!("not a schema: {other}"),
    };
    Box::leak(Box::new(keep))
}

/// Whether two values hold their keys in one order, all the way down, and
/// their numbers as one double each.
fn alike(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Object(left), Value::Object(right)) => {
            left.len() == right.len()
                && left
                    .iter()
                    .zip(right)
                    .all(|((left_key, left), (right_key, right))| {
                        left_key == right_key && alike(left, right)
                    })
        }
        (Value::Array(left), Value::Array(right)) => {
            left.len() == right.len()
                && left
                    .iter()
                    .zip(right)
                    .all(|(left, right)| alike(left, right))
        }
        (Value::Number(left), Value::Number(right)) => left.as_f64() == right.as_f64(),
        (left, right) => left == right,
    }
}

#[test]
fn a_line_kept_by_a_schema_holds_what_node_s_json_parse_holds_of_it() {
    let file = goldens().join("tables.json");
    let tables: Value = serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap();
    let lines = &tables["lines"];
    let keeps: BTreeMap<&str, &Keep> = lines["keeps"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(name, schema)| (name.as_str(), keep(schema)))
        .collect();
    let mut counted = BTreeMap::<&str, usize>::new();
    for case in lines["cases"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let bytes = STANDARD.decode(case["bytes"].as_str().unwrap()).unwrap();
        let read = from_slice_lossy_keeping(&bytes, keeps[case["keep"].as_str().unwrap()]);
        let class = if case["node"]["throws"] == Value::Bool(true) {
            "throws"
        } else {
            case["differs"].as_str().unwrap_or("equal")
        };
        match class {
            "equal" => {
                let kept = read.unwrap_or_else(|error| panic!("{name}: {error}"));
                let node = case["node"]["kept"].as_str().unwrap();
                assert_eq!(js::stringify(&kept), node, "{name}");
                // `stringify` writes keys in JavaScript's order whatever order the
                // value holds them in: the value holds them in Node's.
                let expected: Value = serde_json::from_str(node).unwrap();
                assert!(alike(&kept, &expected), "{name}: {kept} for {node}");
            }
            "throws" => {
                assert!(read.is_err(), "{name}: Node cannot read it");
                assert!(!is_json_lossy(&bytes), "{name}: it is no JSON");
            }
            difference => {
                assert!(read.is_err(), "{name}: not read, as it is {difference}");
                assert!(
                    is_json_lossy(&bytes),
                    "{name}: it is JSON, too much for a value"
                );
            }
        }
        *counted.entry(class).or_default() += 1;
    }
    // None of the kinds is empty, so a table that shrinks or loses a kind fails here.
    assert_eq!(
        counted.into_iter().collect::<Vec<_>>(),
        [("deep", 5), ("equal", 54), ("overflow", 7), ("throws", 31)]
    );
}

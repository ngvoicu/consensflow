//! The roster's golden (`tests/goldens/roster.json`, recorded from Node's
//! roster and fixed since): every operation on each of its files at a fixed
//! time, and seeded sequences of them, as Node answered. Each case starts from
//! the same file in a home of its own, makes the same call at the same instant,
//! and is held to Node's answer or refusal as `JSON.stringify` writes it, to
//! the file it left, byte for byte, and to nothing left beside it.

// The golden's own reading: a failure in it is the test's.
#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::path::Path;

use cf_base::js;
use cf_base::refusal::Refusal;
use cf_base::time::{parse, Clock};
use cf_catalog::{Catalog, Roster};
use serde_json::{json, Map, Value};

fn golden() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("goldens")
        .join("roster.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// The golden's stand-in for what JSON cannot hold, JavaScript's `undefined`.
fn undefined() -> Value {
    json!({ "$undefined": true })
}

/// `value` as `JSON.stringify` writes it: the members that are `undefined`
/// are left out, and an item of a list that is, is `null`.
fn stringified(value: &Value) -> Value {
    match value {
        Value::Object(members) => Value::Object(
            members
                .iter()
                .filter(|(_, member)| **member != undefined())
                .map(|(key, member)| (key.clone(), stringified(member)))
                .collect(),
        ),
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| {
                    if *item == undefined() {
                        Value::Null
                    } else {
                        stringified(item)
                    }
                })
                .collect(),
        ),
        other => other.clone(),
    }
}

/// The text of the file a case starts from, none for one that starts with
/// no file: a document the golden names, or the file the step before left.
fn file_before<'a>(case: &'a Value, documents: &'a Map<String, Value>) -> Option<&'a str> {
    let before = if case["sequence"].is_string() {
        &case["before"]
    } else {
        &documents[case["document"].as_str().unwrap()]
    };
    before.as_str()
}

/// The clock at the case's instant.
struct At(i64);

impl Clock for At {
    fn now_ms(&mut self) -> i64 {
        self.0
    }
}

/// A request's body as the golden holds it: a value, or the text the API
/// read (`{"$json": text}`), read as serde_json reads a request, its keys in
/// the order written, where `JSON.parse` handed Node JavaScript's order.
/// None for a body that is not there (`{"$undefined": true}`).
fn request(value: &Value) -> Option<Value> {
    match value.get("$json") {
        Some(Value::String(text)) => Some(serde_json::from_str(text).unwrap()),
        _ if value.get("$undefined").is_some() => None,
        _ => Some(value.clone()),
    }
}

/// A request that is an object, as `addAgent` and `editAgent` are given one.
fn object(value: &Value) -> Map<String, Value> {
    match request(value) {
        Some(Value::Object(fields)) => fields,
        other => panic!("no object: {other:?}"),
    }
}

/// A value as the JSON it serializes to.
fn json<T: serde::Serialize>(value: &T) -> Value {
    serde_json::to_value(value).unwrap()
}

/// What a call answers, as JSON: `undefined` for nothing found or nothing answered.
fn answered(roster: &Roster<'_>, call: &[Value], clock: &mut At) -> Result<Value, Refusal> {
    match call[0].as_str().unwrap() {
        "listAgents" => roster.list().map(|views| json(&views)),
        "agentRow" => {
            // `{"$undefined":true}` is no name: the empty one, `String(name ?? '')`.
            let name = call[1].as_str().unwrap_or("");
            roster
                .agent_row(name)
                .map(|row| row.map_or_else(undefined, |row| json(&row)))
        }
        "preferences" => roster.preferences().map(|choices| json(&choices)),
        "setPreferences" => roster
            .set_preferences(request(&call[1]).as_ref())
            .map(|choices| json(&choices)),
        "normalizeRoster" => roster.normalize().map(Value::Bool),
        "addAgent" => roster.add(&object(&call[1]), clock).map(|view| json(&view)),
        "editAgent" => roster
            .edit(call[1].as_str().unwrap(), &object(&call[2]), clock)
            .map(|view| json(&view)),
        "removeAgent" => roster
            .remove(call[1].as_str().unwrap())
            .map(|()| undefined()),
        other => panic!("{other} is no roster operation"),
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

/// The names of what the home holds, in order.
fn left_in(home: &Path) -> Vec<String> {
    let mut left: Vec<String> = std::fs::read_dir(home)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    left.sort();
    left
}

#[test]
fn every_operation_on_the_roster_answers_as_node_answered_and_leaves_the_file_node_left() {
    let catalog = Catalog::bundled().unwrap();
    let golden = golden();
    let documents = golden["documents"].as_object().unwrap();
    let cases = golden["cases"].as_array().unwrap();
    let mut calls = BTreeMap::<String, usize>::new();
    let (mut answers, mut refusals, mut wrote, mut from_steps) = (0, 0, 0, 0);
    for case in cases {
        let call = case["call"].as_array().unwrap();
        *calls
            .entry(call[0].as_str().unwrap().to_owned())
            .or_default() += 1;
        from_steps += usize::from(case["sequence"].is_string());
        let before = file_before(case, documents);
        let home = tempfile::tempdir().unwrap();
        let path = home.path().join("agents.json");
        if let Some(text) = before {
            std::fs::write(&path, text).unwrap();
        }
        let roster = Roster::new(&catalog, path.clone());
        let mut clock = At(parse(case["at"].as_str().unwrap()).unwrap());
        match (
            answered(&roster, call, &mut clock),
            case.get("result"),
            case.get("error"),
        ) {
            (Ok(actual), Some(golden), None) => {
                // Compared as text below, where a number is written as
                // JavaScript writes it: first, the answer's own numbers are
                // ones JavaScript holds, an integer past 2^53 a double.
                assert!(
                    holds_js_numbers(&actual),
                    "a number JavaScript rounds: {case}"
                );
                assert_eq!(
                    js::stringify(&actual),
                    js::stringify(&stringified(golden)),
                    "the answer to {case}"
                );
                answers += 1;
            }
            (Err(refusal), None, Some(error)) => {
                // The file's path, whatever the platform writes it as, is `«home»/agents.json`.
                let said = refusal
                    .message
                    .replace(&path.display().to_string(), "«home»/agents.json");
                assert_eq!(Some(said.as_str()), error.as_str(), "{case}");
                assert_eq!(refusal.status, 400, "{case}");
                refusals += 1;
            }
            (answer, golden, error) => {
                panic!("{case}: answered {answer:?}, the golden has {golden:?} {error:?}")
            }
        }
        match case.get("after") {
            Some(after) => {
                assert_eq!(
                    std::fs::read_to_string(&path).unwrap(),
                    after.as_str().unwrap(),
                    "the file {case} left"
                );
                wrote += 1;
            }
            None => match before {
                Some(text) => assert_eq!(std::fs::read(&path).unwrap(), text.as_bytes(), "{case}"),
                None => assert!(!path.exists(), "{case}: no file made"),
            },
        }
        let expected: Vec<String> = if path.exists() {
            vec!["agents.json".to_owned()]
        } else {
            Vec::new()
        };
        assert_eq!(
            left_in(home.path()),
            expected,
            "{case}: nothing left beside the file"
        );
    }
    println!(
        "roster golden: {} cases ({answers} answered, {refusals} refused, {wrote} wrote the file, {from_steps} steps of sequences): {calls:?}",
        cases.len()
    );
    assert_eq!(cases.len(), answers + refusals);
    // Held, so a golden that shrinks fails.
    assert_eq!(
        calls.into_iter().collect::<Vec<_>>(),
        [
            ("addAgent".to_owned(), 479),
            ("agentRow".to_owned(), 143),
            ("editAgent".to_owned(), 500),
            ("listAgents".to_owned(), 48),
            ("normalizeRoster".to_owned(), 28),
            ("preferences".to_owned(), 28),
            ("removeAgent".to_owned(), 108),
            ("setPreferences".to_owned(), 382),
        ]
    );
    assert_eq!((answers, refusals, wrote, from_steps), (716, 1000, 523, 60));
}

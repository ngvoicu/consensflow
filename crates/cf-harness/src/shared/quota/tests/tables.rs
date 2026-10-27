//! The quota tables Node computed (`tables.json`, `quota`): `resets`,
//! `refused` and `statuses`, each answer compared as text. The others
//! (`codex`, `opencode`, `devin`) are other harnesses' own.

use cf_base::js;
use serde_json::{json, Value};

use super::*;

fn quota_tables() -> Value {
    let tables: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/goldens/records/tables.json"
    )))
    .unwrap();
    tables["quota"].clone()
}

/// A cell Node wrote `{"undefined": true}` for: `undefined` (or `null`, which
/// `??` made the same).
fn is_undefined(cell: &Value) -> bool {
    *cell == json!({ "undefined": true })
}

#[test]
fn every_reset_of_the_table_is_read_as_node_read_it_at_every_instant() {
    let quota = quota_tables();
    assert_eq!(quota["defaultZone"], "America/Los_Angeles");
    let local = local();
    // An instant is a number, or "NaN" or "Infinity" where JSON has none.
    let instants: Vec<f64> = quota["instants"]
        .as_array()
        .unwrap()
        .iter()
        .map(|at| js::to_number(Some(at)).unwrap())
        .collect();
    let rows = quota["resets"].as_array().unwrap();
    let mut cells = 0;
    for row in rows {
        let text = row["text"].as_str().unwrap();
        let expected = row["at"].as_array().unwrap();
        assert_eq!(expected.len(), instants.len(), "{text}");
        for (at_ms, expected) in instants.iter().zip(expected) {
            // `{"throws": true}`: a failure, where Node threw.
            let answer = match exhausted_quota(text, *at_ms, &local) {
                Ok(quota) => serde_json::to_value(quota).unwrap(),
                Err(_) => json!({ "throws": true }),
            };
            assert_eq!(
                js::stringify(&answer),
                js::stringify(expected),
                "{text:?} at {at_ms}"
            );
            cells += 1;
        }
    }
    assert_eq!((rows.len(), instants.len(), cells), (39, 16, 624));
}

#[test]
fn every_error_text_of_the_table_is_told_a_refusal_for_quota_as_node_told_it() {
    let rows = quota_tables()["refused"].as_array().unwrap().clone();
    for row in &rows {
        // `String(text ?? '')`.
        let text = if is_undefined(&row["text"]) {
            String::new()
        } else {
            js::text(Some(&row["text"])).into_owned()
        };
        // Node read a 402 as a quota; a billing refusal is none since MUS-23 (a
        // difference kept from Node: it has no reset, `account_refusal`).
        let expected = if account_refusal(&text, None) == Some(Refusal::Billing) {
            Value::Bool(false)
        } else {
            row["refused"].clone()
        };
        assert_eq!(
            js::stringify(&Value::Bool(refused_for_quota(&text))),
            js::stringify(&expected),
            "{text:?}"
        );
    }
    assert_eq!(rows.len(), 15);
}

#[test]
fn every_status_of_the_table_is_told_a_refusal_for_quota_as_node_told_it() {
    let rows = quota_tables()["statuses"].as_array().unwrap().clone();
    for row in &rows {
        let status = (!is_undefined(&row["status"])).then_some(&row["status"]);
        let number = js::to_number(status).unwrap();
        // Node read a 402 as a quota; it is none since MUS-23 (a difference kept from Node).
        let expected = if (number - 402.0).abs() < f64::EPSILON {
            Value::Bool(false)
        } else {
            row["quota"].clone()
        };
        assert_eq!(
            js::stringify(&Value::Bool(quota_status(number))),
            js::stringify(&expected),
            "{status:?}"
        );
    }
    assert_eq!(rows.len(), 14);
}

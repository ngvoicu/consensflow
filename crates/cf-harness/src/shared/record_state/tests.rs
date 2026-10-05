//! The `recordState` table of `tests/goldens/launch/tables.json`, each row
//! held to what Node answered. A row holds the record as JavaScript's look
//! wrote it, which a `Reading` says for all but the records that are no
//! object, that give `items` that is no list, or `failed` that is no flag:
//! the inputs Rust has no reading for.

use std::fs;
use std::path::Path;

use serde_json::{json, Value};

use super::*;
use crate::records::{Item, Quota, Record, Role};

/// The reading a row's record is, or none where no reading says it.
fn reading_of(record: &Value) -> Option<Reading> {
    // The table writes `undefined` as an object of its own.
    let fields = record
        .as_object()
        .filter(|_| *record != json!({ "undefined": true }))?;
    if fields.get("unknown") == Some(&json!(true)) {
        return Some(Reading::Unknown(fields["reason"].as_str()?.to_owned()));
    }
    let mut read = Record::new();
    for item in fields
        .get("items")
        .map_or(Some(&Vec::new()), Value::as_array)?
    {
        read.items.push(Item {
            id: Arc::from(item["id"].as_str()?),
            role: match item["role"].as_str()? {
                "user" => Role::User,
                "assistant" => Role::Assistant,
                "tool" => Role::Tool,
                _ => Role::Custom,
            },
            text: Arc::from(item["text"].as_str()?),
            complete: item["complete"].as_bool()?,
            at: None,
            commentary: false,
        });
    }
    read.in_flight = fields.get("inFlight") == Some(&json!(true));
    read.settlement = match fields
        .get("settlement")
        .and_then(|settlement| settlement["state"].as_str())
    {
        Some("settled") => Settlement::Settled,
        Some("in-flight") => Settlement::InFlight,
        _ => Settlement::Unknown,
    };
    read.failed = match fields.get("failed") {
        None => false,
        Some(flag) => flag.as_bool()?,
    };
    // The table's one quota is an exhausted one.
    read.quota = fields.get("quota").map(|quota| {
        Arc::new(Quota::Exhausted {
            at: quota["at"].as_str().map(str::to_owned),
            resets_at: quota["resetsAt"].as_str().map(str::to_owned),
        })
    });
    Some(Reading::Known(read))
}

#[test]
fn every_record_reads_as_the_state_node_read() {
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/goldens/launch/tables.json");
    let tables: Value = serde_json::from_str(&fs::read_to_string(file).unwrap()).unwrap();
    let rows = tables["recordState"].as_array().unwrap();
    assert_eq!(rows.len(), 16);
    let mut unread = Vec::new();
    for row in rows {
        let Some(reading) = reading_of(&row["record"]) else {
            unread.push(row["record"].to_string());
            continue;
        };
        let state = record_state(Arc::new(reading));
        assert_eq!(state.items(), row_items(&row["state"]).as_slice(), "{row}");
        assert_eq!(json!(state.settled), row["state"]["settled"], "{row}");
        assert_eq!(json!(state.failed), row["state"]["failed"], "{row}");
        assert_eq!(json!(state.quota), row["state"]["quota"], "{row}");
        assert_eq!(state.waiting, None, "{row}");
        assert_eq!(state.switched, None, "{row}");
        assert!(!state.unnamed, "{row}");
    }
    assert_eq!(
        unread,
        [
            // A record that is no object.
            r#"{"undefined":true}"#,
            "null",
            // `items` that is no list.
            r#"{"items":"not a list","settlement":{"state":"settled"}}"#,
            // `failed` that is no flag.
            r#"{"items":[{"id":"a","role":"assistant","text":"x","complete":true}],"failed":"yes"}"#,
        ],
        "the rows a reading cannot say"
    );
}

/// The items a row's state says.
fn row_items(state: &Value) -> Vec<Item> {
    let record = json!({ "items": state["items"] });
    match reading_of(&record) {
        Some(Reading::Known(read)) => read.items,
        _ => panic!("items that are no list"),
    }
}

#[test]
fn a_record_that_could_not_be_read_says_it_is_settled_and_empty() {
    let state = record_state(Arc::new(Reading::Unknown("unreadable: x".to_owned())));
    assert!(state.settled && !state.failed);
    assert_eq!(state.items(), []);
    assert_eq!(state.quota, None);
}

#[test]
fn a_look_names_the_record_it_read() {
    let reading = Arc::new(Reading::Known(Record::new()));
    let state = record_state(Arc::clone(&reading));
    assert!(state
        .reading
        .is_some_and(|read| Arc::ptr_eq(&read, &reading)));
}

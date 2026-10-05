//! The windows of `tests/goldens/launch/tables.json`, each as Node's
//! `hosts/lib/windows.js` built it.

use std::fs;
use std::path::Path;

use serde_json::{json, Map, Value};

use super::*;

fn tables() -> Value {
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/goldens/launch/tables.json");
    serde_json::from_str(&fs::read_to_string(file).unwrap()).unwrap()
}

/// A text of a row: none for null and for JavaScript's undefined.
fn text(value: &Value) -> Option<&str> {
    value.as_str()
}

/// A window as Node wrote it: its `env` always empty, `prompt` only where given.
fn written(window: &Invocation) -> Value {
    let mut fields = Map::new();
    fields.insert("command".to_owned(), json!(window.command));
    fields.insert("args".to_owned(), json!(window.args));
    if let Some(prompt) = &window.prompt {
        fields.insert("prompt".to_owned(), json!(prompt));
    }
    fields.insert("env".to_owned(), json!({}));
    fields.insert("dropEnv".to_owned(), json!(window.drop_env));
    Value::Object(fields)
}

#[test]
fn every_window_opens_and_resumes_as_node_built_it() {
    let tables = tables();
    let rows = tables["windows"].as_array().unwrap();
    for row in rows {
        let agent = &row["agent"];
        let harness = Harness::from_kind(agent["kind"].as_str().unwrap());
        let fields = Agent {
            model: text(&agent["model"]),
            effort: text(&agent["effort"]),
            thinking: text(&agent["thinking"]),
            designer: false,
        };
        let (session, seed) = (text(&row["sessionId"]), text(&row["seed"]));
        let window = harness.and_then(|harness| match row["call"].as_str().unwrap() {
            "start" => start(harness, fields, session, seed),
            _ => resume(harness, fields, session, seed),
        });
        assert_eq!(
            window.as_ref().map_or(Value::Null, written),
            row["window"],
            "{row}"
        );
    }
}

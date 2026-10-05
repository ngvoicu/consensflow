//! The tables of `tests/goldens/launch/tables.json`, each row held to what
//! Node answered.

use std::fs;
use std::path::Path;
use std::sync::LazyLock;

use cf_base::path::to_file_url;
use cf_base::text::{console_text, window_text};
use serde_json::Value;

static TABLES: LazyLock<Value> = LazyLock::new(|| {
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/goldens/launch/tables.json");
    serde_json::from_str(&fs::read_to_string(file).unwrap()).unwrap()
});

/// A row's text, or none where Node was given null.
fn text(value: &Value) -> Option<&str> {
    match value {
        Value::Null => None,
        text => Some(text.as_str().unwrap()),
    }
}

#[test]
fn every_text_a_window_is_given_is_the_text_node_gave() {
    let rows = TABLES["windowText"].as_array().unwrap();
    assert_eq!(rows.len(), 270);
    for row in rows {
        let given = text(&row["text"]);
        assert_eq!(
            given.map(window_text).as_deref(),
            text(&row["window"]),
            "{given:?}"
        );
    }
}

#[test]
fn every_code_point_the_console_changes_is_changed_as_node_changed_it() {
    let table = &TABLES["consoleText"];
    assert_eq!(table["unicode"], "17.0");
    let changed = table["changed"].as_array().unwrap();
    assert_eq!(changed.len(), 1999);
    let mut differ = Vec::new();
    for row in changed {
        let code = u32::try_from(row[0].as_u64().unwrap()).unwrap();
        let character = char::from_u32(code).unwrap().to_string();
        let carried = console_text(&character);
        if carried != row[1].as_str().unwrap() {
            differ.push(format!("U+{code:04X}: {carried:?}, Node {}", row[1]));
        }
    }
    assert!(
        differ.is_empty(),
        "{} differ:\n{}",
        differ.len(),
        differ.join("\n")
    );
    for row in table["texts"].as_array().unwrap() {
        let given = text(&row["text"]);
        assert_eq!(
            given.map(console_text).as_deref(),
            text(&row["console"]),
            "{given:?}"
        );
    }
}

#[test]
fn a_code_point_the_console_leaves_as_it_is_is_left_so() {
    let changed: std::collections::HashSet<u64> = TABLES["consoleText"]["changed"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row[0].as_u64().unwrap())
        .collect();
    let mut differ = Vec::new();
    for code in (0..=0x10_FFFF_u32).filter(|code| !changed.contains(&u64::from(*code))) {
        let Some(character) = char::from_u32(code) else {
            continue;
        };
        let alone = character.to_string();
        if console_text(&alone) != alone {
            differ.push(format!("U+{code:04X}"));
        }
    }
    assert!(
        differ.is_empty(),
        "{} changed here alone: {}",
        differ.len(),
        differ.join(" ")
    );
}

#[test]
fn every_path_is_the_file_url_node_wrote_for_it() {
    let rows = TABLES["fileUrl"].as_array().unwrap();
    assert_eq!(rows.len(), 267);
    for row in rows {
        let path = row["path"].as_str().unwrap();
        let windows = row["windows"].as_bool().unwrap();
        assert_eq!(
            to_file_url(path, windows).as_deref(),
            row["url"].as_str(),
            "{path:?}, windows {windows}"
        );
    }
}

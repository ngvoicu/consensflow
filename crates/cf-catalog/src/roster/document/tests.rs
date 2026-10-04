use super::*;
use crate::roster::testing::{file_with, sentence};
use serde_json::json;
use tempfile::tempdir;

/// The stored document of a file with this text, as JSON text, its keys in order.
fn stored(text: &str) -> String {
    let (_home, path) = file_with(text.as_bytes());
    Value::Object(stored_document(&path).unwrap()).to_string()
}

/// The document of a file whose agents are these rows' text.
fn loaded(rows: &str) -> Result<Document, Refusal> {
    let (_home, path) = file_with(format!(r#"{{"schemaVersion":1,"agents":[{rows}]}}"#).as_bytes());
    load_document(&path)
}

/// The rows of a loaded document, as JSON text.
fn rows_of(document: &Document) -> Vec<String> {
    document
        .agents()
        .iter()
        .map(|row| serde_json::to_string(row).unwrap())
        .collect()
}

fn refusal_of(row: &str) -> Refusal {
    let (_home, path) = file_with(format!(r#"{{"agents":[{row}]}}"#).as_bytes());
    load_document(&path).unwrap_err()
}

#[test]
fn no_file_is_the_version_and_an_empty_list_in_that_order() {
    let home = tempdir().unwrap();
    let path = home.path().join("agents.json");
    let document = stored_document(&path).unwrap();
    assert_eq!(
        Value::Object(document).to_string(),
        r#"{"schemaVersion":1,"agents":[]}"#
    );
    assert!(!path.exists(), "reading it made nothing");
}

#[test]
fn an_empty_object_is_given_the_version_then_the_list() {
    assert_eq!(stored("{}"), r#"{"schemaVersion":1,"agents":[]}"#);
}

#[test]
fn keys_already_there_keep_their_place() {
    assert_eq!(
        stored(r#"{"agents":[],"note":"kept","schemaVersion":3}"#),
        r#"{"agents":[],"note":"kept","schemaVersion":3}"#
    );
}

#[test]
fn a_missing_version_is_appended_and_then_a_missing_list() {
    assert_eq!(
        stored(r#"{"note":"kept"}"#),
        r#"{"note":"kept","schemaVersion":1,"agents":[]}"#
    );
    assert_eq!(
        stored(r#"{"agents":[],"note":"kept"}"#),
        r#"{"agents":[],"note":"kept","schemaVersion":1}"#
    );
    assert_eq!(
        stored(r#"{"schemaVersion":1,"note":"kept"}"#),
        r#"{"schemaVersion":1,"note":"kept","agents":[]}"#
    );
}

#[test]
fn a_null_version_becomes_1_in_its_place() {
    assert_eq!(
        stored(r#"{"schemaVersion":null,"agents":[],"note":1}"#),
        r#"{"schemaVersion":1,"agents":[],"note":1}"#
    );
    assert_eq!(
        stored(r#"{"note":1,"agents":[],"schemaVersion":null}"#),
        r#"{"note":1,"agents":[],"schemaVersion":1}"#
    );
}

#[test]
fn a_version_that_is_not_null_is_kept_whatever_it_is() {
    for version in ["2", "0", r#""1""#, "false", "[]", "{}"] {
        let text = format!(r#"{{"schemaVersion":{version},"agents":[]}}"#);
        assert_eq!(stored(&text), text);
    }
}

#[test]
fn agents_that_are_no_list_become_an_empty_list_in_their_place() {
    for agents in [r#""x""#, "null", "5", "true", r#"{"nova":{"id":"nova"}}"#] {
        assert_eq!(
            stored(&format!(r#"{{"agents":{agents},"other":1}}"#)),
            r#"{"agents":[],"other":1,"schemaVersion":1}"#,
            "{agents}"
        );
    }
}

#[test]
fn the_keys_that_are_indices_come_first_and_the_rest_as_written() {
    assert_eq!(
        stored(r#"{"schemaVersion":1,"note":"kept","agents":[],"7":"x","2":"y"}"#),
        r#"{"2":"y","7":"x","schemaVersion":1,"note":"kept","agents":[]}"#
    );
}

#[test]
fn the_stored_document_leaves_every_row_as_it_is() {
    // Rows are checked and the image agent folded when the document is
    // loaded, not before.
    let text = r#"{"schemaVersion":1,"agents":[5,null,{"id":7},{"kind":"image"}]}"#;
    assert_eq!(stored(text), text);
}

#[test]
fn a_file_that_cannot_be_used_is_refused_by_the_stored_document_too() {
    let (_home, path) = file_with(b"[1,2]");
    let refusal = stored_document(&path).unwrap_err();
    assert_eq!(refusal.message, sentence(&path, "is not an agents file"));
}

#[test]
fn an_image_row_becomes_a_codex_row_with_the_designer_flag_last() {
    let document =
        loaded(r#"{"id":"draw","kind":"image","model":"gpt-image-2","description":"d"}"#).unwrap();
    assert_eq!(
        rows_of(&document),
        [r#"{"id":"draw","kind":"codex","model":"gpt-image-2","description":"d","designer":true}"#]
    );
}

#[test]
fn an_image_row_keeps_its_kind_and_its_designer_flag_where_they_were() {
    let document = loaded(
        r#"{"designer":false,"id":"draw","model":"m","kind":"image"},
           {"id":"other","designer":"no","kind":"image"}"#,
    )
    .unwrap();
    assert_eq!(
        rows_of(&document),
        [
            r#"{"designer":true,"id":"draw","model":"m","kind":"codex"}"#,
            r#"{"id":"other","designer":true,"kind":"codex"}"#,
        ]
    );
}

#[test]
fn a_row_that_is_not_an_image_row_is_left_as_it_is() {
    let rows = [
        r#"{"id":"a","kind":"codex","model":"m"}"#,
        r#"{"id":"b","kind":"Image","designer":"no"}"#,
        r#"{"id":"c","kind":"image ","model":"m"}"#,
        r#"{"id":"d","model":"m"}"#,
    ];
    let document = loaded(&rows.join(",")).unwrap();
    assert_eq!(rows_of(&document), rows);
}

#[test]
fn every_other_field_is_kept_whatever_it_holds() {
    let row = r#"{"id":"nova","name":7,"kind":"codex","designer":"no","workTier":3,"description":{"any":[1,null]},"custom":false,"createdAt":null,"colour":"green","preset":null}"#;
    let document = loaded(row).unwrap();
    assert_eq!(rows_of(&document), [row]);
}

#[test]
fn the_work_tier_is_not_checked_while_rows_load() {
    let document = loaded(r#"{"id":"nova","kind":"codex","workTier":"huge"}"#).unwrap();
    assert_eq!(document.agents()[0].get("workTier"), Some(&json!("huge")));
}

#[test]
fn the_rows_come_in_the_files_order_with_their_keys_in_javascripts_order() {
    let document = loaded(r#"{"id":"b","7":1,"kind":"codex"},{"id":"a","2":1}"#).unwrap();
    assert_eq!(
        rows_of(&document),
        [r#"{"7":1,"id":"b","kind":"codex"}"#, r#"{"2":1,"id":"a"}"#,]
    );
}

#[test]
fn a_file_with_no_rows_loads_with_none() {
    let home = tempdir().unwrap();
    let document = load_document(&home.path().join("agents.json")).unwrap();
    assert!(document.agents().is_empty());
    assert_eq!(
        Value::Object(document.fields().clone()).to_string(),
        r#"{"schemaVersion":1,"agents":[]}"#
    );
}

#[test]
fn a_document_keeps_the_keys_the_file_carried_beside_its_agents() {
    let (_home, path) = file_with(
        br#"{"note":"kept","agents":[{"id":"a"}],"preferences":{"ownHarnessOnly":true},"2":"index"}"#,
    );
    let document = load_document(&path).unwrap();
    assert_eq!(
        Value::Object(document.fields().clone()).to_string(),
        r#"{"2":"index","note":"kept","agents":[{"id":"a"}],"preferences":{"ownHarnessOnly":true},"schemaVersion":1}"#
    );
}

#[test]
fn a_row_that_is_no_object_refuses_the_file_as_no_agents_file() {
    for row in ["null", "5", r#""nova""#, "true", "[]", r#"[{"id":"nova"}]"#] {
        let (_home, path) = file_with(format!(r#"{{"agents":[{row}]}}"#).as_bytes());
        let refusal = load_document(&path).unwrap_err();
        assert_eq!(
            refusal.message,
            sentence(&path, "is not an agents file"),
            "{row}"
        );
        assert_eq!(
            (refusal.code, refusal.status),
            ("agents-file-unreadable", 400),
            "{row}"
        );
    }
}

#[test]
fn an_id_a_kind_or_a_model_that_is_present_and_no_text_refuses_the_file() {
    for field in ["id", "kind", "model"] {
        for value in ["null", "5", "false", "[]", r#"{"a":1}"#] {
            let row = format!(r#"{{"id":"nova","{field}":{value}}}"#);
            let refusal = refusal_of(&row);
            assert!(
                refusal.message.contains("is not an agents file: fix it"),
                "{row}"
            );
            assert_eq!(
                (refusal.code, refusal.status),
                ("agents-file-unreadable", 400),
                "{row}"
            );
        }
    }
}

#[test]
fn a_preset_a_harness_an_effort_or_a_thinking_that_is_set_and_no_text_refuses_the_file() {
    for field in ["preset", "harness", "effort", "thinking"] {
        for value in ["3", "true", "[]", r#"{"a":1}"#] {
            let row = format!(r#"{{"id":"nova","{field}":{value}}}"#);
            let refusal = refusal_of(&row);
            assert!(
                refusal.message.contains("is not an agents file: fix it"),
                "{row}"
            );
            assert_eq!(
                (refusal.code, refusal.status),
                ("agents-file-unreadable", 400),
                "{row}"
            );
        }
    }
}

#[test]
fn a_preset_a_harness_an_effort_or_a_thinking_that_is_null_is_as_good_as_absent() {
    let row = r#"{"id":"nova","kind":"codex","preset":null,"harness":null,"effort":null,"thinking":null}"#;
    assert_eq!(rows_of(&loaded(row).unwrap()), [row]);
}

#[test]
fn one_row_of_the_wrong_shape_among_good_ones_refuses_the_whole_file() {
    let (_home, path) = file_with(
        br#"{"agents":[{"id":"a","kind":"codex"},{"id":"b","kind":"codex"},{"id":"c","kind":3}]}"#,
    );
    let refusal = load_document(&path).unwrap_err();
    assert_eq!(refusal.message, sentence(&path, "is not an agents file"));
}

#[test]
fn the_shape_of_a_row_is_checked_before_the_image_agent_is_folded() {
    let refusal = refusal_of(r#"{"id":5,"kind":"image"}"#);
    assert!(refusal.message.contains("is not an agents file"));
}

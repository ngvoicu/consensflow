//! The `openCodeUrls` of `tests/goldens/launch/tables.json`: each URL as
//! Node's `new URL` wrote it.

use serde_json::Value;

use super::*;

/// The origin the table's URLs were made for.
const ENDPOINT: &str = "http://127.0.0.1:41001";

fn table() -> Vec<Value> {
    let tables: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/goldens/launch/tables.json"
    )))
    .unwrap();
    tables["openCodeUrls"].as_array().unwrap().clone()
}

#[test]
fn every_folder_of_the_table_is_written_into_each_url_as_node_wrote_it() {
    let rows = table();
    for row in &rows {
        let (session_id, directory) = (
            row["session"].as_str().unwrap(),
            row["directory"].as_str().unwrap(),
        );
        assert_eq!(
            session(ENDPOINT, session_id, directory),
            row["settings"],
            "{row}"
        );
        assert_eq!(
            prompt(ENDPOINT, session_id, directory),
            row["prompt"],
            "{row}"
        );
        assert_eq!(creation(ENDPOINT, directory), row["creation"], "{row}");
    }
    assert_eq!(rows.len(), 40);
}

#[test]
fn a_form_writes_a_space_as_a_plus_and_the_component_as_a_percent_with_a_quote_escaped_at_last() {
    let directory = "/work/my app/it's (ok)!~*";
    assert_eq!(
        prompt(ENDPOINT, "ses_a", directory),
        "http://127.0.0.1:41001/session/ses_a/prompt_async?directory=%2Fwork%2Fmy+app%2Fit%27s+%28ok%29%21%7E*"
    );
    assert_eq!(
        creation(ENDPOINT, directory),
        "http://127.0.0.1:41001/session?directory=%2Fwork%2Fmy%20app%2Fit%27s%20(ok)!~*"
    );
}

#[test]
fn encode_uri_component_keeps_letters_digits_and_the_marks_and_writes_the_rest_as_utf_8_percents() {
    assert_eq!(component("aZ09-_.!~*'()"), "aZ09-_.!~*'()");
    assert_eq!(
        component("a b/c?d#e%f+g&h=i"),
        "a%20b%2Fc%3Fd%23e%25f%2Bg%26h%3Di"
    );
    assert_eq!(component("caf\u{e9}\u{1F600}"), "caf%C3%A9%F0%9F%98%80");
    assert_eq!(component(""), "");
}

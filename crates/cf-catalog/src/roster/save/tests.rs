use super::*;
use crate::roster::document::load_document;
use crate::roster::testing::file_with;
use std::fs;

#[test]
fn a_save_writes_two_space_json_and_a_line_break_with_no_stale_field_in_any_row() {
    let (_home, path) = file_with(
        br#"{"schemaVersion":1.0,"agents":[{"id":"nova","skills":["a"],"profile":{"modelKey":"k"},"x":1e21},{"id":"pip","skillPath":"p","skillsPolicy":"q","skillPaths":[]}],"note":{}}"#,
    );
    let mut document = load_document(&path).unwrap();
    save_document(&path, &mut document).unwrap();
    // JSON.stringify(document, null, 2) + '\n', as Node 26 writes it.
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "{\n  \"schemaVersion\": 1,\n  \"agents\": [\n    {\n      \"id\": \"nova\",\n      \"x\": 1e+21\n    },\n    {\n      \"id\": \"pip\"\n    }\n  ],\n  \"note\": {}\n}\n"
    );
}

#[test]
fn a_save_makes_the_folder_and_leaves_nothing_beside_the_file() {
    let home = tempfile::tempdir().unwrap();
    let path = home.path().join("consensflow").join("agents.json");
    let mut document = load_document(&path).unwrap();
    save_document(&path, &mut document).unwrap();
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "{\n  \"schemaVersion\": 1,\n  \"agents\": []\n}\n"
    );
    let left: Vec<_> = fs::read_dir(path.parent().unwrap())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(left, ["agents.json"]);
}

#[test]
fn a_save_that_cannot_write_says_the_failure_by_nodes_name() {
    let home = tempfile::tempdir().unwrap();
    // The folder the file should go in is a file.
    fs::write(home.path().join("consensflow"), "a file").unwrap();
    let path = home.path().join("consensflow").join("agents.json");
    let mut document = load_document(&path)
        .unwrap_or_else(|_| load_document(&home.path().join("none").join("agents.json")).unwrap());
    let refusal = save_document(&path, &mut document).unwrap_err();
    assert_eq!(
        (refusal.code, refusal.status),
        ("agents-file-unwritable", 400)
    );
    let (name, _) = refusal.message.split_once(": ").unwrap();
    assert!(
        name.starts_with('E') && name.chars().all(|c| c.is_ascii_uppercase()),
        "{}",
        refusal.message
    );
    assert_eq!(
        fs::read_to_string(home.path().join("consensflow")).unwrap(),
        "a file"
    );
}

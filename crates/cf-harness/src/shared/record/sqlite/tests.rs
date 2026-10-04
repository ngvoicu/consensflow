use super::*;
use serde_json::json;
use std::path::PathBuf;

/// A store in a folder of its own, written by a connection of the test's.
fn store_with(sql: &str) -> (tempfile::TempDir, PathBuf, Connection) {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("store.db");
    let writer = Connection::open(&file).unwrap();
    writer.execute_batch(sql).unwrap();
    (dir, file, writer)
}

#[test]
fn a_row_is_an_object_of_its_columns_as_javascript_held_it() {
    let (_dir, file, _writer) = store_with(
        "create table t (a integer, b real, c text, d integer);
         insert into t values (9007199254740991, 1.5, 'x', null);
         insert into t values (-2, 0.25, 'y', 7);",
    );
    let store = Store::open(&file).unwrap();
    let rows = store
        .read(|reads| reads.all("select a, b, c, d, 1 as d from t order by a desc", []))
        .unwrap();
    // Node: a later column of a name holds the name's place and its own value.
    assert_eq!(
        serde_json::to_string(&rows).unwrap(),
        r#"[{"a":9007199254740991,"b":1.5,"c":"x","d":1},{"a":-2,"b":0.25,"c":"y","d":1}]"#
    );
    let none = store
        .read(|reads| reads.get("select a from t where a = ?", [3]))
        .unwrap();
    assert_eq!(none, None);
}

#[test]
fn an_integer_past_a_number_and_a_blob_fail_the_read() {
    let (_dir, file, _writer) = store_with(
        "create table t (a integer, b blob);
         insert into t values (9007199254740993, x'0102');",
    );
    let store = Store::open(&file).unwrap();
    // Node: ERR_OUT_OF_RANGE for the integer, a Uint8Array for the blob.
    assert!(store
        .read(|reads| reads.all("select a from t", []))
        .is_err());
    assert!(store
        .read(|reads| reads.all("select b from t", []))
        .is_err());
}

#[test]
fn a_store_is_read_and_never_written_nor_waited_on() {
    let (_dir, file, writer) = store_with("create table t (a integer); insert into t values (1);");
    let store = Store::open(&file).unwrap();
    let written = store.read(|reads| reads.all("insert into t values (2)", []));
    assert!(written.is_err(), "read-only");
    // A writer holding the store: the read fails at once, as with no busy timeout.
    writer.execute_batch("begin exclusive").unwrap();
    let started = std::time::Instant::now();
    assert!(store
        .read(|reads| reads.all("select a from t", []))
        .is_err());
    assert!(started.elapsed() < Duration::from_secs(1));
    writer.execute_batch("rollback").unwrap();
    assert_eq!(
        store
            .read(|reads| reads.all("select a from t", []))
            .unwrap()
            .len(),
        1
    );
    assert!(Store::open(&file.with_file_name("none.db")).is_err());
}

#[test]
fn one_read_sees_one_state_of_the_store() {
    let (_dir, file, writer) = store_with(
        "pragma journal_mode = wal; create table t (a integer); insert into t values (1);",
    );
    let store = Store::open(&file).unwrap();
    let counts = store
        .read(|reads| {
            let before = reads.all("select a from t", [])?.len();
            writer.execute_batch("insert into t values (2)").unwrap();
            Ok((before, reads.all("select a from t", [])?.len()))
        })
        .unwrap();
    assert_eq!(counts, (1, 1), "the write after the read began is not seen");
    assert_eq!(
        store
            .read(|reads| reads.all("select a from t", []))
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn what_a_read_gave_is_bound_as_node_binds_the_javascript_value() {
    let (_dir, file, _writer) =
        store_with("create table t (i integer, x text); insert into t values (1, 'a');");
    let store = Store::open(&file).unwrap();
    let row = store
        .read(|reads| reads.get("select i from t", []))
        .unwrap()
        .unwrap();
    // Node bound the number 1 as a double: an integer column finds it, and
    // text made of it is `1.0`.
    let found = store
        .read(|reads| {
            reads.get(
                "select x, cast(? as text) as written from t where i = ?",
                [bound(&row["i"]), bound(&row["i"])],
            )
        })
        .unwrap()
        .unwrap();
    assert_eq!(
        (&found["x"], &found["written"]),
        (&json!("a"), &json!("1.0"))
    );
    assert_eq!(bound(&json!("x")), Bound::Text("x".to_owned()));
    assert_eq!(bound(&Value::Null), Bound::Null);
}

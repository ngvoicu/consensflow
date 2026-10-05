//! Columns of a kind Devin's own store never holds, read as Node read them:
//! a blob or an infinity where Devin writes a time, row ids of mixed kinds,
//! and a blob where Devin names a node.

use super::*;

/// A store of `schema`, with a session `calm-river` headed by `head`
/// (SQL), under a root of its own.
fn staged_with(schema: &str, head: &str) -> (tempfile::TempDir, Env, Connection) {
    let root = tempfile::tempdir().unwrap();
    let under = |rest: &str| root.path().join(rest).to_string_lossy().into_owned();
    let env = Env::from_vars([
        ("HOME", under("")),
        ("XDG_DATA_HOME", under("data")),
        ("APPDATA", under("data")),
        ("CONSENSFLOW_HOME", under("home")),
    ]);
    fs::create_dir_all(root.path().join("data/devin/cli")).unwrap();
    let store = Connection::open(root.path().join("data/devin/cli/sessions.db")).unwrap();
    store.execute_batch(schema).unwrap();
    store
        .execute(
            &format!("INSERT INTO sessions VALUES (?, {head})"),
            [SESSION],
        )
        .unwrap();
    (root, env, store)
}

#[test]
fn a_time_of_a_kind_no_message_holds_is_read_as_node_read_it() {
    let schema = "CREATE TABLE sessions (id TEXT PRIMARY KEY, main_chain_id TEXT);
        CREATE TABLE message_nodes (row_id INTEGER PRIMARY KEY, session_id TEXT, node_id TEXT,
          parent_node_id TEXT, chat_message TEXT, created_at INTEGER NOT NULL)";
    // Node 26: the message's own time first; else the column, as JSON writes it.
    for (created_at, named, at) in [
        ("x'0102'", true, "123"),
        ("1e999", true, "123"),
        ("x'0102'", false, r#"{"0":1,"1":2}"#),
        ("1e999", false, "null"),
        ("-1e999", false, "null"),
    ] {
        let (_root, env, store) = staged_with(schema, "'n-1'");
        let mut message = json!({
            "message_id": "m",
            "role": "assistant",
            "content": "done",
            "metadata": { "finish_reason": "stop" },
        });
        if named {
            message["metadata"]["created_at"] = json!(123);
        }
        store
            .execute(
                &format!(
                    "INSERT INTO message_nodes (session_id, node_id, parent_node_id, chat_message, created_at)
                     VALUES (?, 'n-1', NULL, ?, {created_at})"
                ),
                (SESSION, message.to_string()),
            )
            .unwrap();
        assert_eq!(
            show(&look(&mut reader(SESSION, &env))),
            format!(
                r#"{{"items":[["m","assistant","done",true,{at}]],"inFlight":false,"asking":false,"failed":false,"settlement":"settled"}}"#
            ),
            "{created_at}, named: {named}"
        );
    }
}

#[test]
fn row_ids_of_mixed_kinds_are_compared_as_javascript_compares_them() {
    let (_root, env, store) = staged_with(
        "CREATE TABLE sessions (id TEXT PRIMARY KEY, main_chain_id);
         CREATE TABLE message_nodes (row_id, session_id, node_id, parent_node_id, chat_message, created_at)",
        "NULL",
    );
    let tool = json!({ "message_id": "t", "role": "tool", "content": "x" });
    let asked = json!({
        "message_id": "q",
        "role": "assistant",
        "content": "x",
        "tool_calls": [{ "name": "ask_user_question", "id": "q" }],
    });
    // 2^57 as a real, and 2^57 + 32 as hexadecimal text: the text is newer.
    for (id, node, message) in [
        ("144115188075855872.0", "a", tool),
        ("'0x200000000000011'", "b", asked),
    ] {
        store
            .execute(
                &format!("INSERT INTO message_nodes VALUES ({id}, ?, ?, NULL, ?, 1)"),
                (SESSION, node, message.to_string()),
            )
            .unwrap();
    }
    assert_eq!(
        show(&look(&mut reader(SESSION, &env))),
        r#"{"items":[],"inFlight":false,"asking":true,"failed":false,"settlement":"unknown"}"#
    );
}

#[test]
fn a_blob_names_no_node_another_blob_names() {
    let (_root, env, store) = staged_with(
        "CREATE TABLE sessions (id TEXT PRIMARY KEY, main_chain_id);
         CREATE TABLE message_nodes (row_id INTEGER PRIMARY KEY, session_id TEXT, node_id,
           parent_node_id, chat_message TEXT, created_at)",
        "x'01'",
    );
    store
        .execute(
            "INSERT INTO message_nodes (session_id, node_id, parent_node_id, chat_message, created_at)
             VALUES (?, x'01', NULL, ?, 1)",
            (
                SESSION,
                json!({ "message_id": "u", "role": "user", "content": "hi" }).to_string(),
            ),
        )
        .unwrap();
    assert_eq!(
        show(&look(&mut reader(SESSION, &env))),
        "unknown: unreadable: missing Devin main chain ancestor"
    );
}

#[test]
fn a_blob_read_in_another_look_is_another_object() {
    let (_root, env, store) = staged_with(
        "CREATE TABLE sessions (id TEXT PRIMARY KEY, main_chain_id);
         CREATE TABLE message_nodes (row_id INTEGER PRIMARY KEY, session_id TEXT, node_id,
           parent_node_id, chat_message TEXT, created_at)",
        "'a'",
    );
    for (node, parent, message) in [
        (
            "'a'",
            "NULL",
            json!({ "message_id": "u", "role": "user", "content": "hi" }),
        ),
        (
            "x'01'",
            "'a'",
            json!({ "message_id": "r", "role": "assistant", "content": "yes" }),
        ),
    ] {
        store
            .execute(
                &format!(
                    "INSERT INTO message_nodes (session_id, node_id, parent_node_id, chat_message, created_at)
                     VALUES (?, {node}, {parent}, ?, 1)"
                ),
                (SESSION, message.to_string()),
            )
            .unwrap();
    }
    let mut reader = reader(SESSION, &env);
    let first = look(&mut reader);
    assert_eq!(
        show(&first),
        r#"{"items":[["u","user","hi",true,1]],"inFlight":false,"asking":false,"failed":false,"settlement":"settled"}"#
    );
    // Its row read again holds another blob: rewritten, as Node saw it.
    assert!(!Arc::ptr_eq(&first, &look(&mut reader)));
    store
        .execute("UPDATE sessions SET main_chain_id = x'01'", [])
        .unwrap();
    assert_eq!(
        show(&look(&mut reader)),
        "unknown: unreadable: missing Devin main chain ancestor"
    );
}

#[test]
fn a_column_a_table_names_in_another_case_is_none_as_javascript_reads_it() {
    let schema = |columns: &str| {
        format!(
            "CREATE TABLE sessions (id TEXT PRIMARY KEY, main_chain_id TEXT);
             CREATE TABLE message_nodes ({columns})"
        )
    };
    let staged = |columns: &str| {
        let (root, env, store) = staged_with(&schema(columns), "'n'");
        store
            .execute(
                "INSERT INTO message_nodes VALUES (1, ?, 'n', NULL, ?, 7)",
                (
                    SESSION,
                    json!({ "message_id": "m", "role": "user", "content": "hi" }).to_string(),
                ),
            )
            .unwrap();
        (root, env)
    };
    // Node 26: the row's columns are named as the table declares them, so
    // `row.created_at` is undefined, and the item has no time.
    let (_root, env) = staged(
        "row_id INTEGER PRIMARY KEY, session_id TEXT, node_id TEXT, parent_node_id TEXT,
         chat_message TEXT, CREATED_AT INTEGER",
    );
    let mut looks = reader(SESSION, &env);
    for _ in 0..2 {
        assert_eq!(
            serde_json::to_string(&*look(&mut looks)).unwrap(),
            r#"{"items":[{"id":"m","role":"user","text":"hi","complete":true}],"inFlight":false,"asking":false,"failed":false,"quota":null,"settlement":{"state":"settled"}}"#
        );
    }
    // `row.row_id` undefined: read once, then bound to the next query, where
    // Node threw ("Provided value cannot be bound to SQLite parameter 2.").
    let (_root, env) = staged(
        "ROW_ID INTEGER PRIMARY KEY, session_id TEXT, node_id TEXT, parent_node_id TEXT,
         chat_message TEXT, created_at INTEGER",
    );
    let mut looks = reader(SESSION, &env);
    assert_eq!(
        serde_json::to_string(&*look(&mut looks)).unwrap(),
        r#"{"items":[{"id":"m","role":"user","text":"hi","complete":true,"at":7}],"inFlight":false,"asking":false,"failed":false,"quota":null,"settlement":{"state":"settled"}}"#
    );
    assert_eq!(
        show(&look(&mut looks)),
        "unknown: unreadable: provided value cannot be bound to SQLite parameter 2"
    );
    // `row.chat_message` undefined: `JSON.parse(undefined)` threw.
    let (_root, env) = staged(
        "row_id INTEGER PRIMARY KEY, session_id TEXT, node_id TEXT, parent_node_id TEXT,
         CHAT_MESSAGE TEXT, created_at INTEGER",
    );
    assert_eq!(
        show(&look(&mut reader(SESSION, &env))),
        "unknown: unreadable: Devin's message at row 1 is no JSON"
    );
}

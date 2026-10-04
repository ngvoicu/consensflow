use super::*;
use cf_base::json::DEEPEST;
use std::sync::Arc;

/// A row of node `node` below `parent`, its id `id` and its message `message`.
fn row(id: f64, node: &str, parent: &str, message: Cell, keys: &mut Keys) -> Row {
    let text = |text: &str| Cell::Text(text.to_owned());
    Row::of(
        Some(Cell::Number(id)),
        Some(&text(node)),
        Some(&text(parent)),
        Some(message),
        Some(text("2026-10-04T10:00:00Z")),
        keys,
    )
}

#[test]
fn of_rows_below_a_node_the_newest_has_the_greatest_id_and_the_first_read_of_equals() {
    let mut keys = Keys::default();
    let mut store = Store::empty();
    let empty = || Cell::Text("{}".to_owned());
    store.set(row(5.0, "a", "p", empty(), &mut keys));
    store.set(row(5.0, "b", "p", empty(), &mut keys));
    store.set(row(4.0, "c", "p", empty(), &mut keys));
    store.set(row(9.0, "d", "q", empty(), &mut keys));
    let text = |text: &str| Key::Text(Arc::from(text));
    let newest = |parent: &str| {
        store
            .newest_child(&text(parent))
            .map(|row| row.node.clone())
    };
    assert_eq!(newest("p"), Some(text("a")));
    assert_eq!(newest("none"), None);
    // A node read again keeps its place, with the row read last.
    store.set(row(6.0, "c", "p", empty(), &mut keys));
    assert_eq!(store.rows.len(), 4);
    assert!(store
        .newest_child(&text("p"))
        .is_some_and(|row| same_of(row.id.as_ref(), Some(&Cell::Number(6.0)))));
}

#[test]
fn a_message_is_parsed_once_and_what_is_no_json_is_told_from_what_cannot_be_held() {
    let mut keys = Keys::default();
    let mut message = |message: Cell| row(1.0, "a", "p", message, &mut keys);
    let user = message(Cell::Text(r#"{"role":"user"}"#.to_owned()));
    let parsed = user.message().unwrap();
    assert_eq!(parsed, &serde_json::json!({ "role": "user" }));
    assert!(std::ptr::eq(parsed, user.message().unwrap()), "parsed once");
    // `JSON.parse(String(value))`: a number and null are JSON as they are.
    assert_eq!(
        message(Cell::Number(5.0)).message().unwrap(),
        &serde_json::json!(5)
    );
    assert_eq!(message(Cell::Null).message().unwrap(), &Value::Null);
    for no_json in [
        Cell::Text("{bad".to_owned()),
        Cell::Bytes(Arc::from(&[1, 2][..])),
        Cell::Number(f64::INFINITY),
    ] {
        assert_eq!(
            message(no_json).message().unwrap_err(),
            "Devin's message at row 1 is no JSON"
        );
    }
    let deep = format!("{}{}", "[".repeat(DEEPEST + 1), "]".repeat(DEEPEST + 1));
    assert!(message(Cell::Text(deep))
        .message()
        .unwrap_err()
        .starts_with("Devin's message at row 1 is JSON this build cannot hold"));
}

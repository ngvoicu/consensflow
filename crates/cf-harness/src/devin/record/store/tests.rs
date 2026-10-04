use super::*;
use serde_json::json;

/// A row of the columns a query reads, its message `message`.
fn row(id: Value, node: &str, parent: &str, message: Value, keys: &mut Keys) -> Row {
    let columns = json!({
        "row_id": id,
        "node_id": node,
        "parent_node_id": parent,
        "chat_message": message,
        "created_at": "2026-10-04T10:00:00Z",
    });
    Row::new(serde_json::from_value(columns).unwrap(), keys)
}

#[test]
fn row_ids_compare_as_javascript_compares_them() {
    // Node 26's `left > right`.
    let cases = [
        (json!("10"), json!("9"), false),
        // By UTF-16 code units: U+FF61 after the surrogates of U+1F600.
        (json!("\u{FF61}"), json!("\u{1F600}"), true),
        (json!(10), json!(9), true),
        (json!("10"), json!(9), true),
        (Value::Null, json!(-1), true),
        (json!("x"), json!(1), false),
        (json!(1), json!("x"), false),
        (json!(1), Value::Null, true),
        (json!(""), Value::Null, false),
        (json!(" 2 "), json!(1), true),
        (json!("0x10"), json!(15), true),
        (json!(2.5), json!(2), true),
    ];
    for (left, right, greater_than) in cases {
        assert_eq!(greater(&left, &right), greater_than, "{left} > {right}");
    }
}

#[test]
fn columns_are_the_same_as_strict_equality_says() {
    let real_one: Value = serde_json::from_str("1.0").unwrap();
    assert!(same(&json!(1), &real_one));
    assert!(!same(&json!("1"), &json!(1)));
    assert!(same(&Value::Null, &Value::Null));
    assert!(!same(&Value::Null, &json!("")));
}

#[test]
fn of_rows_below_a_node_the_newest_has_the_greatest_id_and_the_first_read_of_equals() {
    let mut keys = Keys::default();
    let mut store = Store::empty();
    store.set(row(json!(5), "a", "p", json!("{}"), &mut keys));
    store.set(row(json!(5), "b", "p", json!("{}"), &mut keys));
    store.set(row(json!(4), "c", "p", json!("{}"), &mut keys));
    store.set(row(json!(9), "d", "q", json!("{}"), &mut keys));
    let newest = |parent: &str, keys: &mut Keys| {
        store
            .newest_child(&keys.of(Some(&json!(parent))))
            .map(|row| row.node.clone())
    };
    assert_eq!(newest("p", &mut keys), Some(keys.of(Some(&json!("a")))));
    assert_eq!(newest("none", &mut keys), None);
    // A node read again keeps its place, with the row read last.
    store.set(row(json!(6), "c", "p", json!("{}"), &mut keys));
    assert_eq!(store.rows.len(), 4);
    assert_eq!(
        store
            .newest_child(&keys.of(Some(&json!("p"))))
            .map(|row| &row.id),
        Some(&json!(6))
    );
}

#[test]
fn a_message_is_parsed_once_and_what_is_no_json_is_told_from_what_cannot_be_held() {
    let mut keys = Keys::default();
    let user = row(json!(1), "a", "p", json!(r#"{"role":"user"}"#), &mut keys);
    let parsed = user.message().unwrap();
    assert_eq!(parsed, &json!({ "role": "user" }));
    assert!(std::ptr::eq(parsed, user.message().unwrap()), "parsed once");
    // `JSON.parse(String(value))`: a number and null are JSON as they are.
    let number = row(json!(1), "a", "p", json!(5), &mut keys);
    assert_eq!(number.message().unwrap(), &json!(5));
    let null = row(json!(1), "a", "p", Value::Null, &mut keys);
    assert_eq!(null.message().unwrap(), &Value::Null);
    let bad = row(json!(1), "a", "p", json!("{bad"), &mut keys);
    assert_eq!(
        bad.message().unwrap_err(),
        "Devin's message at row 1 is no JSON"
    );
    let deep = format!("{}{}", "[".repeat(DEEPEST + 1), "]".repeat(DEEPEST + 1));
    let deep = row(json!(1), "a", "p", json!(deep), &mut keys);
    assert!(deep
        .message()
        .unwrap_err()
        .starts_with("Devin's message at row 1 is JSON this build cannot hold"));
}

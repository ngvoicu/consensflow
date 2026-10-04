use super::*;

fn bytes(bytes: &[u8]) -> Cell {
    Cell::Bytes(Arc::from(bytes))
}

#[test]
fn a_cell_is_text_as_string_writes_it() {
    assert_eq!(Cell::Null.text(), "null");
    assert_eq!(Cell::Number(1.0).text(), "1");
    assert_eq!(Cell::Number(f64::INFINITY).text(), "Infinity");
    assert_eq!(Cell::Text("x".to_owned()).text(), "x");
    assert_eq!(bytes(&[1, 2]).text(), "1,2");
    assert_eq!(bytes(&[]).text(), "");
}

#[test]
fn a_blob_is_the_same_as_itself_alone() {
    let blob = bytes(&[1, 2]);
    assert!(blob.same(&blob.clone()), "one object, held twice");
    assert!(
        !blob.same(&bytes(&[1, 2])),
        "another object of the same bytes"
    );
    assert!(Cell::Number(1.0).same(&Cell::Number(1.0)));
    assert!(Cell::Number(0.0).same(&Cell::Number(-0.0)));
    assert!(!Cell::Text("1".to_owned()).same(&Cell::Number(1.0)));
    assert!(Cell::Null.same(&Cell::Null));
    assert!(!Cell::Null.same(&Cell::Text(String::new())));
}

#[test]
fn cells_compare_as_javascript_compares_them() {
    let text = |text: &str| Cell::Text(text.to_owned());
    // Node 26's `left > right`.
    let cases = [
        (text("10"), text("9"), false),
        // By UTF-16 code units: U+FF61 after the surrogates of U+1F600.
        (text("\u{FF61}"), text("\u{1F600}"), true),
        (Cell::Number(10.0), Cell::Number(9.0), true),
        (text("10"), Cell::Number(9.0), true),
        (Cell::Null, Cell::Number(-1.0), true),
        (text("x"), Cell::Number(1.0), false),
        (Cell::Number(1.0), text("x"), false),
        (Cell::Number(1.0), Cell::Null, true),
        (text(""), Cell::Null, false),
        (text(" 2 "), Cell::Number(1.0), true),
        (text("0x10"), Cell::Number(15.0), true),
        (Cell::Number(2.5), Cell::Number(2.0), true),
        // A blob is its text: "1,2" after "1", and no number.
        (bytes(&[1, 2]), text("1"), true),
        (bytes(&[1, 2]), Cell::Number(0.0), false),
        (Cell::Number(f64::INFINITY), Cell::Number(1e308), true),
        // 0x200000000000011 is 2^57 + 32, rounded once.
        (
            text("0x200000000000011"),
            Cell::Number(144_115_188_075_855_872.0),
            true,
        ),
    ];
    for (left, right, greater) in cases {
        assert_eq!(left.greater(&right), greater, "{left:?} > {right:?}");
    }
}

#[test]
fn a_blob_is_a_key_no_other_value_is() {
    let mut keys = Keys::default();
    let blob = bytes(&[1]);
    assert_ne!(blob.key(&mut keys), blob.key(&mut keys));
    assert_eq!(Cell::Number(-0.0).key(&mut keys), Key::number(0.0));
    assert_eq!(
        Cell::Text("a".to_owned()).key(&mut keys),
        Key::Text(Arc::from("a"))
    );
    assert_eq!(Cell::Null.key(&mut keys), Key::Null);
}

#[test]
fn a_cell_is_json_as_json_stringify_writes_it() {
    let written = |cell: Cell| cell.json().to_string();
    assert_eq!(written(Cell::Number(1.0)), "1");
    assert_eq!(written(Cell::Number(-0.0)), "0");
    assert_eq!(written(Cell::Number(1.5)), "1.5");
    assert_eq!(
        written(Cell::Number(9_007_199_254_740_992.0)),
        "9007199254740992"
    );
    assert_eq!(written(Cell::Number(f64::INFINITY)), "null");
    assert_eq!(written(Cell::Number(f64::NEG_INFINITY)), "null");
    assert_eq!(written(bytes(&[1, 2])), r#"{"0":1,"1":2}"#);
    assert_eq!(written(Cell::Text("x".to_owned())), r#""x""#);
    assert_eq!(written(Cell::Null), "null");
}

#[test]
fn a_cell_is_bound_as_node_binds_the_javascript_value() {
    assert_eq!(Cell::Number(1.0).bound(), Bound::Real(1.0));
    assert_eq!(bytes(&[1, 2]).bound(), Bound::Blob(vec![1, 2]));
    assert_eq!(
        Cell::Text("x".to_owned()).bound(),
        Bound::Text("x".to_owned())
    );
    assert_eq!(Cell::Null.bound(), Bound::Null);
}

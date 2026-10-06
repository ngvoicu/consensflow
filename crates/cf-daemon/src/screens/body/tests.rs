//! A screen's body is any JSON value; what a route reads of it is what
//! JavaScript read, down to the words of the one throw.

use bytes::Bytes;
use hyper::Method;
use serde_json::json;

use super::*;
use crate::api::body::Body;

fn posted(chunks: &[&[u8]]) -> Request {
    let chunks: Vec<_> = chunks
        .iter()
        .map(|chunk| Ok(Bytes::copy_from_slice(chunk)))
        .collect();
    Request::new(
        Method::POST,
        "/api/preferences",
        None,
        Body::new(futures_util::stream::iter(chunks)),
    )
    .unwrap()
}

async fn read_text(text: &str) -> Result<Value, String> {
    read(&mut posted(&[text.as_bytes()])).await
}

#[tokio::test]
async fn no_body_is_an_empty_object() {
    assert_eq!(read(&mut posted(&[])).await, Ok(json!({})));
    assert_eq!(read_text("").await, Ok(json!({})));
}

#[tokio::test]
async fn any_json_value_is_a_body() {
    for (text, value) in [
        ("{}", json!({})),
        (
            r#"{"a":[1,{"b":null}]}"#,
            json!({ "a": [1, { "b": null }] }),
        ),
        ("[]", json!([])),
        ("[1,2]", json!([1, 2])),
        ("null", Value::Null),
        ("1", json!(1)),
        ("true", json!(true)),
        (r#""x""#, json!("x")),
        (" {} ", json!({})),
    ] {
        assert_eq!(read_text(text).await, Ok(value), "{text}");
    }
}

#[tokio::test]
async fn a_body_is_read_as_json_parse_reads_it_its_keys_in_javascript_s_order() {
    let body = read_text(r#"{"b":9007199254740993,"2":1,"a":1.5,"b":7}"#)
        .await
        .unwrap();
    assert_eq!(body.to_string(), r#"{"2":1,"b":7,"a":1.5}"#);
    let huge = read_text("[9007199254740993]").await.unwrap();
    assert_eq!(huge, json!([9_007_199_254_740_992.0]));
}

#[tokio::test]
async fn text_that_is_no_json_says_so_and_where() {
    // Node said what V8 said (`Expected property name or '}' in JSON at position 1
    // (line 1 column 2)`): the daemon says it in `serde_json`'s words.
    for text in [
        "{",
        r#"{"a""#,
        "[1,]",
        "nul",
        "'x'",
        r#"{"a":1}x"#,
        "  ",
        "\u{feff}{}",
    ] {
        let Err(words) = read_text(text).await else {
            panic!("{text:?} is JSON")
        };
        assert!(
            words.starts_with("the request body is not valid JSON: ") && words.contains(" line "),
            "{text:?}: {words}"
        );
    }
    assert_eq!(
        read_text("{").await,
        Err(
            "the request body is not valid JSON: EOF while parsing an object at line 1 column 1"
                .to_owned()
        )
    );
}

#[tokio::test]
async fn a_body_too_large_and_one_that_broke_off_are_said_in_their_own_words() {
    let big = "x".repeat(64 * 1024 + 1);
    assert_eq!(read_text(&big).await, Err("body too large".to_owned()));
    let mut broke = Request::new(
        Method::POST,
        "/api/agents",
        None,
        Body::new(futures_util::stream::iter([Err(std::io::Error::other(
            "connection reset",
        ))])),
    )
    .unwrap();
    assert_eq!(read(&mut broke).await, Err("connection reset".to_owned()));
}

#[test]
fn a_property_is_what_the_body_has_and_null_has_none_to_read() {
    let object = json!({ "id": "claude", "gone": null });
    assert_eq!(property(&object, "id"), Ok(Some(&json!("claude"))));
    assert_eq!(
        property(&object, "gone"),
        Ok(Some(&Value::Null)),
        "present, and null"
    );
    assert_eq!(property(&object, "refresh"), Ok(None));
    for other in [json!([1]), json!("id"), json!(5), json!(true)] {
        assert_eq!(property(&other, "id"), Ok(None), "{other}");
    }
    assert_eq!(
        property(&Value::Null, "id"),
        Err("Cannot read properties of null (reading 'id')".to_owned())
    );
}

#[test]
fn the_fields_of_a_roster_input_are_an_object_s_or_none_and_null_names_what_it_read_first() {
    assert_eq!(
        fields(&json!({ "name": "zed", "model": "m" }), "name"),
        Ok(json!({ "name": "zed", "model": "m" })
            .as_object()
            .cloned()
            .unwrap())
    );
    for other in [
        json!([]),
        json!(["name"]),
        json!("x"),
        json!(5),
        json!(false),
    ] {
        assert_eq!(fields(&other, "name"), Ok(Map::new()), "{other}");
    }
    assert_eq!(
        fields(&Value::Null, "workTier"),
        Err("Cannot read properties of null (reading 'workTier')".to_owned())
    );
}

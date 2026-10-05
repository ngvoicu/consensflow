//! `read_on_with`: each line read by the parser it is given. A parser that
//! builds part of a line fails where `from_slice_lossy` fails, and nowhere
//! else, so a look fails at the same record, in the same sentence: the
//! record is malformed, as Node said, or it is JSON this build cannot hold.

use std::fs;
use std::path::Path;

use cf_base::json::{from_slice_lossy_keeping, Keep};
use serde_json::json;

use super::*;

/// A parser that keeps a record's `a` and its `b` whole and drops the rest.
static KEEPS: Keep = Keep::Members(&[("a", &Keep::All), ("b", &Keep::Scalar)]);

fn keeping(line: &[u8]) -> serde_json::Result<Value> {
    from_slice_lossy_keeping(line, &KEEPS)
}

/// What a look at `file` visits, or the sentence it fails in, by `read_on`
/// and by `read_on_with` and the parser that keeps part of a line.
fn both(file: &Path) -> [Result<Vec<(Value, usize)>, String>; 2] {
    let look = |with: Option<Parse>| {
        let mut visited = Vec::new();
        let visit = &mut |record, index| {
            visited.push((record, index));
            Ok(())
        };
        let looked = match with {
            None => read_on(file, None, visit, None),
            Some(parse) => read_on_with(file, None, parse, visit, None),
        };
        looked.map(|_| visited).map_err(|stop| stop.reason())
    };
    [look(None), look(Some(keeping))]
}

fn nested(depth: usize) -> String {
    format!("{}{}", "[".repeat(depth), "]".repeat(depth))
}

#[test]
fn a_parser_that_keeps_part_of_a_line_fails_a_look_where_a_whole_one_does_in_the_same_sentence() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("t.jsonl");
    let deep = format!(r#"{{"a":1,"zz":{}}}"#, nested(200));
    let lines = [
        // Nested too deep, or a number past a double's range, in a part nobody asked for.
        (deep.as_str(), "JSON this build cannot hold at record 1"),
        (
            r#"{"a":1,"zz":[0,{"n":1e999}]}"#,
            "JSON this build cannot hold at record 1",
        ),
        (
            r#"{"a":1,"b":-1e999}"#,
            "JSON this build cannot hold at record 1",
        ),
        // Malformed, wherever, and past what would not be held either.
        (r#"{"a":1,"zz":[1,}"#, "malformed JSONL at record 1"),
        (r#"{"a":1,"zz":01}"#, "malformed JSONL at record 1"),
        (r#"{"zz":1e999,"a":}"#, "malformed JSONL at record 1"),
        ("{\"a\":\"two\nlines\"}", "malformed JSONL at record 1"),
        ("not JSON", "malformed JSONL at record 1"),
    ];
    for (line, said) in lines {
        fs::write(&file, format!("{{\"a\":0}}\n{line}\n{{\"a\":2}}\n")).unwrap();
        let [whole, kept] = both(&file);
        let sentence = whole.unwrap_err();
        assert!(sentence.starts_with(said), "{line}: {sentence}");
        assert_eq!(kept.unwrap_err(), sentence, "{line}");
    }
    // The first line that fails is the failure, whichever the second would be.
    for (first, second, said) in [
        (
            deep.as_str(),
            "not JSON",
            "JSON this build cannot hold at record 1",
        ),
        ("not JSON", deep.as_str(), "malformed JSONL at record 1"),
    ] {
        fs::write(&file, format!("{{\"a\":0}}\n{first}\n{second}\n")).unwrap();
        let [whole, kept] = both(&file);
        let sentence = whole.unwrap_err();
        assert!(sentence.starts_with(said), "{sentence}");
        assert_eq!(kept.unwrap_err(), sentence);
    }
}

#[test]
fn a_parser_that_keeps_part_of_a_line_visits_the_records_a_whole_one_does_with_the_rest_left_out() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("t.jsonl");
    let held = format!(r#"{{"a":{{"x":[1,2.50]}},"zz":{}}}"#, nested(120));
    let text = format!(
        "{}\n\n{{\"b\":[1],\"a\":9007199254740993,\"b\":\"last\"}}\n  \n{held}\n[7]\n3\nnull",
        r#"{"a":0,"c":"dropped"}"#
    );
    fs::write(&file, text).unwrap();
    let [whole, kept] = both(&file);
    let (whole, kept) = (whole.unwrap(), kept.unwrap());
    let places = |visited: &[(Value, usize)]| visited.iter().map(|(_, at)| *at).collect::<Vec<_>>();
    assert_eq!(places(&kept), places(&whole));
    assert_eq!(places(&whole), [0, 1, 2, 3, 4, 5]);
    let records: Vec<_> = kept.into_iter().map(|(record, _)| record).collect();
    assert_eq!(
        records,
        [
            json!({ "a": 0 }),
            json!({ "b": "last", "a": 9_007_199_254_740_992.0 }),
            json!({ "a": { "x": [1, 2.5] } }),
            json!([]),
            json!(3),
            Value::Null,
        ]
    );
}

#[test]
fn the_unterminated_last_line_is_read_by_the_parser_it_is_given() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("t.jsonl");
    // Whole JSON already: visited, and remembered. Not whole yet: waited for.
    // Never JSON: a failure. Too deep for a value: a failure of its own.
    for (last, visits, failure) in [
        (r#"{"a":1,"zz":[1]}"#, 2, None),
        (r#"{"a":1,"zz":[1"#, 1, None),
        (r#"{"a":1,"#, 1, None),
        (r#"{"a":1} x"#, 1, Some("malformed JSONL at record 1")),
        ("[1,}", 1, Some("malformed JSONL at record 1")),
    ] {
        fs::write(&file, format!("{{\"a\":0}}\n{last}")).unwrap();
        let [whole, kept] = both(&file);
        assert_eq!(
            whole.as_ref().map(Vec::len).map_err(String::clone),
            kept.as_ref().map(Vec::len).map_err(String::clone),
            "{last}"
        );
        match failure {
            None => assert_eq!(kept.unwrap().len(), visits, "{last}"),
            Some(said) => assert_eq!(kept.unwrap_err(), said, "{last}"),
        }
    }
    let deep = format!(r#"{{"a":1,"zz":{}}}"#, nested(200));
    fs::write(&file, format!("{{\"a\":0}}\n{deep}")).unwrap();
    let [whole, kept] = both(&file);
    let sentence = whole.unwrap_err();
    assert!(
        sentence.starts_with("JSON this build cannot hold at record 1"),
        "{sentence}"
    );
    assert_eq!(kept.unwrap_err(), sentence);
}

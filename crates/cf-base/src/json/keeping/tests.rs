use serde::de::IgnoredAny;
use serde_json::json;

use super::*;
use crate::json::{from_slice_lossy, DEEPEST};

/// A block, as a reader of a message's content asks for one.
static BLOCK: Keep = Keep::Members(&[
    ("type", &Keep::Scalar),
    ("text", &Keep::Scalar),
    ("content", &Keep::All),
]);
static MESSAGE: Keep = Keep::Members(&[("id", &Keep::Scalar), ("content", &Keep::Items(&BLOCK))]);
static RECORD: Keep = Keep::Members(&[
    ("type", &Keep::Scalar),
    ("timestamp", &Keep::All),
    ("message", &MESSAGE),
]);

/// What the keeping of `keep` is: the value `from_slice_lossy` reads, with
/// what is not kept left out.
fn kept(value: Value, keep: &Keep) -> Value {
    match (keep, value) {
        (Keep::All, value) => value,
        (_, Value::Array(items)) => match keep {
            Keep::Items(item) => Value::Array(items.into_iter().map(|v| kept(v, item)).collect()),
            _ => Value::Array(Vec::new()),
        },
        (_, Value::Object(fields)) => match keep {
            Keep::Members(members) => Value::Object(
                fields
                    .into_iter()
                    .filter_map(|(key, value)| {
                        let (_, keep) = members.iter().find(|(name, _)| *name == key)?;
                        Some((key, kept(value, keep)))
                    })
                    .collect(),
            ),
            _ => Value::Object(Map::new()),
        },
        (_, scalar) => scalar,
    }
}

fn read(text: &str, keep: &Keep) -> Value {
    from_slice_lossy_keeping(text.as_bytes(), keep).unwrap()
}

#[test]
fn a_member_asked_for_is_kept_whole_and_the_rest_is_gone() {
    let line = r#"{"type":"user","toolUseResult":{"stdout":"x","n":[1,2,{"deep":true}]},"message":{"id":"m1","usage":{"input":3},"content":[{"type":"text","text":"hi","extra":1},{"type":"thinking","thinking":"..."}]},"timestamp":{"a":[1,2]},"snapshot":[[[]]]}"#;
    assert_eq!(
        read(line, &RECORD),
        json!({
            "type": "user",
            "message": {
                "id": "m1",
                "content": [{ "type": "text", "text": "hi" }, { "type": "thinking" }]
            },
            "timestamp": { "a": [1, 2] }
        })
    );
    assert_eq!(
        read(line, &Keep::All),
        from_slice_lossy(line.as_bytes()).unwrap()
    );
}

#[test]
fn what_is_not_the_shape_asked_for_is_kept_as_a_scalar_would_be() {
    // A list or an object where a scalar is asked for is one of its own kind, empty.
    assert_eq!(read(r#"[1,{"a":2}]"#, &Keep::Scalar), json!([]));
    assert_eq!(read(r#"{"a":[1]}"#, &Keep::Scalar), json!({}));
    assert_eq!(read(r#""text""#, &Keep::Scalar), json!("text"));
    assert_eq!(read("-0.5e3", &Keep::Scalar), json!(-500.0));
    assert_eq!(read("null", &Keep::Scalar), Value::Null);
    // Members of a list, and items of an object, are none.
    assert_eq!(read(r#"[{"type":"x"}]"#, &RECORD), json!([]));
    assert_eq!(read(r#"{"a":1}"#, &Keep::Items(&Keep::Scalar)), json!({}));
    assert_eq!(read("7", &RECORD), json!(7));
    let message = r#"{"id":"m","content":"plain text"}"#;
    assert_eq!(
        read(message, &MESSAGE),
        json!({ "id": "m", "content": "plain text" })
    );
    let blocks = r#"{"content":[{"type":"a"},[1,2],"s",3,null,{"text":{"x":1}}]}"#;
    assert_eq!(
        read(blocks, &MESSAGE),
        json!({ "content": [{ "type": "a" }, [], "s", 3, null, { "text": {} }] })
    );
}

#[test]
fn a_key_said_twice_is_its_last_value_in_the_place_of_its_first() {
    let line = r#"{"timestamp":1,"type":"a","message":{"id":"x"},"timestamp":2,"message":"y","type":{"b":1}}"#;
    let expected = json!({ "timestamp": 2, "type": {}, "message": "y" });
    assert_eq!(read(line, &RECORD), expected);
    let keys: Vec<_> = read(line, &RECORD)
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    assert_eq!(keys, ["timestamp", "type", "message"]);
    // Dropped members said twice are nothing, and the one kept stays the last.
    let line = r#"{"zz":[1],"type":"a","zz":{"deep":[[]]},"type":"b"}"#;
    assert_eq!(read(line, &RECORD), json!({ "type": "b" }));
}

#[test]
fn keys_are_unescaped_before_they_are_matched_and_enumerate_as_javascript_does() {
    let line = r#"{"t\u0079pe":"user","\u0074imestamp":{"b":1,"2":2,"a":3,"1":4}}"#;
    let kept = read(line, &RECORD);
    assert_eq!(kept["type"], "user");
    let order = |value: &Value| {
        value
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect::<Vec<_>>()
    };
    assert_eq!(order(&kept["timestamp"]), ["1", "2", "b", "a"]);
}

#[test]
fn numbers_are_the_doubles_json_parse_reads() {
    // Node: JSON.stringify(JSON.parse(text)) for each.
    let line = r#"{"timestamp":[9007199254740993,-9007199254740993,1e21,1.0,5e-324,123456789012345678901234567890],"type":9007199254740993}"#;
    let kept = read(line, &RECORD);
    assert_eq!(
        crate::js::stringify(&kept),
        r#"{"timestamp":[9007199254740992,-9007199254740992,1e+21,1,5e-324,1.2345678901234568e+29],"type":9007199254740992}"#
    );
    assert!(kept["type"].is_f64(), "past 2^53 a number is a double");
}

#[test]
fn bytes_that_are_no_utf8_and_lone_surrogates_read_as_replacement_characters() {
    let kept = from_slice_lossy_keeping(
        b"{\"timestamp\":\"a\xff \\ud83d \\ude00 \\ud83d\\ude00\",\"type\":\"\xc3\x28\"}",
        &RECORD,
    )
    .unwrap();
    assert_eq!(kept["timestamp"], "a\u{FFFD} \u{FFFD} \u{FFFD} \u{1F600}");
    assert_eq!(kept["type"], "\u{FFFD}(");
}

#[test]
fn what_is_dropped_is_read_all_the_same_and_fails_where_a_value_would() {
    let nested = |depth: usize| format!("{}{}", "[".repeat(depth), "]".repeat(depth));
    // A line may nest DEEPEST levels, its object the first, in a member nobody asked for.
    let held = format!(r#"{{"dropped":{}}}"#, nested(DEEPEST - 1));
    assert_eq!(read(&held, &RECORD), json!({}));
    assert!(from_slice_lossy(held.as_bytes()).is_ok());
    for depth in [DEEPEST, DEEPEST + 1, 500, 100_000] {
        let line = format!(r#"{{"dropped":{}}}"#, nested(depth));
        assert!(from_slice_lossy(line.as_bytes()).is_err(), "{depth}");
        assert!(
            from_slice_lossy_keeping(line.as_bytes(), &RECORD).is_err(),
            "{depth}"
        );
        // Dropped as `IgnoredAny` drops it, it would be read.
        assert!(serde_json::from_str::<IgnoredAny>(&line).is_ok(), "{depth}");
    }
    // A number past a double's range, anywhere.
    for line in [
        r#"{"dropped":1e400}"#,
        r#"{"dropped":[0,{"x":-1e999}]}"#,
        r#"{"type":1e309}"#,
        r#"{"timestamp":[1e999]}"#,
        r#"{"dropped":{"a":123e400}}"#,
    ] {
        assert!(from_slice_lossy(line.as_bytes()).is_err(), "{line}");
        assert!(
            from_slice_lossy_keeping(line.as_bytes(), &RECORD).is_err(),
            "{line}"
        );
        assert!(serde_json::from_str::<IgnoredAny>(line).is_ok(), "{line}");
    }
    // A number at the edge of the range, and below it, is read.
    for line in [
        r#"{"dropped":1.7976931348623157e308}"#,
        r#"{"dropped":-1.7976931348623158e308}"#,
        r#"{"dropped":1e-400}"#,
        r#"{"dropped":0e999999999999}"#,
    ] {
        assert!(from_slice_lossy(line.as_bytes()).is_ok(), "{line}");
        assert!(
            from_slice_lossy_keeping(line.as_bytes(), &RECORD).is_ok(),
            "{line}"
        );
    }
}

#[test]
fn text_that_is_no_json_is_none_wherever_it_is_wrong() {
    for line in [
        "",
        " ",
        "{",
        r#"{"type":"#,
        r#"{"dropped":[1,}"#,
        r#"{"dropped":01}"#,
        r#"{"dropped":"\x"}"#,
        "{\"dropped\":\"a\nb\"}",
        r#"{"dropped":nul}"#,
        r#"{"type":"a"} x"#,
        r#"{"type":"a"}{"type":"b"}"#,
        r#"{"dropped":[1,2]]}"#,
        r#"{'a':1}"#,
        "\u{feff}{}",
        r#"{"a" 1}"#,
        r#"{1:2}"#,
        r#"{"dropped":.5}"#,
        r#"{"dropped":+1}"#,
        r#"{"dropped":1.}"#,
        r#"{"dropped":NaN}"#,
        r#"{"a":1,}"#,
    ] {
        assert!(from_slice_lossy(line.as_bytes()).is_err(), "{line:?}");
        assert!(
            from_slice_lossy_keeping(line.as_bytes(), &RECORD).is_err(),
            "{line:?}"
        );
    }
}

/// xorshift64*, for lines of a thousand shapes from a seed.
struct Random(u64);

impl Random {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, bound: usize) -> usize {
        (self.next() % bound as u64) as usize
    }

    fn pick<'a>(&mut self, from: &[&'a str]) -> &'a str {
        from[self.below(from.len())]
    }
}

const KEYS: [&str; 12] = [
    "type",
    "message",
    "content",
    "timestamp",
    "text",
    "id",
    "a",
    "b",
    "1",
    "2",
    "10",
    r#"t\u0079pe"#,
];
const SCALARS: [&str; 26] = [
    "null",
    "true",
    "false",
    "0",
    "-0",
    "1",
    "-1.5",
    "1e21",
    "1E+2",
    "5e-324",
    "9007199254740992",
    "9007199254740993",
    "-9007199254740993",
    "123456789012345678901234567890",
    "1e400",
    "-1e400",
    "0.1",
    r#""text""#,
    r#""""#,
    r#""line\nbreak \"quoted\" \\ \u00e9 é 日本 😀""#,
    r#""\ud83d""#,
    r#""\ude00 tail""#,
    r#""\ud83d\ude00""#,
    r#""\ud83d\u0041""#,
    r#""\\ud83d""#,
    r#""a\u0000b""#,
];

/// A value of JSON text, nested up to `depth` more levels.
fn value(random: &mut Random, depth: usize, out: &mut String) {
    let space = |random: &mut Random, out: &mut String| {
        if random.below(8) == 0 {
            out.push_str(random.pick(&[" ", "\t", "\n", "\r\n", "  "]));
        }
    };
    space(random, out);
    match random.below(if depth == 0 { 3 } else { 8 }) {
        0..=2 => out.push_str(random.pick(&SCALARS)),
        3..=5 => {
            out.push('{');
            for member in 0..random.below(7) {
                if member > 0 {
                    out.push(',');
                }
                space(random, out);
                out.push('"');
                out.push_str(random.pick(&KEYS));
                out.push_str("\":");
                value(random, depth - 1, out);
            }
            out.push('}');
        }
        _ => {
            out.push('[');
            for item in 0..random.below(5) {
                if item > 0 {
                    out.push(',');
                }
                value(random, depth - 1, out);
            }
            out.push(']');
        }
    }
    space(random, out);
}

/// Bytes broken in one place, as a writer cut short or a disk gone wrong breaks them.
fn broken(random: &mut Random, mut bytes: Vec<u8>) -> Vec<u8> {
    if bytes.is_empty() {
        return bytes;
    }
    let at = random.below(bytes.len());
    match random.below(6) {
        0 => bytes.truncate(at),
        1 => {
            bytes.remove(at);
        }
        2 => bytes[at] = b"{}[],:\"\\0e-.\xff"[random.below(13)],
        3 => bytes.insert(at, b"{}[],:\"\\\xff\xc3 "[random.below(11)]),
        4 => bytes.extend_from_slice(random.pick(&["x", " ", "}", ",1", "\n{}"]).as_bytes()),
        _ => bytes[at] = b'\\',
    }
    bytes
}

#[test]
fn a_line_is_read_as_the_value_it_is_read_as_with_the_rest_left_out_whatever_the_line() {
    let schemas: [&Keep; 6] = [
        &Keep::All,
        &Keep::Scalar,
        &Keep::Items(&Keep::Scalar),
        &RECORD,
        &MESSAGE,
        &Keep::Items(&BLOCK),
    ];
    let mut random = Random(0x5eed);
    let (mut agreed, mut failed) = (0, 0);
    for case in 0..6000 {
        let mut text = String::new();
        // Mostly shallow, some past the depth a value is read to.
        let depth = if case % 40 == 0 {
            DEEPEST + 3
        } else {
            1 + random.below(6)
        };
        value(&mut random, depth.min(5), &mut text);
        if case % 40 == 0 {
            text = format!("{}{text}{}", "[".repeat(depth), "]".repeat(depth));
        }
        let mut bytes = text.into_bytes();
        if case % 3 == 0 {
            bytes = broken(&mut random, bytes);
        }
        let valid = from_slice_lossy(&bytes);
        for keep in schemas {
            let reading = from_slice_lossy_keeping(&bytes, keep);
            match (&valid, reading) {
                (Ok(whole), Ok(reading)) => {
                    assert_eq!(
                        reading,
                        kept(whole.clone(), keep),
                        "{keep:?}: {:?}",
                        String::from_utf8_lossy(&bytes)
                    );
                    agreed += 1;
                }
                (Err(_), Err(_)) => failed += 1,
                (valid, reading) => panic!(
                    "{keep:?}: {:?}: whole {valid:?}, kept {reading:?}",
                    String::from_utf8_lossy(&bytes)
                ),
            }
        }
    }
    // The generator made lines of both kinds, plenty of each.
    assert!(
        agreed > 5000 && failed > 5000,
        "{agreed} agreed, {failed} failed"
    );
}

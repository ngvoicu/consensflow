//! JSON as Node writes it. `JSON.stringify` writes a string holding half of
//! a surrogate pair (a preview `.slice` cut through an emoji) as a lone
//! surrogate's escape, which serde_json refuses; and Node reads invalid UTF-8
//! as U+FFFD where serde_json refuses that too. Read here, both become U+FFFD,
//! as Node prints them in text. Written back out as JSON (cf's `--json`), the
//! half emoji is U+FFFD too, where Node wrote the lone escape again: valid
//! JSON for invalid, on purpose.
//!
//! A line of a big file can be read for part of what it holds
//! ([`from_slice_lossy_keeping`]): the rest is read to its end, and never built.

mod keeping;

use std::borrow::Cow;

use serde_json::{Map, Number, Value};

pub use keeping::{from_slice_lossy_keeping, Keep};

/// The most levels of arrays and objects serde_json reads, the root's own
/// included: JSON nested deeper is no value here.
pub const DEEPEST: usize = 127;

/// How many levels of arrays and objects `value` nests, its own included:
/// none for a number, a text, a flag or null.
pub fn nesting(value: &Value) -> usize {
    match value {
        Value::Array(items) => 1 + items.iter().map(nesting).max().unwrap_or(0),
        Value::Object(fields) => 1 + fields.values().map(nesting).max().unwrap_or(0),
        _ => 0,
    }
}

/// The JSON in `bytes` as `JSON.parse` holds it: every number the double it
/// reads as, keys in the order JavaScript enumerates them; invalid UTF-8 and
/// lone surrogate escapes read as U+FFFD. serde_json reads no further than
/// [`DEEPEST`] levels of nesting, where JavaScript's `JSON.parse` has no such
/// limit: [`is_json_lossy`] tells what is too deep to read from what is no
/// JSON.
pub fn from_slice_lossy(bytes: &[u8]) -> serde_json::Result<Value> {
    let text = String::from_utf8_lossy(bytes);
    let mut value = serde_json::from_str(&without_lone_surrogates(&text))?;
    as_parsed(&mut value);
    Ok(value)
}

/// What [`from_slice_exact`] will not read: JSON a value here cannot hold as
/// `JSON.parse` would, without losing some of it.
#[derive(Debug)]
pub enum Inexact {
    /// No JSON, or JSON serde_json does not read: nested past [`DEEPEST`]
    /// levels, or a number past a double's range, where JavaScript reads
    /// `Infinity`.
    Json(serde_json::Error),
    /// A lone surrogate's escape, which a JavaScript string holds and a Rust
    /// one cannot: read as U+FFFD, two keys could become one.
    LoneSurrogate,
}

/// The JSON in `bytes` as `JSON.parse` reads it from a file Node decoded as
/// UTF-8: invalid UTF-8 as U+FFFD, keys in the order JavaScript enumerates
/// them, and every number the double it reads as (an integer past 2^53
/// rounds). What a value here cannot hold without losing some of it is no
/// reading at all ([`Inexact`]), so a file read to be written back is never
/// written back changed.
pub fn from_slice_exact(bytes: &[u8]) -> Result<Value, Inexact> {
    let text = String::from_utf8_lossy(bytes);
    if matches!(without_lone_surrogates(&text), Cow::Owned(_)) {
        return Err(Inexact::LoneSurrogate);
    }
    let mut value = serde_json::from_str(&text).map_err(Inexact::Json)?;
    as_parsed(&mut value);
    Ok(value)
}

/// `value` as `JSON.parse` holds the same JSON, in place: every integer
/// past 2^53 the double it reads as, and each object's keys in the order
/// JavaScript enumerates them.
fn as_parsed(value: &mut Value) {
    match value {
        Value::Number(number) => {
            if let Some(double) = past_safe(number) {
                *number = double;
            }
        }
        Value::Array(items) => items.iter_mut().for_each(as_parsed),
        Value::Object(fields) => {
            in_enumeration_order(fields);
            fields.values_mut().for_each(as_parsed);
        }
        _ => {}
    }
}

/// The double JavaScript reads an integer past 2^53 as; none for any other
/// number, which it reads as it is.
fn past_safe(number: &Number) -> Option<Number> {
    const SAFE: u64 = 1 << 53;
    let beyond = number.as_u64().is_some_and(|whole| whole > SAFE)
        || number
            .as_i64()
            .is_some_and(|whole| whole.unsigned_abs() > SAFE);
    number
        .as_f64()
        .filter(|_| beyond)
        .and_then(Number::from_f64)
}

/// Whether `bytes` are JSON nested to any depth, which [`from_slice_lossy`]
/// reads to [`DEEPEST`] levels only: JSON that is skipped, never built into a
/// value, so it cannot overflow a stack.
pub fn is_json_lossy(bytes: &[u8]) -> bool {
    let text = String::from_utf8_lossy(bytes);
    serde_json::from_str::<serde::de::IgnoredAny>(&text).is_ok()
}

/// `value` with each object's keys in the order JavaScript enumerates them:
/// the keys that are array indices first, ascending, then the others as
/// written. `JSON.stringify` writes them so, and what Node printed is printed
/// so again.
pub fn js_order(mut value: Value) -> Value {
    order_keys(&mut value);
    value
}

/// A request's body, read by serde_json as it was written, made what
/// `JSON.parse` would have handed Node, in place: every integer past 2^53
/// the double it reads as, and every object's keys in JavaScript's order.
pub fn as_parsed_fields(fields: &mut Map<String, Value>) {
    in_enumeration_order(fields);
    fields.values_mut().for_each(as_parsed);
}

fn order_keys(value: &mut Value) {
    match value {
        Value::Array(items) => items.iter_mut().for_each(order_keys),
        Value::Object(fields) => {
            in_enumeration_order(fields);
            fields.values_mut().for_each(order_keys);
        }
        _ => {}
    }
}

/// `fields` in the order JavaScript enumerates them, their values as they
/// are: the keys that are array indices first, ascending, then the others
/// as written. Most objects hold no such key, and are left as they are.
fn in_enumeration_order(fields: &mut Map<String, Value>) {
    if !fields.keys().any(|key| array_index(key).is_some()) {
        return;
    }
    let (mut indices, named): (Vec<_>, Vec<_>) = std::mem::take(fields)
        .into_iter()
        .partition(|(key, _)| array_index(key).is_some());
    indices.sort_by_key(|(key, _)| array_index(key));
    *fields = indices.into_iter().chain(named).collect();
}

/// The array index a key is: digits with no leading zero, below 2^32 - 1.
pub(crate) fn array_index(key: &str) -> Option<u32> {
    let canonical = !key.is_empty()
        && key.bytes().all(|byte| byte.is_ascii_digit())
        && (key == "0" || !key.starts_with('0'));
    canonical
        .then(|| key.parse::<u32>().ok())
        .flatten()
        .filter(|index| *index != u32::MAX)
}

/// `json` with the escape of every lone surrogate replaced by the escape of U+FFFD.
fn without_lone_surrogates(json: &str) -> Cow<'_, str> {
    let bytes = json.as_bytes();
    let mut fixed: Option<String> = None;
    let mut copied = 0;
    let mut at = 0;
    // Most of a line is no escape: `memchr` goes from one backslash to the next.
    while let Some(found) = bytes.get(at..).and_then(|rest| memchr::memchr(b'\\', rest)) {
        at += found;
        let Some(unit) = escaped_unit(bytes, at) else {
            // `\"`, `\\`, `\n` and the like: what they escape starts no escape.
            at += 2;
            continue;
        };
        let leading = (0xD800..=0xDBFF).contains(&unit);
        let paired = leading
            && escaped_unit(bytes, at + 6).is_some_and(|next| (0xDC00..=0xDFFF).contains(&next));
        if paired {
            at += 12;
        } else if leading || (0xDC00..=0xDFFF).contains(&unit) {
            let out = fixed.get_or_insert_with(|| String::with_capacity(json.len()));
            out.push_str(&json[copied..at]);
            out.push_str("\\ufffd");
            at += 6;
            copied = at;
        } else {
            at += 6;
        }
    }
    match fixed {
        None => Cow::Borrowed(json),
        Some(mut out) => {
            out.push_str(&json[copied..]);
            Cow::Owned(out)
        }
    }
}

/// The UTF-16 code unit a `\uXXXX` escape starting at `at` stands for.
fn escaped_unit(bytes: &[u8], at: usize) -> Option<u16> {
    match bytes.get(at..at + 6)? {
        [b'\\', b'u', hex @ ..] if hex.iter().all(u8::is_ascii_hexdigit) => {
            u16::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok()
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn json_read_exactly_is_what_json_parse_reads_or_no_reading() {
        let read = |text: &str| from_slice_exact(text.as_bytes());
        // Node: JSON.parse('{"b":9007199254740993,"2":1,"a":1.5}')
        assert_eq!(
            crate::js::stringify(&read(r#"{"b":9007199254740993,"2":1,"a":1.5}"#).unwrap()),
            r#"{"2":1,"b":9007199254740992,"a":1.5}"#
        );
        assert_eq!(
            crate::js::stringify(&read("[-9007199254740993, 9007199254740992, 5]").unwrap()),
            "[-9007199254740992,9007199254740992,5]"
        );
        assert!(matches!(
            read(r#"{"\ud800":1,"\ud801":2}"#),
            Err(Inexact::LoneSurrogate)
        ));
        assert!(matches!(read(r#"["\udc00"]"#), Err(Inexact::LoneSurrogate)));
        assert_eq!(read(r#"["\ud83d\ude00"]"#).unwrap(), json!(["\u{1F600}"]));
        assert!(matches!(read(r#"{"x":1e400}"#), Err(Inexact::Json(_))));
        let deep = format!("{}{}", "[".repeat(200), "]".repeat(200));
        assert!(matches!(read(&deep), Err(Inexact::Json(_))));
        assert!(matches!(read("{,}"), Err(Inexact::Json(_))));
        // Bytes that are no UTF-8 are U+FFFD, as Node decoded the file.
        assert_eq!(
            from_slice_exact(b"[\"\xff\"]").unwrap(),
            json!(["\u{FFFD}"])
        );
    }

    #[test]
    fn every_number_reads_as_the_double_json_parse_reads() {
        // Node: JSON.stringify(JSON.parse(text)) for each. serde_json reads
        // the first two wrong without `float_roundtrip`.
        for (text, node) in [
            ("1000000000000000.1", "1000000000000000.1"),
            ("1.7976931348623157000e308", "1.7976931348623157e+308"),
            ("5e-324", "5e-324"),
            ("4.9406564584124654e-324", "5e-324"),
            ("2.2250738585072011e-308", "2.225073858507201e-308"),
            ("123456789012345678901234567890", "1.2345678901234568e+29"),
            ("0.30000000000000004", "0.30000000000000004"),
            ("9007199254740993.0", "9007199254740992"),
            ("1e-400", "0"),
        ] {
            let read = from_slice_exact(text.as_bytes()).unwrap();
            assert_eq!(crate::js::stringify(&read), node, "{text}");
        }
    }

    #[test]
    fn json_read_lossily_holds_numbers_and_keys_as_json_parse_does() {
        let read = from_slice_lossy(
            br#"{"b":9007199254740993,"2":[-9007199254740993,7],"a":{"z":0,"1":1}}"#,
        )
        .unwrap();
        // Node: JSON.parse(text), whose keys enumerate as Object.keys lists them.
        assert!(read["b"].is_f64() && read["b"] == json!(9_007_199_254_740_992.0));
        assert_eq!(read["2"], json!([-9_007_199_254_740_992.0, 7]));
        let keys = |value: &Value| {
            value
                .as_object()
                .unwrap()
                .keys()
                .cloned()
                .collect::<Vec<_>>()
        };
        assert_eq!(keys(&read), ["2", "b", "a"]);
        assert_eq!(keys(&read["a"]), ["1", "z"]);
    }

    #[test]
    fn nesting_counts_the_levels_of_arrays_and_objects() {
        assert_eq!(nesting(&json!(1)), 0);
        assert_eq!(nesting(&json!([])), 1);
        assert_eq!(nesting(&json!({ "a": [1, { "b": {} }], "c": "x" })), 4);
    }

    #[test]
    fn a_preview_cut_through_an_emoji_reads_with_a_replacement_character() {
        // What Node writes for `{ preview: 'cut 😀'.slice(0, 5) }`.
        let read = from_slice_lossy(br#"{"preview":"cut \ud83d"}"#).unwrap();
        assert_eq!(read, json!({ "preview": "cut \u{FFFD}" }));
        let trailing = from_slice_lossy(br#"["\ude00 tail"]"#).unwrap();
        assert_eq!(trailing, json!(["\u{FFFD} tail"]));
    }

    #[test]
    fn a_whole_pair_and_an_escaped_backslash_stay_as_written() {
        let read = from_slice_lossy(
            br#"["\ud83d\ude00", "\\ud83d", "\ud83d\u0041", "\ud83d\ud83d\ude00"]"#,
        )
        .unwrap();
        assert_eq!(read, json!(["😀", "\\ud83d", "\u{FFFD}A", "\u{FFFD}😀"]));
    }

    #[test]
    fn invalid_utf8_reads_as_a_replacement_character() {
        let read = from_slice_lossy(b"{\"text\":\"a\xff b\"}").unwrap();
        assert_eq!(read, json!({ "text": "a\u{FFFD} b" }));
    }

    #[test]
    fn json_with_no_lone_surrogate_is_read_without_a_copy() {
        assert!(matches!(
            without_lone_surrogates(r#"{"a":"\n\u00e9"}"#),
            Cow::Borrowed(_)
        ));
    }

    #[test]
    fn orders_keys_as_javascript_enumerates_them_indices_first() {
        let written = json!({ "b": 1, "2": 2, "a": 3, "1": 4, "01": 5, "4294967294": 6,
            "4294967295": 7, "-1": 8, "nested": { "z": 0, "10": 1, "9": 2 } });
        assert_eq!(
            js_order(written).to_string(),
            r#"{"1":4,"2":2,"4294967294":6,"b":1,"a":3,"01":5,"4294967295":7,"-1":8,"nested":{"9":2,"10":1,"z":0}}"#
        );
    }

    #[test]
    fn what_is_not_json_is_still_refused() {
        assert!(from_slice_lossy(b"{\"a\":").is_err());
    }

    #[test]
    fn json_nested_deeper_than_serde_json_reads_is_json_all_the_same() {
        let nested = |depth: usize| format!("{}{}", "[".repeat(depth), "]".repeat(depth));
        // What `JSON.parse` reads at any depth is read here to 127 levels (the root is the first).
        let deepest = from_slice_lossy(nested(DEEPEST).as_bytes()).unwrap();
        assert_eq!(nesting(&deepest), DEEPEST);
        for depth in [DEEPEST + 1, 200, 100_000] {
            let deep = nested(depth);
            assert!(from_slice_lossy(deep.as_bytes()).is_err(), "{depth}");
            assert!(is_json_lossy(deep.as_bytes()), "{depth}");
        }
        assert!(is_json_lossy(b"{\"a\":[1,{\"b\":null}]}"));
        assert!(
            is_json_lossy(br#"["\ud83d"]"#),
            "a lone surrogate escape is read"
        );
        assert!(
            is_json_lossy(b"[\"a\xff\"]"),
            "so are bytes that are no UTF-8"
        );
    }

    #[test]
    fn what_is_no_json_is_none_however_deep() {
        for text in [
            "", "not json", "[1,", "{\"a\":}", "[[[[1]]]", "[] []", "{'a':1}",
        ] {
            assert!(!is_json_lossy(text.as_bytes()), "{text:?}");
        }
        let broken = format!("{}1{}", "[".repeat(300), "]".repeat(299));
        assert!(!is_json_lossy(broken.as_bytes()));
    }

    /// `without_lone_surrogates` as it was first written, a byte at a time.
    fn by_the_byte(json: &str) -> Cow<'_, str> {
        let bytes = json.as_bytes();
        let mut fixed: Option<String> = None;
        let (mut copied, mut at) = (0, 0);
        while at < bytes.len() {
            if bytes[at] != b'\\' {
                at += 1;
                continue;
            }
            let Some(unit) = escaped_unit(bytes, at) else {
                at += 2;
                continue;
            };
            let leading = (0xD800..=0xDBFF).contains(&unit);
            let paired = leading
                && escaped_unit(bytes, at + 6)
                    .is_some_and(|next| (0xDC00..=0xDFFF).contains(&next));
            if paired {
                at += 12;
            } else if leading || (0xDC00..=0xDFFF).contains(&unit) {
                let out = fixed.get_or_insert_with(|| String::with_capacity(json.len()));
                out.push_str(&json[copied..at]);
                out.push_str("\\ufffd");
                at += 6;
                copied = at;
            } else {
                at += 6;
            }
        }
        match fixed {
            None => Cow::Borrowed(json),
            Some(mut out) => {
                out.push_str(&json[copied..]);
                Cow::Owned(out)
            }
        }
    }

    #[test]
    fn the_escapes_are_found_as_a_scan_of_every_byte_finds_them() {
        // Texts made of what escapes are made of, and of what they can be mistaken for.
        const PIECES: [&str; 20] = [
            "\\", "\\\\", "\\u", "\\ud83d", "\\ude00", "\\uD800", "\\udFFF", "\\u0041", "\\u00e",
            "\\n", "\\\"", "u", "d8", "00", "\"", "é", "日", "😀", "x", " ",
        ];
        let mut state = 0x9E37_79B9_7F4A_7C15_u64;
        let mut next = move || {
            state ^= state >> 12;
            state ^= state << 25;
            state ^= state >> 27;
            usize::try_from(state.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 33).unwrap_or(0)
        };
        let mut changed = 0;
        for _ in 0..20_000 {
            let text: String = (0..next() % 14)
                .map(|_| PIECES[next() % PIECES.len()])
                .collect();
            let (fast, slow) = (without_lone_surrogates(&text), by_the_byte(&text));
            assert_eq!(fast, slow, "{text:?}");
            assert_eq!(matches!(fast, Cow::Owned(_)), matches!(slow, Cow::Owned(_)));
            changed += usize::from(matches!(fast, Cow::Owned(_)));
        }
        // Plenty had a lone surrogate to fix, and plenty had none.
        assert!((2000..18_000).contains(&changed), "{changed}");
        // A backslash is the last byte, or the one before it.
        for text in ["\\", "a\\", "\\u", "\\ud800", "\\ud800\\", "é\\"] {
            assert_eq!(without_lone_surrogates(text), by_the_byte(text), "{text:?}");
        }
    }
}

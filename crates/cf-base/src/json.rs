//! JSON as Node writes it. `JSON.stringify` writes a string holding half of
//! a surrogate pair (a preview `.slice` cut through an emoji) as a lone
//! surrogate's escape, which serde_json refuses; and Node reads invalid UTF-8
//! as U+FFFD where serde_json refuses that too. Read here, both become U+FFFD,
//! as Node prints them in text. Written back out as JSON (cf's `--json`), the
//! half emoji is U+FFFD too, where Node wrote the lone escape again: valid
//! JSON for invalid, on purpose.

use std::borrow::Cow;

use serde_json::Value;

/// The JSON in `bytes`, invalid UTF-8 and lone surrogate escapes read as U+FFFD.
/// serde_json reads no further than 128 levels of nesting, where JavaScript's
/// `JSON.parse` has no such limit: [`is_json_lossy`] tells what is too deep to
/// read from what is no JSON.
pub fn from_slice_lossy(bytes: &[u8]) -> serde_json::Result<Value> {
    let text = String::from_utf8_lossy(bytes);
    serde_json::from_str(&without_lone_surrogates(&text))
}

/// What [`from_slice_exact`] will not read: JSON a value here cannot hold as
/// `JSON.parse` would, without losing some of it.
#[derive(Debug)]
pub enum Inexact {
    /// No JSON, or JSON serde_json does not read: nested past 128 levels, or a
    /// number past a double's range, where JavaScript reads `Infinity`.
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
    let value = serde_json::from_str(&text).map_err(Inexact::Json)?;
    Ok(js_order(as_doubles(value)))
}

/// `value` with every integer past 2^53 the double JavaScript reads it as.
fn as_doubles(value: Value) -> Value {
    const SAFE: u64 = 1 << 53;
    match value {
        Value::Number(number) => {
            let beyond = number.as_u64().is_some_and(|whole| whole > SAFE)
                || number
                    .as_i64()
                    .is_some_and(|whole| whole.unsigned_abs() > SAFE);
            match number
                .as_f64()
                .filter(|_| beyond)
                .and_then(serde_json::Number::from_f64)
            {
                Some(double) => Value::Number(double),
                None => Value::Number(number),
            }
        }
        Value::Array(items) => Value::Array(items.into_iter().map(as_doubles).collect()),
        Value::Object(fields) => Value::Object(
            fields
                .into_iter()
                .map(|(key, item)| (key, as_doubles(item)))
                .collect(),
        ),
        other => other,
    }
}

/// Whether `bytes` are JSON nested to any depth, which [`from_slice_lossy`]
/// reads to 128 levels only: JSON that is skipped, never built into a value, so
/// it cannot overflow a stack.
pub fn is_json_lossy(bytes: &[u8]) -> bool {
    let text = String::from_utf8_lossy(bytes);
    serde_json::from_str::<serde::de::IgnoredAny>(&text).is_ok()
}

/// `value` with each object's keys in the order JavaScript enumerates them:
/// the keys that are array indices first, ascending, then the others as
/// written. `JSON.stringify` writes them so, and what Node printed is printed
/// so again.
pub fn js_order(value: Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.into_iter().map(js_order).collect()),
        Value::Object(map) => {
            let (mut indices, named): (Vec<_>, Vec<_>) = map
                .into_iter()
                .map(|(key, value)| (key, js_order(value)))
                .partition(|(key, _)| array_index(key).is_some());
            indices.sort_by_key(|(key, _)| array_index(key));
            Value::Object(indices.into_iter().chain(named).collect())
        }
        other => other,
    }
}

/// The array index a key is: digits with no leading zero, below 2^32 - 1.
fn array_index(key: &str) -> Option<u32> {
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
    while at < bytes.len() {
        if bytes[at] != b'\\' {
            at += 1;
            continue;
        }
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
        assert!(from_slice_lossy(nested(127).as_bytes()).is_ok());
        for depth in [128, 200, 100_000] {
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
}

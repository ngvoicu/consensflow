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
pub fn from_slice_lossy(bytes: &[u8]) -> serde_json::Result<Value> {
    let text = String::from_utf8_lossy(bytes);
    serde_json::from_str(&without_lone_surrogates(&text))
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
}

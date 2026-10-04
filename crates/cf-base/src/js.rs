//! JavaScript's readings of values and text, for ports of code that relied
//! on them: a JSON value written into a template literal, tested for truth,
//! joined, written by `JSON.stringify`; a number written as `String` writes
//! it; text trimmed, read as a number, or ordered as `localeCompare` orders
//! it; a JSON value read as `Number(value)` reads it. A port that reads a
//! value the way the Node code did says what Node said.

use std::borrow::Cow;
use std::cmp::Ordering;

use serde_json::{Map, Value};

use crate::json::array_index;

mod to_number;

pub use to_number::to_number;

/// `value` as `${value}` wrote it; `None` is a field that was not there. For
/// a value that may hold an object with a `toString` of its own, where
/// JavaScript threw, see [`string`].
pub fn text(value: Option<&Value>) -> Cow<'_, str> {
    match value {
        None => Cow::Borrowed("undefined"),
        Some(Value::Null) => Cow::Borrowed("null"),
        Some(Value::String(text)) => Cow::Borrowed(text),
        Some(Value::Array(items)) => Cow::Owned(join(items, ",")),
        Some(Value::Object(_)) => Cow::Borrowed("[object Object]"),
        Some(Value::Number(number)) => Cow::Owned(number_text(number.as_f64().unwrap_or(f64::NAN))),
        Some(Value::Bool(flag)) => Cow::Borrowed(if *flag { "true" } else { "false" }),
    }
}

/// `String(value)`, and `${value}`, or why JavaScript threw instead. An
/// object with a `toString` of its own, which JSON can make only something
/// that is no function, has no way to be text: V8 throws `Cannot convert
/// object to primitive value`. So does a list that holds one at any depth,
/// whose `join` makes each item text.
pub fn string(value: Option<&Value>) -> Result<Cow<'_, str>, String> {
    if value.is_some_and(holds_its_own_to_string) {
        return Err("an object with a toString of its own cannot be made text".to_owned());
    }
    Ok(text(value))
}

fn holds_its_own_to_string(value: &Value) -> bool {
    match value {
        Value::Object(fields) => fields.contains_key("toString"),
        Value::Array(items) => items.iter().any(holds_its_own_to_string),
        _ => false,
    }
}

/// `number` as `String(number)` writes it: the shortest digits that read
/// back as it, in a plain or an exponent form by its size (`2`, `0.000001`,
/// `1e-7`, `1e+21`), where Rust writes `2.0` and `1e21`.
pub fn number_text(number: f64) -> String {
    if number.is_nan() {
        return "NaN".to_owned();
    }
    if number == 0.0 {
        return "0".to_owned();
    }
    if number.is_infinite() {
        return if number > 0.0 {
            "Infinity"
        } else {
            "-Infinity"
        }
        .to_owned();
    }
    let sign = if number < 0.0 { "-" } else { "" };
    // Rust's exponent form is the shortest digits that read back: `d.ddde±x`.
    let scientific = format!("{:e}", number.abs());
    let (mantissa, exponent) = scientific.split_once('e').unwrap_or((&scientific, "0"));
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let point = exponent.parse::<i32>().unwrap_or(0) + 1;
    let count = i32::try_from(digits.len()).unwrap_or(i32::MAX);
    let zeros = |count: i32| "0".repeat(usize::try_from(count).unwrap_or(0));
    let written = if count <= point && point <= 21 {
        format!("{digits}{}", zeros(point - count))
    } else if 0 < point && point <= 21 {
        let (whole, fraction) = digits.split_at(usize::try_from(point).unwrap_or(0));
        format!("{whole}.{fraction}")
    } else if -6 < point && point <= 0 {
        format!("0.{}{digits}", zeros(-point))
    } else {
        let power = point - 1;
        let power_sign = if power < 0 { '-' } else { '+' };
        let (first, rest) = digits.split_at(1);
        let rest = if rest.is_empty() {
            String::new()
        } else {
            format!(".{rest}")
        };
        format!("{first}{rest}e{power_sign}{}", power.abs())
    };
    format!("{sign}{written}")
}

/// `value` as `JSON.stringify(value)` writes it: no white space, each
/// object's keys in the order JavaScript enumerates them (those that are
/// array indices first, ascending, then the others as they are held), every
/// number as JavaScript writes it (`2` for `2.0`, and an integer past 2^53
/// as the double it reads as).
pub fn stringify(value: &Value) -> String {
    let mut written = String::new();
    write_json(value, "", 0, &mut written);
    written
}

/// `value` as `JSON.stringify(value, null, spaces)` writes it: each member
/// and item on a line of its own, indented `spaces` more than its parent;
/// an empty object or list as `{}` or `[]`.
pub fn stringify_indented(value: &Value, spaces: usize) -> String {
    let mut written = String::new();
    write_json(value, &" ".repeat(spaces), 0, &mut written);
    written
}

fn write_json(value: &Value, indent: &str, depth: usize, written: &mut String) {
    // Before a member or an item: nothing compact, else a line at its depth.
    let open_line = |written: &mut String, depth: usize| {
        if !indent.is_empty() {
            written.push('\n');
            written.push_str(&indent.repeat(depth));
        }
    };
    match value {
        Value::Null => written.push_str("null"),
        Value::Bool(flag) => written.push_str(if *flag { "true" } else { "false" }),
        Value::Number(number) => {
            let number = number.as_f64().unwrap_or(f64::NAN);
            // JSON writes a number that is no finite one as null.
            if number.is_finite() {
                written.push_str(&number_text(number));
            } else {
                written.push_str("null");
            }
        }
        Value::String(text) => written.push_str(&Value::String(text.clone()).to_string()),
        Value::Array(items) => {
            written.push('[');
            for (at, item) in items.iter().enumerate() {
                if at > 0 {
                    written.push(',');
                }
                open_line(written, depth + 1);
                write_json(item, indent, depth + 1, written);
            }
            if !items.is_empty() {
                open_line(written, depth);
            }
            written.push(']');
        }
        Value::Object(fields) => {
            written.push('{');
            for (at, (key, item)) in enumerated(fields).into_iter().enumerate() {
                if at > 0 {
                    written.push(',');
                }
                open_line(written, depth + 1);
                written.push_str(&Value::String(key.clone()).to_string());
                written.push(':');
                if !indent.is_empty() {
                    written.push(' ');
                }
                write_json(item, indent, depth + 1, written);
            }
            if !fields.is_empty() {
                open_line(written, depth);
            }
            written.push('}');
        }
    }
}

/// An object's fields in the order JavaScript enumerates them: the keys
/// that are array indices first, ascending, then the others as held. An
/// object Node parsed or built enumerates so, whatever order serde_json
/// read its text in.
fn enumerated(fields: &Map<String, Value>) -> Vec<(&String, &Value)> {
    let mut entries: Vec<_> = fields.iter().collect();
    if entries.iter().any(|(key, _)| array_index(key).is_some()) {
        // A stable sort: the other keys keep their order behind the indices.
        entries.sort_by_key(|(key, _)| array_index(key).map_or((1, 0), |index| (0, index)));
    }
    entries
}

/// ASCII in the order ICU's root collation sorts it at its first level,
/// as Node's `localeCompare` reported it (`tests/goldens/records/tables.json`):
/// white space, punctuation and symbols, digits, then letters, a letter's
/// two cases as one. The other control characters are ignored.
const COLLATED: &str = "\t\n\u{b}\u{c}\r _-,;:!?.'\"()[]{}@*/\\&#%`^+<=>|~$0123456789aAbBcCdDeEfFgGhHiIjJkKlLmMnNoOpPqQrRsStTuUvVwWxXyYzZ";

/// A character's place among `COLLATED` at the first level (a letter's two
/// cases share one), and its case at the third: none for a character ICU
/// ignores, past every ASCII one for any other.
fn collation_weights(character: char) -> Option<(u32, u32)> {
    if character.is_ascii_control() && !"\t\n\u{b}\u{c}\r".contains(character) {
        return None;
    }
    let lower = character.to_ascii_lowercase();
    let primary = COLLATED
        .chars()
        .filter(|collated| !collated.is_ascii_uppercase())
        .position(|collated| collated == lower)
        .map_or(0x100 + u32::from(character), |at| {
            u32::try_from(at).unwrap_or(u32::MAX)
        });
    Some((primary, u32::from(character.is_ascii_uppercase())))
}

/// `left.localeCompare(right)` for text in ASCII: ICU's root collation, its
/// first level (letters as one case, white space and punctuation before
/// digits before letters), then its third (a lowercase letter before its
/// capital). Beyond ASCII it orders by code point, where ICU would weigh
/// each script: no id ConsensFlow sorts holds such a character.
pub fn locale_compare(left: &str, right: &str) -> Ordering {
    let weights = |text: &str| {
        text.chars()
            .filter_map(collation_weights)
            .collect::<Vec<_>>()
    };
    let (left, right) = (weights(left), weights(right));
    let primary =
        |weights: &[(u32, u32)]| weights.iter().map(|weight| weight.0).collect::<Vec<_>>();
    let tertiary =
        |weights: &[(u32, u32)]| weights.iter().map(|weight| weight.1).collect::<Vec<_>>();
    primary(&left)
        .cmp(&primary(&right))
        .then_with(|| tertiary(&left).cmp(&tertiary(&right)))
}

/// `items` as `.join(separator)` joined them, `null` as nothing.
pub fn join(items: &[Value], separator: &str) -> String {
    items
        .iter()
        .map(|item| match item {
            Value::Null => Cow::Borrowed(""),
            item => text(Some(item)),
        })
        .collect::<Vec<_>>()
        .join(separator)
}

/// Whether `value ? … : …` took the first branch.
pub fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(flag)) => *flag,
        Some(Value::Number(number)) => number.as_f64().is_some_and(|number| number != 0.0),
        Some(Value::String(text)) => !text.is_empty(),
        Some(_) => true,
    }
}

/// `text` without what `.trim()` took off: Rust's own `trim` keeps U+FEFF,
/// which JavaScript drops, and drops U+0085, which JavaScript keeps.
pub fn trim(text: &str) -> &str {
    text.trim_matches(is_space)
}

/// `text` without what `.trimStart()` took off.
pub fn trim_start(text: &str) -> &str {
    text.trim_start_matches(is_space)
}

/// Whether JavaScript's `\s` and `.trim()` take `character` for white space.
fn is_space(character: char) -> bool {
    character == '\u{FEFF}' || (character != '\u{85}' && character.is_whitespace())
}

/// `text` as `Number(text)` read it: blank is 0, and what is no number is NaN.
pub fn number(text: &str) -> f64 {
    let text = trim(text);
    if text.is_empty() {
        return 0.0;
    }
    if let Some(value) = radix_number(text) {
        return value;
    }
    let (sign, unsigned) = match text.strip_prefix('-') {
        Some(rest) => (-1.0, rest),
        None => (1.0, text.strip_prefix('+').unwrap_or(text)),
    };
    if unsigned == "Infinity" {
        return sign * f64::INFINITY;
    }
    if !is_decimal(unsigned) {
        return f64::NAN;
    }
    unsigned
        .parse::<f64>()
        .map_or(f64::NAN, |value| sign * value)
}

/// `0x10`, `0o7`, `0b11`: unsigned numbers in another radix, as V8 reads
/// them (`InternalStringToIntDouble`): exact to 53 bits, then rounded once to
/// the nearest double, the even one of two as near, every digit after the
/// 53 bits weighed.
fn radix_number(text: &str) -> Option<f64> {
    let bits = match text.get(..2)? {
        "0x" | "0X" => 4,
        "0o" | "0O" => 3,
        "0b" | "0B" => 1,
        _ => return None,
    };
    let radix = 1 << bits;
    let digits = &text[2..];
    if digits.is_empty() || !digits.chars().all(|digit| digit.is_digit(radix)) {
        return Some(f64::NAN);
    }
    let mut digits = digits
        .chars()
        .filter_map(|digit| digit.to_digit(radix))
        .map(u64::from)
        .skip_while(|digit| *digit == 0);
    let mut number: u64 = 0;
    let mut exponent: i32 = 0;
    while let Some(digit) = digits.next() {
        number = number * u64::from(radix) + digit;
        let over = number >> 53;
        if over == 0 {
            continue;
        }
        // Past 53 bits: the bits over them are dropped, and decide the rounding
        // with every digit after them.
        let dropped_bits = 64 - over.leading_zeros();
        let dropped = number & ((1 << dropped_bits) - 1);
        number >>= dropped_bits;
        exponent = i32::try_from(dropped_bits).ok()?;
        let mut zero_tail = true;
        for digit in digits.by_ref() {
            zero_tail &= digit == 0;
            exponent += bits;
        }
        let half = 1 << (dropped_bits - 1);
        if dropped > half || (dropped == half && (number & 1 == 1 || !zero_tail)) {
            number += 1;
        }
        if number >> 53 != 0 {
            number >>= 1;
            exponent += 1;
        }
        break;
    }
    // Under 2^53, the number is a double as it is; a power of two scales it
    // exactly, or past the largest double, to infinity.
    #[allow(clippy::cast_precision_loss)]
    Some(number as f64 * 2_f64.powi(exponent))
}

/// Digits with an optional fraction and exponent: `5`, `5.`, `.5`, `1e1`.
fn is_decimal(text: &str) -> bool {
    let digits = |part: &str| part.bytes().all(|byte| byte.is_ascii_digit());
    let (mantissa, exponent) = match text.find(['e', 'E']) {
        Some(at) => (&text[..at], Some(&text[at + 1..])),
        None => (text, None),
    };
    let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let exponent = exponent.map(|exponent| exponent.strip_prefix(['+', '-']).unwrap_or(exponent));
    digits(whole)
        && digits(fraction)
        && !(whole.is_empty() && fraction.is_empty())
        && exponent.is_none_or(|exponent| !exponent.is_empty() && digits(exponent))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn writes_values_as_a_template_literal_did() {
        assert_eq!(text(Some(&json!("x"))), "x");
        assert_eq!(text(Some(&json!(3))), "3");
        assert_eq!(text(Some(&json!(true))), "true");
        assert_eq!(text(Some(&json!(null))), "null");
        assert_eq!(text(None), "undefined");
        assert_eq!(text(Some(&json!([1, null, "a", [2, 3]]))), "1,,a,2,3");
        assert_eq!(text(Some(&json!({ "a": 1 }))), "[object Object]");
        assert_eq!(
            join(&[json!("worker"), json!(null), json!("advisor")], "+"),
            "worker++advisor"
        );
    }

    #[test]
    fn writes_a_number_as_string_did() {
        // Node 26's String(x) for each.
        let cases = [
            (1e21, "1e+21"),
            (1.5e-7, "1.5e-7"),
            (123_456_789_012_345_680_000.0, "123456789012345680000"),
            (0.000_001, "0.000001"),
            (1e-7, "1e-7"),
            (-0.0, "0"),
            (2.5, "2.5"),
            (2.0, "2"),
            (100.0, "100"),
            (1.0 / 3.0, "0.3333333333333333"),
            (5e-324, "5e-324"),
            (f64::MAX, "1.7976931348623157e+308"),
            (12_345_678_901_234_567_890.0, "12345678901234567000"),
            (0.1 + 0.2, "0.30000000000000004"),
            (-1.25e-9, "-1.25e-9"),
            (f64::NAN, "NaN"),
            (f64::NEG_INFINITY, "-Infinity"),
        ];
        for (number, written) in cases {
            assert_eq!(number_text(number), written, "{number:e}");
        }
        assert_eq!(text(Some(&serde_json::from_str("2.0").unwrap())), "2");
        assert_eq!(text(Some(&serde_json::from_str("1e21").unwrap())), "1e+21");
    }

    #[test]
    fn writes_json_as_json_stringify_did() {
        let read = |text: &str| serde_json::from_str::<Value>(text).unwrap();
        assert_eq!(
            stringify(&read(r#"{"b":2.0,"a":[1e21,null,"x\u0001\"y"],"c":{}}"#)),
            r#"{"b":2,"a":[1e+21,null,"x\u0001\"y"],"c":{}}"#
        );
        // An integer past 2^53 is the double JavaScript reads it as.
        assert_eq!(
            stringify(&read("12345678901234567890")),
            "12345678901234567000"
        );
        assert_eq!(
            stringify(&json!("line\nbreak\u{2028}")),
            "\"line\\nbreak\u{2028}\""
        );
        // JSON.stringify(value, null, 2), as Node 26 writes it.
        assert_eq!(
            stringify_indented(&read(r#"{"a":1.0,"b":[1,[],{}],"c":{"d":null},"e":[]}"#), 2),
            "{\n  \"a\": 1,\n  \"b\": [\n    1,\n    [],\n    {}\n  ],\n  \"c\": {\n    \"d\": null\n  },\n  \"e\": []\n}"
        );
        assert_eq!(stringify_indented(&read("[]"), 2), "[]");
        assert_eq!(stringify_indented(&json!("x"), 2), "\"x\"");
    }

    #[test]
    fn a_value_is_text_as_string_makes_it_or_fails_where_v8_threw() {
        let made = |value: Value| string(Some(&value)).map(Cow::into_owned);
        // Node: String(JSON.parse(text)) for each.
        assert_eq!(made(json!([1, [2, null], "x"])).unwrap(), "1,2,,x");
        assert_eq!(made(json!({ "valueOf": 1 })).unwrap(), "[object Object]");
        assert_eq!(string(None).unwrap(), "undefined");
        for thrown in [
            json!({ "toString": null }),
            json!({ "toString": "x" }),
            json!([1, [{ "toString": 0 }]]),
        ] {
            assert!(made(thrown.clone()).is_err(), "{thrown}");
        }
    }

    #[test]
    fn writes_keys_that_are_array_indices_first_whatever_order_they_were_read_in() {
        let read = |text: &str| serde_json::from_str::<Value>(text).unwrap();
        // Node: JSON.stringify(JSON.parse(text)).
        assert_eq!(
            stringify(&read(
                r#"{"b":1,"10":2,"a":{"z":0,"2":1,"1":2},"01":3,"4294967295":4,"4294967294":5}"#
            )),
            r#"{"10":2,"4294967294":5,"b":1,"a":{"1":2,"2":1,"z":0},"01":3,"4294967295":4}"#
        );
        assert_eq!(
            stringify_indented(&read(r#"{"b":1,"0":2}"#), 1),
            "{\n \"0\": 2,\n \"b\": 1\n}"
        );
    }

    #[test]
    fn orders_ascii_as_locale_compare_did() {
        fn sorted<'a>(words: &[&'a str]) -> Vec<&'a str> {
            let mut words = words.to_vec();
            words.sort_by(|left, right| locale_compare(left, right));
            words
        }
        // Node 26 (ICU 78) sorts each list so.
        assert_eq!(
            sorted(&["b", "AB", "Ab", "aB", "ab", "A", "a"]),
            ["a", "A", "ab", "aB", "Ab", "AB", "b"]
        );
        assert_eq!(
            sorted(&["aB", "ab", "a1", "a-b", "a_b", "a b"]),
            ["a b", "a_b", "a-b", "a1", "ab", "aB"]
        );
        assert_eq!(
            sorted(&["a2", "a10", "2", "1a", "10"]),
            ["10", "1a", "2", "a10", "a2"]
        );
        assert_eq!(locale_compare("\t", " "), Ordering::Less);
        assert_eq!(
            locale_compare("a\u{1}b", "ab"),
            Ordering::Equal,
            "a control character is ignored"
        );
        assert_eq!(
            locale_compare("a\tb", "ab"),
            Ordering::Less,
            "white space is not"
        );
        assert_eq!(locale_compare("\u{7f}", "a"), Ordering::Less);
    }

    #[test]
    fn tells_truth_as_javascript_does() {
        for falsy in [
            None,
            Some(json!(null)),
            Some(json!(false)),
            Some(json!(0)),
            Some(json!("")),
        ] {
            assert!(!truthy(falsy.as_ref()), "{falsy:?}");
        }
        for true_ in [json!(true), json!(1), json!("0"), json!([]), json!({})] {
            assert!(truthy(Some(&true_)), "{true_:?}");
        }
    }

    #[test]
    fn trims_what_javascript_trims() {
        assert_eq!(trim(" \t\n\u{A0}\u{3000}x y\u{2028}\r "), "x y");
        assert_eq!(trim("\u{FEFF}x\u{FEFF}"), "x");
        assert_eq!(trim("\u{85}x\u{85}"), "\u{85}x\u{85}");
    }

    #[test]
    fn trims_the_start_alone_as_javascript_does() {
        assert_eq!(trim_start(" \t\n\u{A0}\u{FEFF}x y \n"), "x y \n");
        assert_eq!(trim_start("\u{85}x"), "\u{85}x");
        assert_eq!(trim_start(" \n"), "");
    }

    #[test]
    fn reads_a_number_as_number_did() {
        let cases = [
            ("5", 5.0),
            (" 5 ", 5.0),
            ("5.0", 5.0),
            ("1e1", 10.0),
            ("0x10", 16.0),
            ("0o7", 7.0),
            ("0b11", 3.0),
            ("+5", 5.0),
            ("-5", -5.0),
            ("", 0.0),
            ("  ", 0.0),
            (".5", 0.5),
            ("5.", 5.0),
            ("Infinity", f64::INFINITY),
            ("-Infinity", f64::NEG_INFINITY),
        ];
        for (written, value) in cases {
            assert_eq!(number(written), value, "{written:?}");
        }
        for written in [
            "inf", "nan", "1_000", "5x", "0x", "+0x10", "1e", ".", "+-5", "\u{663}",
        ] {
            assert!(number(written).is_nan(), "{written:?} is no number");
        }
    }

    #[test]
    fn a_number_in_another_radix_is_rounded_once_as_v8_rounds_it() {
        // Node 26's `String(Number(text))`.
        let f = |count: usize| "f".repeat(count);
        let cases = [
            (format!("0b1{}101", "0".repeat(52)), "36028797018963976"),
            ("0x1fffffffffffff".to_owned(), "9007199254740991"),
            // Half way: to the even one.
            ("0x20000000000001".to_owned(), "9007199254740992"),
            ("0x20000000000003".to_owned(), "9007199254740996"),
            // Half way but for a digit far after: up.
            (
                "0x200000000000010000000000000001".to_owned(),
                "1.6615349947311452e+35",
            ),
            (
                format!("0x20000000000001{}", "0".repeat(40)),
                "1.3164036458569648e+64",
            ),
            (format!("0x{}", f(256)), "Infinity"),
            (format!("0x{}7", f(255)), "Infinity"),
            (format!("0x{}", f(300)), "Infinity"),
            (format!("0o{}", "7".repeat(30)), "1.2379400392853803e+27"),
            (format!("0o1{}1", "0".repeat(20)), "9223372036854776000"),
            ("0X000".to_owned(), "0"),
            ("0b0".to_owned(), "0"),
            (
                format!("0x{}1fffffffffffff8", "0".repeat(40)),
                "144115188075855870",
            ),
            (format!("0B{}", "1".repeat(55)), "36028797018963970"),
            ("0x12AbCdEf".to_owned(), "313249263"),
        ];
        for (written, read) in cases {
            assert_eq!(number_text(number(&written)), read, "{written}");
        }
    }
}

//! JavaScript's readings of values and text, for ports of code that relied
//! on them: a JSON value written into a template literal, tested for truth,
//! joined; text trimmed or read as a number. A port that reads a value the
//! way the Node code did says what Node said.

use std::borrow::Cow;

use serde_json::Value;

/// `value` as `${value}` wrote it; `None` is a field that was not there.
pub fn text(value: Option<&Value>) -> Cow<'_, str> {
    match value {
        None => Cow::Borrowed("undefined"),
        Some(Value::Null) => Cow::Borrowed("null"),
        Some(Value::String(text)) => Cow::Borrowed(text),
        Some(Value::Array(items)) => Cow::Owned(join(items, ",")),
        Some(Value::Object(_)) => Cow::Borrowed("[object Object]"),
        Some(other) => Cow::Owned(other.to_string()),
    }
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
    text.trim_matches(|character: char| {
        character == '\u{FEFF}' || (character != '\u{85}' && character.is_whitespace())
    })
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

/// `0x10`, `0o7`, `0b11`: unsigned numbers in another radix.
fn radix_number(text: &str) -> Option<f64> {
    let radix = match text.get(..2)? {
        "0x" | "0X" => 16,
        "0o" | "0O" => 8,
        "0b" | "0B" => 2,
        _ => return None,
    };
    let digits = &text[2..];
    if digits.is_empty() || !digits.chars().all(|digit| digit.is_digit(radix)) {
        return Some(f64::NAN);
    }
    Some(
        digits
            .chars()
            .filter_map(|digit| digit.to_digit(radix))
            .fold(0.0, |value, digit| {
                value * f64::from(radix) + f64::from(digit)
            }),
    )
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
}

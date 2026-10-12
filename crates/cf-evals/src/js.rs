//! What the numbers and the texts of the evals owe to JavaScript, which wrote
//! every report there is: a length is counted in UTF-16 code units, a text is cut
//! at them, white space is the set `\s` and `trim` know, and a number is shown as
//! `String` shows it. The reports are kept, and a report read now must read as it
//! was written, so these are held to JavaScript's own answers.

use serde_json::Value;

/// How long `text` is, as JavaScript's `length` says: in UTF-16 code units, which
/// a character beyond the first plane takes two of.
pub fn length(text: &str) -> usize {
    text.encode_utf16().count()
}

/// The first `most` code units of `text`, as `slice(0, most)` keeps them, but for
/// a character that would be cut in two (the first half of a pair): a string here
/// cannot hold half of one, and drops it whole.
pub fn first(text: &str, most: usize) -> String {
    let mut taken = 0;
    let mut kept = String::new();
    for at in text.chars() {
        taken += at.len_utf16();
        if taken > most {
            break;
        }
        kept.push(at);
    }
    kept
}

/// `text` with spaces after it to `width` code units, as `padEnd` pads it.
pub fn pad_end(text: &str, width: usize) -> String {
    format!("{text}{}", " ".repeat(width.saturating_sub(length(text))))
}

/// `text` with spaces before it to `width` code units, as `padStart` pads it.
pub fn pad_start(text: &str, width: usize) -> String {
    format!("{}{text}", " ".repeat(width.saturating_sub(length(text))))
}

/// `text` with each run of white space made one space, as `replace(/\s+/g, ' ')`.
pub fn squeeze(text: &str) -> String {
    let mut squeezed = String::with_capacity(text.len());
    let mut spaced = false;
    for at in text.chars() {
        if is_space(at) {
            if !spaced {
                squeezed.push(' ');
            }
            spaced = true;
        } else {
            squeezed.push(at);
            spaced = false;
        }
    }
    squeezed
}

/// `count` with a comma between each three digits, as `toLocaleString('en-US')`
/// writes a whole number.
pub fn thousands(count: u64) -> String {
    let digits = count.to_string();
    let mut grouped = String::new();
    for (at, digit) in digits.chars().enumerate() {
        if at > 0 && (digits.len() - at).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    grouped
}

/// Whether `at` is white space as JavaScript has it (`\s`, and what `trim`
/// takes off): this is not Unicode's `White_Space`, which has the next-line mark
/// and not the byte order mark.
pub fn is_space(at: char) -> bool {
    matches!(
        at,
        '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200a}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202f}'
                | '\u{205f}'
                | '\u{3000}'
                | '\u{feff}'
    )
}

/// `text` without the white space at its ends, as `trim` leaves it.
pub fn trim(text: &str) -> &str {
    text.trim_matches(is_space)
}

/// What `String(value)` says of a JSON value a report holds: a string as it is,
/// a number as JavaScript writes it, `null`, and `undefined` for what is not
/// there (a report from before the field existed). An array is its items joined
/// with commas, and an object is `[object Object]`, which no report holds.
pub fn string(value: Option<&Value>) -> String {
    match value {
        None => "undefined".to_owned(),
        Some(Value::Null) => "null".to_owned(),
        Some(Value::Bool(flag)) => flag.to_string(),
        Some(Value::Number(number)) => number_text(number),
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| match item {
                Value::Null => String::new(),
                other => string(Some(other)),
            })
            .collect::<Vec<_>>()
            .join(","),
        Some(Value::Object(_)) => "[object Object]".to_owned(),
    }
}

/// A number as JavaScript writes it: a whole one without a decimal point, and
/// any other by the shortest digits that read back the same.
fn number_text(number: &serde_json::Number) -> String {
    if let Some(whole) = number.as_i64() {
        return whole.to_string();
    }
    if let Some(whole) = number.as_u64() {
        return whole.to_string();
    }
    match number.as_f64() {
        Some(real) if real.fract() == 0.0 && real.abs() < 1e21 => format!("{real:.0}"),
        Some(real) => real.to_string(),
        None => number.to_string(),
    }
}

/// Whether the report field is set the way JavaScript's `??` and `?.` ask: there,
/// and not `null`.
pub fn present(value: Option<&Value>) -> Option<&Value> {
    value.filter(|value| !value.is_null())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_length_counts_the_code_units_a_character_beyond_the_first_plane_takes_two_of() {
        assert_eq!(length("abc"), 3);
        assert_eq!(length("ș"), 1);
        assert_eq!(length("😀"), 2);
        assert_eq!(length("a😀b"), 4);
        assert_eq!(length(""), 0);
    }

    #[test]
    fn a_text_is_cut_at_code_units_and_a_pair_is_never_left_in_halves() {
        assert_eq!(first("abcdef", 3), "abc");
        assert_eq!(first("abc", 10), "abc");
        assert_eq!(first("a😀b", 3), "a😀");
        // JavaScript would keep half of the pair; a string here drops it.
        assert_eq!(first("a😀b", 2), "a");
        assert_eq!(first("😀", 0), "");
    }

    #[test]
    fn white_space_is_the_set_javascript_has_and_not_unicodes() {
        for yes in [
            '\t', '\n', '\r', ' ', '\u{a0}', '\u{2003}', '\u{2028}', '\u{3000}', '\u{feff}',
        ] {
            assert!(is_space(yes), "{yes:?}");
        }
        // The next-line mark is white space to Unicode and to Rust's trim, and not to JavaScript.
        assert!(!is_space('\u{85}'));
        assert!('\u{85}'.is_whitespace());
        assert!(!is_space('a'));
        assert_eq!(trim(" \u{feff}x\u{85}\n"), "x\u{85}");
    }

    #[test]
    fn a_text_is_padded_to_a_width_in_code_units_and_never_cut() {
        assert_eq!(pad_end("ab", 5), "ab   ");
        assert_eq!(pad_start("ab", 5), "   ab");
        assert_eq!(pad_end("abcdef", 3), "abcdef");
        assert_eq!(pad_start("😀", 4), "  😀");
        assert_eq!(pad_end("", 2), "  ");
    }

    #[test]
    fn white_space_is_squeezed_to_one_space_and_numbers_get_their_commas() {
        assert_eq!(squeeze("a \n\t b  \u{a0}c"), "a b c");
        assert_eq!(squeeze("  x  "), " x ");
        assert_eq!(squeeze(""), "");
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1000), "1,000");
        assert_eq!(thousands(1_234_567), "1,234,567");
        assert_eq!(thousands(12_345), "12,345");
    }

    #[test]
    fn a_value_is_shown_as_string_shows_it() {
        assert_eq!(string(None), "undefined");
        assert_eq!(string(Some(&json!(null))), "null");
        assert_eq!(string(Some(&json!(true))), "true");
        assert_eq!(string(Some(&json!(338))), "338");
        assert_eq!(string(Some(&json!(2.5))), "2.5");
        assert_eq!(string(Some(&json!(3.0))), "3");
        assert_eq!(string(Some(&json!("gpt-5.6-luna"))), "gpt-5.6-luna");
        assert_eq!(string(Some(&json!(["claude", null, "pi"]))), "claude,,pi");
        assert_eq!(string(Some(&json!({}))), "[object Object]");
    }

    #[test]
    fn a_field_that_is_null_is_as_absent_as_one_that_is_not_there() {
        assert!(present(None).is_none());
        assert!(present(Some(&json!(null))).is_none());
        assert_eq!(present(Some(&json!(0))), Some(&json!(0)));
    }
}

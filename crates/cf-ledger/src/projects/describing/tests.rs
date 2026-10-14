use serde_json::json;

use super::*;

fn description(text: &str) -> Result<Option<String>, String> {
    description_of(text).map_err(|refused| refused.to_string())
}

#[test]
fn a_description_is_kept_trimmed_and_blank_text_is_none() {
    for (given, kept) in [
        ("Moves the cargo", Some("Moves the cargo")),
        ("  Moves the cargo \t", Some("Moves the cargo")),
        // A line break at either end goes with the other spaces.
        ("\nMoves the cargo\r\n", Some("Moves the cargo")),
        ("café · планы · 日本語", Some("café · планы · 日本語")),
        ("<b>not markup</b>", Some("<b>not markup</b>")),
        ("", None),
        ("   ", None),
        (" \t\n\u{a0}\u{feff} ", None),
    ] {
        assert_eq!(description(given), Ok(kept.map(str::to_owned)), "{given:?}");
    }
}

#[test]
fn a_description_has_two_hundred_characters_as_javascript_counts_them_and_no_more() {
    let fits = [
        "a".repeat(DESCRIPTION_MAX),
        "é".repeat(DESCRIPTION_MAX),
        // Two units each in JavaScript: a hundred of them are two hundred.
        "😀".repeat(DESCRIPTION_MAX / 2),
        // The spaces at either end are not part of what is kept.
        format!("  {}  ", "a".repeat(DESCRIPTION_MAX)),
    ];
    for text in fits {
        assert!(description(&text).is_ok(), "{} units", utf16_len(&text));
    }
    let too_long = [
        "a".repeat(DESCRIPTION_MAX + 1),
        "😀".repeat(DESCRIPTION_MAX / 2 + 1),
        format!("a{}", "😀".repeat(DESCRIPTION_MAX / 2)),
    ];
    for text in too_long {
        assert_eq!(
            description(&text),
            Err(
                "a project's description is one line of plain text: it is at most 200 characters"
                    .to_owned()
            ),
            "{} units",
            utf16_len(&text)
        );
    }
}

#[test]
fn a_line_break_and_any_other_control_character_inside_the_text_is_refused_and_never_made_a_space()
{
    for text in [
        "one\ntwo",
        "one\r\ntwo",
        "one\rtwo",
        "one\u{85}two",
        "one\u{2028}two",
        "one\u{2029}two",
        "one\u{b}two",
        "one\u{c}two",
        "one\ttwo",
        "one\0two",
        "one\u{1b}[31mtwo",
    ] {
        assert_eq!(
            description(text),
            Err("a project's description is one line of plain text: it has no line breaks or other control characters"
                .to_owned()),
            "{text:?}"
        );
    }
}

#[test]
fn a_description_that_is_not_text_is_refused_and_a_missing_one_or_null_takes_it_away() {
    let parsed = |value: Option<serde_json::Value>| {
        parse_description(value.as_ref()).map_err(|refused| refused.to_string())
    };
    assert_eq!(parsed(None), Ok(None));
    assert_eq!(parsed(Some(json!(null))), Ok(None));
    assert_eq!(parsed(Some(json!(" "))), Ok(None));
    assert_eq!(parsed(Some(json!(" hi "))), Ok(Some("hi".to_owned())));
    for value in [json!(5), json!(true), json!(["a"]), json!({ "text": "a" })] {
        assert_eq!(
            parsed(Some(value.clone())),
            Err(
                "a project's description is one line of plain text: it is text, or none to take it away"
                    .to_owned()
            ),
            "{value}"
        );
    }
}

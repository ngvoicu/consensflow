use serde_json::json;

use super::*;

fn rules(text: &str) -> Result<Option<String>, String> {
    rules_of(text).map_err(|refused| refused.to_string())
}

#[test]
fn the_rules_are_kept_trimmed_with_their_lines_and_tabs_and_blank_text_is_none() {
    for (given, kept) in [
        ("Push to origin only.", Some("Push to origin only.")),
        (
            "  - Push to origin only.\n- Test models:\tHaiku.\n\n",
            Some("- Push to origin only.\n- Test models:\tHaiku."),
        ),
        ("café · планы · 日本語", Some("café · планы · 日本語")),
        ("<b>not markup</b>", Some("<b>not markup</b>")),
        ("", None),
        ("   ", None),
        (" \t\n\u{a0}\u{feff} ", None),
    ] {
        assert_eq!(rules(given), Ok(kept.map(str::to_owned)), "{given:?}");
    }
}

#[test]
fn the_rules_have_eight_thousand_bytes_and_no_more_whatever_characters_hold_them() {
    let fits = [
        "a".repeat(STAFF_RULES_MAX_BYTES),
        // Two bytes each: four thousand of them are eight thousand bytes.
        "é".repeat(STAFF_RULES_MAX_BYTES / 2),
        // Four bytes each.
        "😀".repeat(STAFF_RULES_MAX_BYTES / 4),
        // The spaces at either end are not part of what is kept.
        format!("  {}  ", "a".repeat(STAFF_RULES_MAX_BYTES)),
    ];
    for text in fits {
        assert!(rules(&text).is_ok(), "{} bytes", text.len());
    }
    for text in [
        "a".repeat(STAFF_RULES_MAX_BYTES + 1),
        // Three bytes each: 2,667 of them are 8,001 bytes, though they are
        // fewer characters than the bound says bytes.
        "日".repeat(STAFF_RULES_MAX_BYTES / 3 + 1),
        format!("a{}", "😀".repeat(STAFF_RULES_MAX_BYTES / 4)),
    ] {
        assert_eq!(
            rules(&text),
            Err(format!(
                "the rules for the staff are plain text: they are at most 8000 bytes, and these are {}",
                text.len()
            )),
        );
    }
}

#[test]
fn the_rules_have_a_hundred_and_fifty_lines_and_no_more_blank_lines_among_them() {
    let lines = |count: usize| vec!["a rule"; count].join("\n");
    assert!(rules(&lines(STAFF_RULES_MAX_LINES)).is_ok());
    // A blank line inside them is a line.
    let with_blanks = vec!["a rule"; STAFF_RULES_MAX_LINES / 2 + 1].join("\n\n");
    assert_eq!(
        rules(&with_blanks),
        Err(
            "the rules for the staff are plain text: they are at most 150 lines, and these are 151"
                .to_owned()
        )
    );
    // Line breaks at either end are trimmed, and are no lines of the rules.
    assert!(rules(&format!("\n\n{}\n\n", lines(STAFF_RULES_MAX_LINES))).is_ok());
}

#[test]
fn a_control_character_other_than_a_line_break_or_a_tab_is_refused() {
    for text in ["a\rb", "a\r\nb", "a\u{7}b", "a\u{0}b", "a\u{1b}[31mb"] {
        assert_eq!(
            rules(text),
            Err("the rules for the staff are plain text: they have no control characters but line breaks and tabs".to_owned()),
            "{text:?}"
        );
    }
}

#[test]
fn the_rules_are_text_or_none_and_nothing_else() {
    assert_eq!(parse_staff_rules(None).unwrap(), None);
    assert_eq!(parse_staff_rules(Some(&json!(null))).unwrap(), None);
    assert_eq!(parse_staff_rules(Some(&json!("  "))).unwrap(), None);
    assert_eq!(
        parse_staff_rules(Some(&json!(" Push to origin only. "))).unwrap(),
        Some("Push to origin only.".to_owned())
    );
    for value in [json!(1), json!(true), json!(["a"]), json!({ "text": "a" })] {
        assert_eq!(
            parse_staff_rules(Some(&value)).unwrap_err().to_string(),
            "the rules for the staff are plain text: they are text, or none to take them away",
            "{value}"
        );
    }
}

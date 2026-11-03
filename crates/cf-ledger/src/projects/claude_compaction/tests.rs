use serde_json::json;

use super::*;

fn window(value: serde_json::Value) -> Result<Option<u32>, String> {
    parse_claude_compaction(Some(&value)).map_err(|refused| refused.to_string())
}

const WORDS: &str = "the compaction window for Claude members is a whole number of tokens from 100000 to 1000000, or none for Claude's own point";

#[test]
fn a_size_from_a_hundred_thousand_to_a_million_tokens_is_kept_as_it_is() {
    for tokens in [100_000, 100_001, 400_000, 999_999, 1_000_000] {
        assert_eq!(window(json!(tokens)), Ok(Some(tokens)), "{tokens}");
    }
}

#[test]
fn no_size_is_claudes_own_point_a_missing_field_null_or_nothing_else() {
    assert_eq!(parse_claude_compaction(None).unwrap(), None);
    assert_eq!(parse_claude_compaction(Some(&json!(null))).unwrap(), None);
}

#[test]
fn a_size_below_or_above_what_claude_takes_is_refused_in_the_words_that_say_the_range() {
    for tokens in [
        json!(0),
        json!(1),
        json!(99_999),
        json!(1_000_001),
        json!(u64::MAX),
    ] {
        assert_eq!(window(tokens.clone()), Err(WORDS.to_owned()), "{tokens}");
    }
}

#[test]
fn only_a_whole_number_is_a_size() {
    for value in [
        json!(-400_000),
        json!(400_000.5),
        json!("400000"),
        json!("400k"),
        json!(""),
        json!(true),
        json!([400_000]),
        json!({ "tokens": 400_000 }),
    ] {
        assert_eq!(window(value.clone()), Err(WORDS.to_owned()), "{value}");
    }
}

#[test]
fn the_range_is_claudes_own_hundred_thousand_to_a_million() {
    assert_eq!(CLAUDE_COMPACTION_MIN, 100_000);
    assert_eq!(CLAUDE_COMPACTION_MAX, 1_000_000);
}

use serde_json::json;

use super::*;

fn window(value: serde_json::Value) -> Result<Option<u32>, String> {
    parse_compaction(Some(&value)).map_err(|refused| refused.to_string())
}

const WORDS: &str = "the compaction size is a whole number of tokens from 200000 to 1000000, or none for each harness's own point";

#[test]
fn a_size_from_two_hundred_thousand_to_a_million_tokens_is_kept_as_it_is() {
    for tokens in [200_000, 200_001, 400_000, 999_999, 1_000_000] {
        assert_eq!(window(json!(tokens)), Ok(Some(tokens)), "{tokens}");
    }
}

#[test]
fn no_size_is_claudes_own_point_a_missing_field_null_or_nothing_else() {
    assert_eq!(parse_compaction(None).unwrap(), None);
    assert_eq!(parse_compaction(Some(&json!(null))).unwrap(), None);
}

#[test]
fn a_size_below_or_above_what_claude_takes_is_refused_in_the_words_that_say_the_range() {
    for tokens in [
        json!(0),
        json!(1),
        // The old floor, and the sizes under the new one, at which a window that has just
        // compacted is already over its harness's own firing point (T-1439).
        json!(99_999),
        json!(100_000),
        json!(150_000),
        json!(199_999),
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
fn the_range_is_two_hundred_thousand_to_a_million() {
    assert_eq!(COMPACTION_MIN, 200_000);
    assert_eq!(COMPACTION_MAX, 1_000_000);
}

/// Why the floor is where it is (T-1439). Claude Code compacts at the size less 33,000
/// (20,000 of room for its answer and a 13,000 buffer) and, when a window refills to that
/// point within three turns of a compaction three times in a row, ends the turn with an error
/// instead (`autocompact_thrashing`). The largest context the next call had after a compaction
/// that fired under 300,000 was 120,000 (50 of T-1436's, all roles); after the ones that fired
/// at 967,000 it reached 161,000, a kept part of 82,000 that the smaller windows did not show.
/// A call adds about 2,500: the firing point must lie above the context after a compaction and
/// three calls on, or a window compacts at once and then stops doing work. The other harnesses
/// leave far less after a compaction (Codex 39,000, Devin 75,000, Pi 102,000 seen), so
/// Claude's is the line that binds. It makes thrashing unlikely, not impossible: the context a
/// compaction leaves grows with the owner's MCP servers, skills and rules.
#[test]
fn the_floor_leaves_a_claude_window_room_above_the_largest_context_a_compaction_left() {
    const CLAUDES_MARGIN: u32 = 33_000;
    const LARGEST_AFTER_A_COMPACTION: u32 = 120_000;
    const THREE_CALLS: u32 = 3 * 2_500;
    assert!(
        COMPACTION_MIN - CLAUDES_MARGIN >= LARGEST_AFTER_A_COMPACTION + THREE_CALLS,
        "{COMPACTION_MIN} fires Claude Code at {} and a window refills past that at once",
        COMPACTION_MIN - CLAUDES_MARGIN
    );
}

#[test]
fn the_size_just_under_the_floor_is_refused_and_the_floor_itself_is_a_size() {
    assert_eq!(window(json!(COMPACTION_MIN - 1)), Err(WORDS.to_owned()));
    assert_eq!(window(json!(COMPACTION_MIN)), Ok(Some(COMPACTION_MIN)));
}

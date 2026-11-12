use serde_json::json;

use super::*;

fn window(value: serde_json::Value) -> Result<Option<u32>, String> {
    parse_compaction(Some(&value)).map_err(|refused| refused.to_string())
}

const WORDS: &str = "the compaction size is a whole number of tokens from 250000 to 1000000, or none for each harness's own point";

#[test]
fn a_size_from_a_quarter_of_a_million_to_a_million_tokens_is_kept_as_it_is() {
    for tokens in [250_000, 250_001, 400_000, 999_999, 1_000_000] {
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
        json!(200_000),
        json!(249_999),
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
fn a_new_project_that_says_nothing_starts_on_300000_and_only_null_is_the_harness_s_own_point() {
    assert_eq!(COMPACTION_DEFAULT, 300_000);
    assert!((COMPACTION_MIN..=COMPACTION_MAX).contains(&COMPACTION_DEFAULT));
    assert_eq!(
        parse_new_compaction(None).unwrap(),
        Some(COMPACTION_DEFAULT)
    );
    assert_eq!(parse_new_compaction(Some(&json!(null))).unwrap(), None);
    assert_eq!(
        parse_new_compaction(Some(&json!(250_000))).unwrap(),
        Some(250_000)
    );
    // What is said and is no size is refused, as a change to a project refuses it.
    for value in [json!(249_999), json!("300000"), json!(true)] {
        assert_eq!(
            parse_new_compaction(Some(&value)).map_err(|refused| refused.to_string()),
            Err(WORDS.to_owned()),
            "{value}"
        );
    }
    // A change to an existing project that says nothing is still the harness's own point.
    assert_eq!(parse_compaction(None).unwrap(), None);
}

#[test]
fn the_range_is_a_quarter_of_a_million_to_a_million() {
    assert_eq!(COMPACTION_MIN, 250_000);
    assert_eq!(COMPACTION_MAX, 1_000_000);
}

/// Why the floor is where it is (T-1439). Claude Code compacts at the size less 33,000
/// (20,000 of room for its answer and a 13,000 buffer) and, when a window refills to that
/// point within three turns of a compaction three times in a row, ends the turn with an error
/// instead (`autocompact_thrashing`). The largest context the next call had after a compaction
/// was 161,268 (T-1436's 60 real worker compactions; at most 120,000 after the ones that fired
/// under 300,000, 161,268 after one that fired at 967,000: what a compaction leaves at a smaller
/// firing point was not measured below 267,000). A call adds about 2,500. On the real Claude
/// Code 2.1.322 against a stub, a window that left 161,268 compacted 3 times in 15 calls at
/// 200,000, once at 225,000 and not at all at 250,000. So the firing point must lie at least
/// twenty calls above the largest context left, or a window compacts every few calls and the
/// compaction costs more than it saves (T-1436's replay with about 104,000 left after a
/// compaction, which is 13,000 of room at 150,000 and 63,000 at 200,000: 150,000 cost 9.4% more
/// than no setting, 200,000 saved 32.5%). The other harnesses leave far
/// less after a compaction (Codex 39,000, Devin 75,000 where it took effect, Pi 102,000 seen),
/// so Claude's is the line that binds. It makes thrashing unlikely, not impossible: the context
/// a compaction leaves grows with the owner's MCP servers, skills and rules.
#[test]
fn the_floor_leaves_a_claude_window_twenty_calls_above_the_largest_context_a_compaction_left() {
    const CLAUDES_MARGIN: u32 = 33_000;
    const LARGEST_AFTER_A_COMPACTION: u32 = 161_268;
    const TWENTY_CALLS: u32 = 20 * 2_500;
    // Where Claude Code fires for a size (a function, so that the sum is a test and not a
    // constant the compiler has already settled).
    let fires_at = |size: u32| size - CLAUDES_MARGIN;
    assert!(
        fires_at(COMPACTION_MIN) >= LARGEST_AFTER_A_COMPACTION + TWENTY_CALLS,
        "{COMPACTION_MIN} fires Claude Code at {} and a window that left {LARGEST_AFTER_A_COMPACTION} compacts every few calls",
        fires_at(COMPACTION_MIN)
    );
}

#[test]
fn the_size_just_under_the_floor_is_refused_and_the_floor_itself_is_a_size() {
    assert_eq!(window(json!(COMPACTION_MIN - 1)), Err(WORDS.to_owned()));
    assert_eq!(window(json!(COMPACTION_MIN)), Ok(Some(COMPACTION_MIN)));
}

#[test]
fn a_launch_is_given_the_least_for_a_size_the_row_holds_under_it_and_every_other_size_as_it_is() {
    // 3.0.7-alpha.4 took any size from 100,000, so a row can hold one of them (T-1439).
    for stored in [0, 1, 100_000, 150_000, 200_000, 249_999] {
        assert_eq!(launch_size(stored), COMPACTION_MIN, "{stored}");
    }
    for stored in [250_000, 250_001, 300_000, 999_999, 1_000_000] {
        assert_eq!(launch_size(stored), stored, "{stored}");
    }
    // What a launch is given is always one the ledger itself would have taken.
    for stored in [0, 100_000, 249_999, 250_000, 1_000_000] {
        assert_eq!(size_of(launch_size(stored)).unwrap(), launch_size(stored));
    }
}

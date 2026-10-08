use super::*;
use serde_json::json;

#[test]
fn a_digit_is_one_ascii_digit() {
    for digit in ["0", "5", "9"] {
        assert!(is_digit(digit), "{digit}");
    }
    for not in ["", "55", "a", "-", "٥", "５", "5\n", " 5"] {
        assert!(!is_digit(not), "{not:?}");
    }
}

#[test]
fn the_dash_in_an_anthropic_version_becomes_a_dot_where_the_pattern_matched() {
    for (key, dotted) in [
        ("claude-fable-5-1", "claude-fable-5.1"),
        ("claude-opus-5-5", "claude-opus-5.5"),
        ("claude-sonnet-5-5", "claude-sonnet-5.5"),
        ("claude-sonnet-0-9", "claude-sonnet-0.9"),
        ("claude-haiku-5-5", "claude-haiku-5.5"),
        ("claude-haiku-4-5", "claude-haiku-4.5"),
    ] {
        assert_eq!(claude_version_dotted(key).as_deref(), Some(dotted), "{key}");
    }
    for key in [
        "claude-fable-15-1",
        "claude-fable-5-10",
        "claude-fable-٥-1",
        "claude-fable-5-٥",
        "claude-haiku-4-5-20251001",
        "claude-instant-1-2",
        "claude-Fable-5-1",
        "Claude-fable-5-1",
        "claude-fable-5-1-x",
        "claude-fable-5-1\n",
        "claude-fable-5",
        "claude-fable--1",
        "claude-fable-5.1",
        "claude--5-1",
        "claude-5-1",
        "xclaude-fable-5-1",
        "openrouter/anthropic/claude-fable-5-1",
        "",
    ] {
        assert_eq!(claude_version_dotted(key), None, "{key:?}");
    }
}

#[test]
fn the_dashes_in_a_gpt_version_become_a_dot_where_the_pattern_matched() {
    for (key, dotted) in [
        ("gpt-6-1-sol", "gpt-6.1-sol"),
        ("gpt-5-6-terra", "gpt-5.6-terra"),
        ("gpt-0-0-x", "gpt-0.0-x"),
    ] {
        assert_eq!(gpt_version_dotted(key).as_deref(), Some(dotted), "{key}");
    }
    for key in [
        "gpt-6-astra",
        "gpt-6-1-Sol",
        "gpt-6-1-sol2",
        "gpt-6-1-sol-x",
        "gpt-6-1-so_l",
        "gpt-6-1-",
        "gpt-6-1",
        "gpt-6-10-sol",
        "gpt-16-1-sol",
        "gpt-٦-1-sol",
        "gpt-6-1-sól",
        "GPT-6-1-sol",
        "gpt-6-1-sol\n",
        "gpt-6.1-sol",
        "xgpt-6-1-sol",
        "",
    ] {
        assert_eq!(gpt_version_dotted(key), None, "{key:?}");
    }
}

#[test]
fn the_contributor_routes_of_muse_spark_become_its_key_and_nothing_else_does() {
    for key in [
        "muse-spark-1.3-contributor",
        "muse-spark-1.3-contributor-free",
    ] {
        assert_eq!(
            muse_spark_plain(key).as_deref(),
            Some("muse-spark-1.3"),
            "{key}"
        );
    }
    for key in [
        "muse-spark-1.3",
        "muse-spark-1.3-contributor-paid",
        "muse-spark-1.3-contributor-free-",
        "muse-spark-1.3-contributor-",
        "muse-spark-1x3-contributor",
        "muse-spark-1.2-contributor",
        "Muse-spark-1.3-contributor",
        "muse-spark-1.3-contributor\n",
        "opencode/muse-spark-1.3-contributor-free",
    ] {
        assert_eq!(muse_spark_plain(key), None, "{key:?}");
    }
}

#[test]
fn a_key_is_the_last_segment_with_its_version_in_the_catalog_s_spelling() {
    assert_eq!(curated_key("gpt-6-astra"), "gpt-6-astra");
    assert_eq!(curated_key("openrouter/openai/gpt-6.1-sol"), "gpt-6.1-sol");
    assert_eq!(curated_key("gpt-6-1-sol"), "gpt-6.1-sol");
    assert_eq!(curated_key("claude-opus-5-5"), "claude-opus-5.5");
    assert_eq!(curated_key("claude-haiku-5-5"), "claude-haiku-5.5");
    assert_eq!(
        curated_key("openrouter/anthropic/claude-fable-5.1"),
        "claude-fable-5.1"
    );
    assert_eq!(
        curated_key("opencode/muse-spark-1.3-contributor-free"),
        "muse-spark-1.3"
    );
    assert_eq!(curated_key("a/b/"), "");
    assert_eq!(curated_key("/x"), "x");
}

#[test]
fn a_work_tier_is_one_of_four_words_or_none() {
    assert_eq!(validate_work_tier(None), Ok(None));
    assert_eq!(validate_work_tier(Some(&json!(null))), Ok(None));
    for tier in WORK_TIERS {
        assert_eq!(
            validate_work_tier(Some(&json!(tier.as_str()))),
            Ok(Some(tier))
        );
    }
}

#[test]
fn any_other_work_tier_is_refused_with_the_one_sentence() {
    for refused in [
        json!("huge"),
        json!(""),
        json!("Critical"),
        json!(" critical"),
        json!("critical "),
        json!("constructor"),
        json!("__proto__"),
        json!("hasOwnProperty"),
        json!(7),
        json!(0),
        json!(true),
        json!(false),
        json!([]),
        json!(["critical"]),
        json!({}),
        json!({ "critical": true }),
    ] {
        let refusal = validate_work_tier(Some(&refused)).unwrap_err();
        assert_eq!(
            refusal.message, "Work tier must be critical, complex, standard or light",
            "{refused}"
        );
        assert_eq!(
            (refusal.code, refusal.status),
            ("work-tier", 400),
            "{refused}"
        );
    }
}

#[test]
fn the_tiers_read_as_the_page_shows_them() {
    let labels: Vec<_> = WORK_TIERS
        .into_iter()
        .map(|tier| work_tier_info(tier).label)
        .collect();
    assert_eq!(
        labels,
        [
            "Critical work",
            "Complex work",
            "Standard work",
            "Light work"
        ]
    );
    assert_eq!(
        serde_json::to_string(&work_tier_info(WorkTier::Light)).unwrap(),
        r#"{"label":"Light work","description":"Bounded fixes, lookups and routine tasks; verify the model is suitable."}"#
    );
}

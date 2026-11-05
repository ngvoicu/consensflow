//! What a chief's launch asks for, from its row and what its harness's record says
//! (CHF-05).

use cf_harness::contract::Used;
use cf_ledger::ParticipantView;

use super::ChiefRuns;

fn chief(model: Option<&str>, launched: Option<&str>) -> ParticipantView {
    ParticipantView {
        id: 2,
        project_id: 1,
        handle: "chief".into(),
        role: "chief".into(),
        agent: None,
        harness: Some("claude-code".into()),
        model: model.map(str::to_owned),
        effort: None,
        model_name: None,
        needs_edit: false,
        launched_model: launched.map(str::to_owned),
        gone_model: None,
        ran_on: None,
        designer: false,
        created_at: "2026-10-29T10:00:00.000Z".into(),
        left_at: None,
        tier: None,
        roles: Vec::new(),
        out_until: None,
        out_since: None,
        member_id: None,
        member: None,
        session: None,
    }
}

fn used(model: &str, effort: Option<&str>, by_harness: bool) -> Used {
    Used {
        model: model.to_owned(),
        effort: effort.map(str::to_owned),
        thinking: None,
        by_harness,
    }
}

fn runs(model: Option<&str>, effort: Option<&str>) -> ChiefRuns {
    ChiefRuns {
        model: model.map(str::to_owned),
        effort: effort.map(str::to_owned),
        thinking: None,
    }
}

#[test]
fn a_reopen_goes_on_with_the_model_and_effort_the_record_says_it_ran_on_last() {
    let chief = chief(Some("claude-opus-5-5"), Some("claude-opus-5-5"));
    assert_eq!(
        ChiefRuns::of(&chief, Some(used("claude-sonnet-5-5", Some("low"), false))),
        runs(Some("claude-sonnet-5-5"), Some("low"))
    );
}

#[test]
fn a_record_that_names_no_model_leaves_the_model_picked_for_the_chief_and_no_effort() {
    let chief = chief(Some("claude-opus-5-5"), Some("claude-sonnet-5-5"));
    assert_eq!(
        ChiefRuns::of(&chief, None),
        runs(Some("claude-opus-5-5"), None)
    );
}

#[test]
fn a_model_the_harness_moved_to_by_itself_is_never_taken_and_the_last_launchs_is_asked_again() {
    let moved = used("claude-sonnet-5-5", Some("high"), true);
    assert_eq!(
        ChiefRuns::of(
            &chief(Some("claude-opus-5-5"), Some("claude-haiku-5-5")),
            Some(moved.clone())
        ),
        runs(Some("claude-haiku-5-5"), None)
    );
    // A last launch that asked for none: the model picked for the chief.
    assert_eq!(
        ChiefRuns::of(&chief(Some("claude-opus-5-5"), None), Some(moved)),
        runs(Some("claude-opus-5-5"), None)
    );
}

#[test]
fn a_chief_from_before_with_no_model_and_a_silent_record_is_left_to_its_harnesss_default() {
    assert_eq!(
        ChiefRuns::of(&chief(None, None), None),
        ChiefRuns::default()
    );
}

#[test]
fn a_pi_chiefs_thinking_is_what_the_record_says_and_never_an_effort() {
    let pi = Used {
        model: "openai/gpt-6-luna".into(),
        effort: None,
        thinking: Some("low".into()),
        by_harness: false,
    };
    assert_eq!(
        ChiefRuns::of(&chief(Some("anthropic/claude-fable-5"), None), Some(pi)),
        ChiefRuns {
            model: Some("openai/gpt-6-luna".into()),
            effort: None,
            thinking: Some("low".into()),
        }
    );
}

/// The effort the human picked for a chief (harness-models: New project and Switch chief pick
/// a harness, a model and its effort) is what its launch asks for: Claude Code's and Codex's
/// as the effort, Pi's as its thinking, and none for Devin, whose model is the exact id it
/// lists for the effort.
#[test]
fn the_effort_picked_for_a_chief_is_its_launch_s_pi_s_its_thinking_and_devin_s_none() {
    let picked = |harness: &str, model: &str| {
        let mut chief = chief(Some(model), None);
        chief.harness = Some(harness.to_owned());
        chief.effort = Some("high".to_owned());
        ChiefRuns::of(&chief, None)
    };
    assert_eq!(
        picked("claude-code", "claude-opus-5-5"),
        runs(Some("claude-opus-5-5"), Some("high"))
    );
    assert_eq!(
        picked("codex", "gpt-6-astra"),
        runs(Some("gpt-6-astra"), Some("high"))
    );
    assert_eq!(
        picked("pi", "anthropic/claude-opus-5-5"),
        ChiefRuns {
            model: Some("anthropic/claude-opus-5-5".into()),
            effort: None,
            thinking: Some("high".into()),
        }
    );
    assert_eq!(
        picked("devin", "claude-opus-5-5-high"),
        runs(Some("claude-opus-5-5-high"), None)
    );
}

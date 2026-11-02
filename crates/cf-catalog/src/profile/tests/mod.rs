//! The profile's tests: the key a model is called by and the tier words
//! (`keys`), and the branches of `agentProfile`, in their order (`branches`).

use super::*;

mod branches;
mod keys;

fn catalog() -> Catalog {
    Catalog::bundled().unwrap()
}

fn preset(name: &str, kind: &str, model: &str, effort: Option<&str>) -> Preset {
    Preset {
        preset: name.to_owned(),
        id: name.to_owned(),
        name: name.to_owned(),
        label: name.to_owned(),
        description: name.to_owned(),
        kind: kind.to_owned(),
        model: model.to_owned(),
        effort: effort.map(str::to_owned),
        thinking: None,
        designer: false,
    }
}

/// The tier Add member's tier select starts on (T-1143): the presets' rules for a model a
/// preset runs on that harness, at the effort picked; none for a model no preset runs
/// there, at any effort, though `profile` calls such an agent light.
#[test]
fn the_preset_tier_is_the_rules_tier_of_a_model_the_presets_run_on_that_harness_and_none_else() {
    let catalog = catalog();
    assert_eq!(
        catalog.preset_tier("claude", "claude-opus-5-5", Some("max")),
        Some(WorkTier::Critical)
    );
    assert_eq!(
        catalog.preset_tier("claude", "claude-opus-5-5", Some("medium")),
        Some(WorkTier::Standard)
    );
    assert_eq!(
        catalog.preset_tier("pi", "openrouter/anthropic/claude-opus-5.5", Some("high")),
        Some(WorkTier::Complex),
        "Pi's effort is its thinking"
    );
    assert_eq!(
        catalog.preset_tier("claude", "claude-mythos-5", Some("max")),
        None
    );
    assert_eq!(
        catalog.preset_tier("muse", "muse-spark-1.3", Some("max")),
        None,
        "no preset runs on Muse"
    );
    assert_eq!(
        catalog.preset_tier("codex", "claude-opus-5-5", Some("max")),
        None,
        "a model the presets run, but on another harness"
    );
}

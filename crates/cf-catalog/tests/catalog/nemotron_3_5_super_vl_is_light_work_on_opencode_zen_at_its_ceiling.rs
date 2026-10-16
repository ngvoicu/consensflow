//! Nemotron 3.5 Super VL on OpenCode Zen (the owner's decision, 2026-10-16): a
//! row of the Rust catalog, added after Node went.
//!
//! `opencode/nemotron-3.5-super-vl-free` answers on the Mac and on zeewin. Zen's
//! models.dev entry for it publishes `reasoning_options` effort {none, minimal,
//! low, medium, high, xhigh}, where its entry for Nemotron 3 Ultra publishes none:
//! so this row names its ceiling, `xhigh`, where audhumla names nothing. Pi has no
//! Zen provider, so the row has no twin.

use super::*;
use cf_catalog::Roster;

#[test]
fn is_the_one_nemotron_3_5_of_the_catalog_with_its_label_its_route_and_its_effort() {
    let catalog = catalog();
    let nemotron: Vec<(Harness, &str)> = entries(&catalog)
        .filter(|(_, entry)| entry.model.contains("nemotron-3.5"))
        .map(|(harness, entry)| (harness, entry.name.as_str()))
        .collect();
    assert_eq!(nemotron, [(Harness::Opencode, "buri")]);
    let entry = found(&catalog, "buri");
    assert_eq!(
        (
            entry.harness,
            entry.entry.model.as_str(),
            entry.entry.effort.as_deref(),
            entry.entry.description.as_str(),
            entry.entry.detail.as_str(),
        ),
        (
            Harness::Opencode,
            "opencode/nemotron-3.5-super-vl-free",
            Some("xhigh"),
            "OpenCode Zen Nemotron 3.5 Super VL FREE XHIGH",
            "Coding and analysis across longer tasks.",
        )
    );
    let profile = &entry.entry.profile;
    assert_eq!(
        (
            profile.model_key.as_str(),
            profile.model_label.as_str(),
            profile.route_label.as_str(),
            profile.route_note.as_deref(),
            profile.work_tier,
        ),
        (
            "nemotron-3.5-super-vl-free",
            "Nemotron 3.5 Super VL (Zen free)",
            "OpenCode Zen",
            None,
            WorkTier::Light
        )
    );
    // The same model on the same road as Nemotron 3 Ultra's Zen row, which names
    // no effort because Zen publishes none for it.
    assert_eq!(found(&catalog, "audhumla").entry.effort, None);
}

#[test]
fn opens_an_opencode_window_on_its_model_and_zen_s_ceiling() {
    let catalog = catalog();
    let home = tempfile::tempdir().unwrap();
    let row = Roster::new(&catalog, home.path().join("agents.json"))
        .agent_row("buri")
        .unwrap()
        .unwrap();
    assert_eq!(
        (row.kind(), row.model(), row.effort()),
        (
            Some("opencode"),
            Some("opencode/nemotron-3.5-super-vl-free"),
            Some("xhigh")
        )
    );
    // xhigh is the top of what Zen lists; max is a level OpenCode takes and this
    // model does not have.
    assert!(efforts(Harness::Opencode).contains(&"xhigh"));
    assert!(efforts(Harness::Opencode).contains(&"max"));
}

#[test]
fn is_light_work_at_every_effort_as_every_free_opencode_model_is_and_the_tier_the_human_chose_still_comes_last(
) {
    let catalog = catalog();
    let mut tried: Vec<Option<&str>> = efforts(Harness::Opencode)
        .iter()
        .copied()
        .map(Some)
        .collect();
    tried.extend([None, Some(""), Some("bogus")]);
    for effort in tried {
        let profile = catalog.profile(&Settings {
            harness: Some("opencode"),
            model: Some("opencode/nemotron-3.5-super-vl-free"),
            effort,
            ..Settings::default()
        });
        assert_eq!(
            (profile.work_tier, profile.model_label.as_str()),
            (WorkTier::Light, "Nemotron 3.5 Super VL (Zen free)"),
            "Nemotron 3.5 Super VL {effort:?}"
        );
    }
    // The other free OpenCode rows: light work too.
    for name in ["ymir", "aegir", "audhumla", "gefjon"] {
        assert_eq!(
            found(&catalog, name).entry.profile.work_tier,
            WorkTier::Light,
            "{name}"
        );
    }
    let chosen = catalog.profile(&Settings {
        harness: Some("opencode"),
        model: Some("opencode/nemotron-3.5-super-vl-free"),
        effort: Some("xhigh"),
        work_tier: Some(WorkTier::Standard),
        ..Settings::default()
    });
    assert_eq!(chosen.work_tier, WorkTier::Standard);
}

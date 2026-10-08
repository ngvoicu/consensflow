//! The tests under `describe('Claude Haiku 5.5 is light work on Claude Code
//! alone')` of Node's catalog suite.
//!
//! 2026-10-08: Claude Code 2.1.294 answers claude-haiku-5-5 as itself at every level it lists.
//! Devin lists Claude only as Sonnet, and Pi and OpenCode (OpenRouter) carry Haiku 4.5 and a
//! `~anthropic/claude-haiku-latest` alias, no 5.5: no other harness has a row for it.
//!
//! The JS opens the window with `interactiveStart` and reads its `--model` and `--effort`. The
//! Rust launch is the Claude adapter's, held by `cf-harness`'s own tests and by
//! `npm run live:agent`; the second test here holds the row the launcher is given.

use super::*;
use cf_catalog::Roster;

#[test]
fn is_the_one_haiku_of_the_catalog_with_its_label_its_route_and_its_effort() {
    let catalog = catalog();
    let haiku: Vec<(Harness, &str)> = entries(&catalog)
        .filter(|(_, entry)| entry.model.contains("haiku"))
        .map(|(harness, entry)| (harness, entry.name.as_str()))
        .collect();
    assert_eq!(haiku, [(Harness::Claude, "huginn")]);
    let entry = found(&catalog, "huginn");
    assert_eq!(
        (
            entry.harness,
            entry.entry.model.as_str(),
            entry.entry.effort.as_deref(),
            entry.entry.description.as_str(),
            entry.entry.detail.as_str(),
        ),
        (
            Harness::Claude,
            "claude-haiku-5-5",
            Some("max"),
            "Claude Code Haiku 5.5 MAX",
            "Routine coding and second opinions.",
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
            "claude-haiku-5.5",
            "Claude Haiku 5.5",
            "Claude Code account",
            None,
            WorkTier::Light
        )
    );
    // Only on Claude Code is the id the catalog's: elsewhere it is called by what it says.
    let elsewhere = catalog.profile(&Settings {
        harness: Some("pi"),
        model: Some("claude-haiku-5-5"),
        ..Settings::default()
    });
    assert_eq!(elsewhere.model_label, "claude-haiku-5-5");
}

#[test]
fn opens_a_claude_code_window_on_its_model_and_effort() {
    let catalog = catalog();
    let home = tempfile::tempdir().unwrap();
    let row = Roster::new(&catalog, home.path().join("agents.json"))
        .agent_row("huginn")
        .unwrap()
        .unwrap();
    assert_eq!(
        (row.kind(), row.model(), row.effort()),
        (Some("claude-code"), Some("claude-haiku-5-5"), Some("max"))
    );
    let effort = row.effort().unwrap();
    assert!(
        efforts(Harness::Claude).contains(&effort),
        "{effort} is an effort Claude Code takes"
    );
}

#[test]
fn is_light_work_at_every_effort_and_the_tier_the_human_chose_still_comes_last() {
    let catalog = catalog();
    // A bogus or an absent effort too: the tier follows the model alone.
    let mut tried: Vec<Option<&str>> = efforts(Harness::Claude).iter().copied().map(Some).collect();
    tried.extend([None, Some(""), Some("bogus")]);
    for effort in tried {
        let profile = catalog.profile(&Settings {
            harness: Some("claude"),
            model: Some("claude-haiku-5-5"),
            effort,
            ..Settings::default()
        });
        assert_eq!(
            (profile.work_tier, profile.model_label.as_str()),
            (WorkTier::Light, "Claude Haiku 5.5"),
            "Haiku 5.5 {effort:?}"
        );
    }
    let chosen = catalog.profile(&Settings {
        harness: Some("claude"),
        model: Some("claude-haiku-5-5"),
        effort: Some("max"),
        work_tier: Some(WorkTier::Standard),
        ..Settings::default()
    });
    assert_eq!(chosen.work_tier, WorkTier::Standard);
}

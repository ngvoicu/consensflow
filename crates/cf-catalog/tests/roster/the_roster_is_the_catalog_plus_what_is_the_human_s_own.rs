//! The tests under `describe('the roster is the catalog plus what is the
//! human’s own')` of Node's roster suite. Each starts from a home of its own:
//! the JS ran them in order in one, the later ones on the file an earlier one
//! put there.

use super::*;
use cf_catalog::WORK_TIERS;

#[test]
fn uses_agents_json_inside_the_explicitly_configured_private_home() {
    let home = Home::new();
    assert_eq!(
        roster_path(&home.env()),
        Some(home.root.path().join("consensflow").join("agents.json"))
    );
}

#[test]
fn lists_every_catalog_agent_with_no_file_at_all_as_the_catalog_has_it() {
    let home = Home::new();
    let catalog = catalog();
    let roster = Roster::new(&catalog, home.path());
    let agents = roster.list().unwrap();
    assert_eq!(agents.len(), catalog.presets().len());
    let gefjon = agents
        .iter()
        .find(|agent| agent.name.as_deref() == Some("gefjon"))
        .unwrap();
    assert_eq!(
        (
            gefjon.harness.as_deref(),
            gefjon.model.as_deref(),
            gefjon.effort.as_deref(),
            gefjon.preset.as_deref(),
            gefjon.custom,
        ),
        (
            Some("opencode"),
            Some("opencode/muse-spark-1.3-contributor-free"),
            Some("xhigh"),
            Some("gefjon"),
            false,
        )
    );
    // `gefjon.edited` is `undefined`: no such field is written.
    assert!(serde_json::to_value(gefjon)
        .unwrap()
        .get("edited")
        .is_none());
    assert_eq!(
        gefjon.description,
        Some("OpenCode Zen Muse Spark 1.3 Contributor FREE XHIGH".into())
    );
    assert!(WORK_TIERS.contains(&gefjon.profile.work_tier));
    assert!(!home.path().exists(), "listing writes nothing");
    let row = roster.agent_row("gefjon").unwrap().unwrap();
    assert_eq!(
        (row.kind(), row.model(), row.effort()),
        (Some("opencode"), gefjon.model.as_deref(), Some("xhigh"))
    );
    assert_eq!(
        roster.agent_row("@gefjon").unwrap().unwrap().id(),
        Some("gefjon")
    );
}

#[test]
fn reads_v1_rows_as_agents_kind_to_harness_thinking_or_effort_to_effort() {
    let home = Home::new();
    home.seed_the_v1_roster();
    let agents = home.by_name(&catalog());
    assert_eq!(agents["zeus"].harness.as_deref(), Some("claude"));
    assert_eq!(agents["zeus"].effort.as_deref(), Some("max"));
    assert_eq!(agents["endymion"].harness.as_deref(), Some("pi"));
    // The fixture's copy says xhigh; a catalog agent reads as the catalog has it.
    assert_eq!(agents["endymion"].effort.as_deref(), Some("max"));
    assert_eq!(agents["mani"].harness.as_deref(), Some("opencode"));
}

#[test]
fn reads_the_image_agent_a_v1_file_saved_on_the_image_harness_as_the_catalog_has_it_codex_designing(
) {
    let home = Home::new();
    home.seed_the_v1_roster();
    let catalog = catalog();
    let pygmalion = &home.by_name(&catalog)["pygmalion"];
    assert_eq!(
        (
            pygmalion.harness.as_deref(),
            pygmalion.designer,
            pygmalion.model.as_deref(),
            pygmalion.unsupported,
        ),
        (Some("codex"), true, Some("codex-image"), false)
    );
    let row = Roster::new(&catalog, home.path())
        .agent_row("pygmalion")
        .unwrap()
        .unwrap();
    assert_eq!(row.kind(), Some("codex"));
    assert_eq!(row.get("designer"), Some(&serde_json::Value::Bool(true)));
}

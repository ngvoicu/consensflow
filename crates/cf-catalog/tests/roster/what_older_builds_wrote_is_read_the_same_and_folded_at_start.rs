//! The tests under `describe('what older builds wrote is read the same, and
//! folded at start')` in `tests/roster.test.mjs`.

use super::*;
use serde_json::json;

/// The file the first test writes and the second folds: copies of catalog
/// entries, stored display data, and an image agent of before.
fn older_file() -> String {
    json!({
        "schemaVersion": 1,
        "agents": [
            {
                "id": "gefjon",
                "name": "Gefjon",
                "kind": "opencode",
                "model": "opencode/muse-spark-1.3-contributor-free",
                "effort": "xhigh",
                "preset": "gefjon",
                "profile": { "workTier": "light" },
            },
            {
                "id": "apollo",
                "name": "Apollo",
                "kind": "claude-code",
                "model": "claude-opus-5",
                "effort": "low",
                "preset": "apollo",
            },
            {
                "id": "mine",
                "name": "Mine",
                "kind": "codex",
                "model": "gpt-6-astra",
                "effort": "low",
                "skillsPolicy": "default",
            },
            // The human's own image agent, from when `image` was a harness of its own.
            {
                "id": "my-draw",
                "name": "My-draw",
                "kind": "image",
                "model": "gpt-image-2",
                "description": "My drawings",
            },
        ],
    })
    .to_string()
}

#[test]
fn a_copy_of_a_catalog_entry_edited_or_not_reads_as_the_catalog_has_it_without_writing() {
    let home = Home::new();
    let original = older_file();
    home.write(&original);
    let catalog = catalog();
    let agents = home.by_name(&catalog);
    assert_eq!(
        (
            agents["gefjon"].custom,
            agents["apollo"].effort.as_deref(),
            agents["mine"].custom
        ),
        (false, Some("xhigh"), true)
    );
    let draw = &agents["my-draw"];
    assert_eq!(
        (
            draw.harness.as_deref(),
            draw.designer,
            draw.custom,
            draw.profile.model_label.as_str()
        ),
        (Some("codex"), true, true, "Codex Images"),
        "an image agent on the `image` harness reads as the Codex agent that designs it is now"
    );
    let row = Roster::new(&catalog, home.path())
        .agent_row("my-draw")
        .unwrap()
        .unwrap();
    assert_eq!(row.kind(), Some("codex"));
    assert_eq!(home.text(), original, "a read writes nothing");
}

#[test]
fn normalizing_keeps_only_the_human_s_own_agents_drops_stored_display_data_makes_an_image_agent_a_designing_codex_one_and_is_idempotent(
) {
    let home = Home::new();
    home.write(&older_file());
    let catalog = catalog();
    let roster = home.roster(&catalog);
    assert!(roster.normalize().unwrap());
    assert_eq!(
        home.raw()["agents"],
        json!([
            { "id": "mine", "name": "Mine", "kind": "codex", "model": "gpt-6-astra", "effort": "low" },
            { "id": "my-draw", "name": "My-draw", "kind": "codex", "model": "gpt-image-2", "description": "My drawings", "designer": true },
        ])
    );
    assert!(!roster.normalize().unwrap());
}

#[test]
fn normalizing_writes_an_image_agent_of_its_own_the_same_way_though_nothing_else_in_the_file_is_old(
) {
    let home = Home::new();
    let draw =
        json!({ "id": "my-draw", "name": "My-draw", "kind": "image", "model": "codex-image" });
    home.write(&format!(
        "{}\n",
        json!({ "schemaVersion": 1, "agents": [draw] })
    ));
    let catalog = catalog();
    let roster = home.roster(&catalog);
    assert!(roster.normalize().unwrap());
    assert_eq!(
        home.raw()["agents"],
        json!([{ "id": "my-draw", "name": "My-draw", "kind": "codex", "model": "codex-image", "designer": true }])
    );
    assert!(!roster.normalize().unwrap());
}

#[test]
fn normalizing_a_home_with_no_roster_writes_nothing() {
    let home = Home::new();
    let catalog = catalog();
    assert!(!home.roster(&catalog).normalize().unwrap());
    assert!(!home.path().exists());
}

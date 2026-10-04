//! The reading halves of two tests under `describe('agents defined by hand
//! are stored in full, v1-shaped')` in `tests/roster.test.mjs`. The others
//! define, edit and remove agents: the roster's writes. Both of these end
//! in a `removeAgent`, which here is the file written without the row.

use super::*;
use serde_json::json;

/// The reading half of 'defines no agent on Kimi, and reads one an older
/// build saved as a harness it does not run'; its writing half is the
/// refusal to add one, and removing it.
#[test]
fn reads_one_an_older_build_saved_as_a_harness_it_does_not_run() {
    let home = Home::new();
    let old_kimi = json!({
        "id": "old-kimi",
        "name": "Old-kimi",
        "kind": "kimi",
        "model": "moonshot-ai/kimi-k3",
    });
    home.write(&json!({ "schemaVersion": 1, "agents": [old_kimi] }).to_string());
    let agents = home.by_name(&catalog());
    assert!(agents["old-kimi"].unsupported);
}

/// The reading half of 'a custom row that took a catalog name on another
/// harness hides that entry'; its writing half is `removeAgent('zeus')`.
#[test]
fn a_custom_row_that_took_a_catalog_name_on_another_harness_hides_that_entry() {
    let home = Home::new();
    let catalog = catalog();
    let zeus = json!({
        "id": "zeus",
        "name": "Zeus",
        "kind": "opencode",
        "model": "opencode/muse-spark-1.3",
    });
    home.write(
        &serde_json::to_string_pretty(&json!({ "schemaVersion": 1, "agents": [zeus] })).unwrap(),
    );
    let agents = home.by_name(&catalog);
    let zeus = &agents["zeus"];
    assert_eq!(
        (zeus.harness.as_deref(), zeus.custom, zeus.preset.as_deref()),
        (Some("opencode"), true, None)
    );
    let listed = Roster::new(&catalog, home.path()).list().unwrap();
    let named_zeus = |agent: &&AgentView| agent.name.as_deref() == Some("zeus");
    assert_eq!(listed.iter().filter(named_zeus).count(), 1);
    // The row taken out of the file: the catalog entry is back.
    home.write(&json!({ "schemaVersion": 1, "agents": [] }).to_string());
    assert_eq!(
        home.by_name(&catalog)["zeus"].harness.as_deref(),
        Some("claude"),
        "the catalog entry is back"
    );
}

//! The reading half of the test under `describe("the human's own agent and a
//! catalog that takes its name later")` in `tests/roster.test.mjs`. Its
//! writing half is `normalizeRoster` keeping the row in the file.

use super::*;
use serde_json::json;

#[test]
fn keeps_the_agent_on_its_own_harness_too_and_hides_the_catalog_entry() {
    let home = Home::new();
    let catalog = catalog();
    // Saved by hand before a release added hapi, Devin's SWE-1.6, to the catalog.
    assert!(catalog.presets().iter().any(|preset| preset.id == "hapi"));
    let mine = json!({ "id": "hapi", "kind": "devin", "model": "swe-2", "effort": "high" });
    home.write(&format!(
        "{}\n",
        json!({ "schemaVersion": 1, "agents": [mine] })
    ));
    let listed = Roster::new(&catalog, home.path()).list().unwrap();
    let hapi: Vec<(Option<&str>, bool)> = listed
        .iter()
        .filter(|agent| agent.name.as_deref() == Some("hapi"))
        .map(|agent| (agent.model.as_deref(), agent.custom))
        .collect();
    assert_eq!(hapi, [(Some("swe-2"), true)], "one hapi: the human's");
}

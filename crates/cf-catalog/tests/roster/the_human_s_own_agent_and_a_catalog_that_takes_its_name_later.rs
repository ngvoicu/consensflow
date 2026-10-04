//! The test under `describe("the human's own agent and a catalog that takes
//! its name later")` in `tests/roster.test.mjs`.

use super::*;
use serde_json::json;

#[test]
fn keeps_the_agent_on_its_own_harness_too_and_hides_the_catalog_entry() {
    let home = Home::new();
    let catalog = catalog();
    let mine = json!({ "id": "hapi", "kind": "devin", "model": "swe-2", "effort": "high" });
    home.write(&format!(
        "{}\n",
        json!({ "schemaVersion": 1, "agents": [mine] })
    ));
    let roster = home.roster(&catalog);
    roster.normalize().unwrap();
    assert_eq!(home.raw()["agents"], json!([mine]), "the file keeps it");
    let hapi: Vec<(Option<String>, bool)> = roster
        .list()
        .unwrap()
        .into_iter()
        .filter(|agent| agent.name.as_deref() == Some("hapi"))
        .map(|agent| (agent.model, agent.custom))
        .collect();
    assert_eq!(
        hapi,
        [(Some("swe-2".to_owned()), true)],
        "one hapi: the human's"
    );
}

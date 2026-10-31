//! The tests under `describe('a catalog agent stays as the catalog has it')` of
//! Node's roster suite.

use super::*;
use serde_json::json;

#[test]
fn is_neither_edited_nor_removed_and_its_name_cannot_be_defined_again() {
    let home = Home::new();
    let catalog = catalog();
    let roster = home.roster(&catalog);
    let refused =
        |answer: Result<AgentView, cf_base::refusal::Refusal>| answer.unwrap_err().message;
    assert!(
        refused(roster.edit("clio", &body(json!({ "effort": "low" })), &mut Now))
            .contains("catalog agent and stays")
    );
    assert!(
        refused(roster.edit("clio", &body(json!({ "description": "x" })), &mut Now))
            .contains("catalog agent and stays")
    );
    assert!(roster
        .remove("clio")
        .unwrap_err()
        .message
        .contains("not yours to remove"));
    assert!(refused(roster.add(
        &body(json!({ "name": "clio", "harness": "codex", "model": "m" })),
        &mut Now
    ))
    .contains("catalog agent: pick another name"));
    assert!(!home.path().exists(), "nothing was written");
    let clio = &home.by_name(&catalog)["clio"];
    assert_eq!(
        (clio.effort.as_deref(), clio.custom),
        (Some("xhigh"), false)
    );
}

#[test]
fn names_an_unknown_agent_on_edit_and_remove() {
    let home = Home::new();
    let catalog = catalog();
    let roster = home.roster(&catalog);
    assert!(roster
        .edit("nobody", &body(json!({ "model": "m" })), &mut Now)
        .unwrap_err()
        .message
        .contains("nobody"));
    assert!(roster
        .remove("nobody")
        .unwrap_err()
        .message
        .contains("nobody"));
}

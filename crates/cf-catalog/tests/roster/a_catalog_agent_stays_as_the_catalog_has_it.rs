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
        refused(roster.edit("gefjon", &body(json!({ "effort": "low" })), &mut Now))
            .contains("catalog agent and stays")
    );
    assert!(
        refused(roster.edit("gefjon", &body(json!({ "description": "x" })), &mut Now))
            .contains("catalog agent and stays")
    );
    assert!(roster
        .remove("gefjon")
        .unwrap_err()
        .message
        .contains("not yours to remove"));
    assert!(refused(roster.add(
        &body(json!({ "name": "gefjon", "harness": "codex", "model": "m" })),
        &mut Now
    ))
    .contains("catalog agent: pick another name"));
    assert!(!home.path().exists(), "nothing was written");
    let gefjon = &home.by_name(&catalog)["gefjon"];
    assert_eq!(
        (gefjon.effort.as_deref(), gefjon.custom),
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

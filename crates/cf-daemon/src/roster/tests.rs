//! The saved agents over a real agents file and the real catalog: what a
//! launch reads of one, what the agents' API reads, and that the members'
//! tiers follow the agents at a start without a broken file stopping it.

use serde_json::json;

use super::*;
use crate::testing::scene;

fn agents_in(home: &std::path::Path, document: &serde_json::Value) -> Agents {
    let file = home.join("agents.json");
    std::fs::write(&file, document.to_string()).unwrap();
    Agents::new(Catalog::bundled().unwrap(), file)
}

fn mine() -> serde_json::Value {
    json!({
        "schemaVersion": 1,
        "agents": [
            { "id": "mybuilder", "kind": "claude-code", "model": "fake", "effort": "high", "workTier": "complex" },
            { "id": "mypi", "kind": "pi", "model": "fake", "thinking": "low" },
            { "id": "mydesigner", "kind": "codex", "model": "fake", "designer": true },
        ]
    })
}

#[test]
fn a_launch_reads_a_saved_agent_s_model_effort_and_whether_it_designs() {
    let home = tempfile::tempdir().unwrap();
    let agents = agents_in(home.path(), &mine());
    let saved = |name: &str| cf_engine::seams::Roster::agent(&agents, name).unwrap();
    assert_eq!(
        saved("mybuilder"),
        Some(SavedAgent {
            model: Some("fake".to_owned()),
            effort: Some("high".to_owned()),
            thinking: None,
            designer: false,
        })
    );
    // Pi's effort is its `thinking`.
    let pi = saved("mypi").unwrap();
    assert_eq!((pi.effort, pi.thinking.as_deref()), (None, Some("low")));
    assert!(saved("mydesigner").unwrap().designer);
    assert_eq!(
        saved("nobody"),
        None,
        "an agent the human deleted has no row"
    );
}

#[test]
fn the_agents_api_reads_the_same_rows() {
    let home = tempfile::tempdir().unwrap();
    let agents = agents_in(home.path(), &mine());
    let row = agents.row("mybuilder").unwrap().unwrap();
    assert_eq!((row.model(), row.effort()), (Some("fake"), Some("high")));
    assert!(agents.row("nobody").unwrap().is_none());
}

#[test]
fn a_catalog_agent_is_there_without_the_file_saying_so() {
    let home = tempfile::tempdir().unwrap();
    // No file at all: the roster is the catalog's.
    let agents = Agents::new(Catalog::bundled().unwrap(), home.path().join("agents.json"));
    let listed = agents.roster().list().unwrap();
    let name = listed[0].name.clone().unwrap();
    assert!(agents.row(&name).unwrap().is_some(), "{name}");
    assert!(
        cf_engine::seams::Roster::agent(&agents, &name)
            .unwrap()
            .is_some(),
        "{name}"
    );
}

#[test]
fn a_file_that_cannot_be_read_is_a_refusal_in_the_words_of_the_roster_and_is_left_alone() {
    let home = tempfile::tempdir().unwrap();
    let broken = "{\"schemaVersion\": 1, \"agents\": [{\"id\": \"mine\", \"kind\": \"codex\"},]}\n";
    let file = home.path().join("agents.json");
    std::fs::write(&file, broken).unwrap();
    let agents = Agents::new(Catalog::bundled().unwrap(), file.clone());
    let refusal = agents.row("mine").unwrap_err();
    assert!(
        refusal.message.starts_with("Your agents file"),
        "{}",
        refusal.message
    );
    assert!(
        refusal.message.contains("is not valid JSON"),
        "{}",
        refusal.message
    );
    assert!(cf_engine::seams::Roster::agent(&agents, "mine").is_err());

    let scene = scene();
    let failed = agents
        .normalize_and_follow(&mut scene.context.ledger.borrow_mut())
        .unwrap_err();
    assert!(failed.starts_with("Your agents file"), "{failed}");
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        broken,
        "left as the human wrote it"
    );
}

#[test]
fn at_a_start_a_member_s_tier_is_read_again_from_its_agent() {
    let scene = scene();
    let home = tempfile::tempdir().unwrap();
    // zeus joined as a standard worker; its agent is a complex one now.
    let agents = agents_in(
        home.path(),
        &json!({
            "schemaVersion": 1,
            "agents": [{ "id": "zeus", "kind": "claude-code", "model": "fake", "workTier": "complex" }]
        }),
    );
    let changed = agents
        .normalize_and_follow(&mut scene.context.ledger.borrow_mut())
        .unwrap();
    assert_eq!(
        changed,
        [TierChange {
            project: scene.project.id,
            handle: "zeus".to_owned(),
            from: Some("standard".to_owned()),
            to: "complex".to_owned(),
        }]
    );
    // Nothing moved the second time.
    assert!(agents
        .follow(&mut scene.context.ledger.borrow_mut())
        .unwrap()
        .is_empty());
}

#[test]
fn a_member_whose_agent_is_gone_keeps_the_tier_it_has() {
    let scene = scene();
    let home = tempfile::tempdir().unwrap();
    // `ghost` is no catalog agent, and the human's file has none of its own.
    scene
        .context
        .ledger
        .borrow_mut()
        .add_member(
            scene.project.id,
            &cf_ledger::NewMember {
                agent: "ghost".to_owned(),
                harness: "claude-code".to_owned(),
                designer: false,
                roles: vec!["worker".to_owned()],
                tier: "light".to_owned(),
            },
        )
        .unwrap();
    let agents = agents_in(home.path(), &json!({ "schemaVersion": 1, "agents": [] }));
    let changed = agents
        .follow(&mut scene.context.ledger.borrow_mut())
        .unwrap();
    assert!(
        changed.iter().all(|change| change.handle != "ghost"),
        "{changed:?}"
    );
    let member = scene
        .context
        .ledger
        .borrow()
        .project(scene.project.id)
        .unwrap()
        .unwrap();
    let ghost = member
        .participants
        .iter()
        .find(|p| p.handle == "ghost")
        .unwrap();
    assert_eq!(ghost.tier.as_deref(), Some("light"));
}

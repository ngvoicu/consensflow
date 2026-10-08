//! The tests under `describe('agents defined by hand are stored in full,
//! v1-shaped')` of Node's roster suite.

use super::*;
use serde_json::json;

/// What the first test leaves: `mine`, added by hand.
fn with_mine(home: &Home, catalog: &Catalog) {
    home.roster(catalog)
        .add(
            &body(json!({ "name": "mine", "harness": "claude", "model": "claude-opus-5" })),
            &mut Now,
        )
        .unwrap();
}

#[test]
fn starts_with_the_catalog_only_and_creates_the_v1_file_shape_on_first_add() {
    let home = Home::new();
    let catalog = catalog();
    let roster = home.roster(&catalog);
    assert!(roster.list().unwrap().iter().all(|agent| !agent.custom));
    with_mine(&home, &catalog);
    let file = home.raw();
    assert_eq!(file["schemaVersion"], 1);
    let first = &file["agents"][0];
    assert_eq!(
        (
            first["id"].as_str(),
            first["name"].as_str(),
            first["kind"].as_str()
        ),
        (Some("mine"), Some("Mine"), Some("claude-code"))
    );
    assert!(first["createdAt"].is_string());
    assert!(
        first.get("profile").is_none(),
        "no display data in the file"
    );
    let mine = &home.by_name(&catalog)["mine"];
    assert_eq!(
        (mine.custom, mine.preset.as_deref(), mine.harness.as_deref()),
        (true, None, Some("claude"))
    );
    assert_eq!(roster.list().unwrap().len(), catalog.presets().len() + 1);
}

#[test]
fn validates_adds_bad_names_unknown_harnesses_empty_models_duplicates() {
    let home = Home::new();
    let catalog = catalog();
    with_mine(&home, &catalog);
    let roster = home.roster(&catalog);
    let add = |input: Value| roster.add(&body(input), &mut Now);
    assert!(add(json!({ "name": "Bad Name", "harness": "claude", "model": "m" })).is_err());
    assert!(add(json!({ "name": "ok", "harness": "not-a-cli", "model": "m" })).is_err());
    assert!(add(json!({ "name": "ok", "harness": "claude", "model": "" })).is_err());
    assert!(add(json!({ "name": "mine", "harness": "codex", "model": "m" })).is_err());
    // A pi agent edit lands in `thinking`, the key the pi runner reads.
    add(json!({ "name": "my-pi", "harness": "pi", "model": "openrouter/moonshotai/kimi-k3" }))
        .unwrap();
    roster
        .edit("my-pi", &body(json!({ "effort": "high" })), &mut Now)
        .unwrap();
    let pi = home.raw()["agents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == "my-pi")
        .unwrap()
        .clone();
    assert_eq!(
        (pi["thinking"].as_str(), pi.get("effort")),
        (Some("high"), None)
    );
    assert_eq!(
        home.by_name(&catalog)["my-pi"].effort.as_deref(),
        Some("high")
    );
    // An image agent is a Codex agent with the designer flag, and has no effort to edit, plainly.
    let said = |input: Value| add(input).unwrap_err().message;
    assert!(
        said(json!({ "name": "my-image", "harness": "image", "model": "codex-image" }))
            .contains(r#"unknown harness "image""#)
    );
    assert!(
        said(json!({ "name": "my-image", "harness": "pi", "designer": true, "model": "x" }))
            .contains("an image agent is a Codex agent")
    );
    assert!(said(
        json!({ "name": "my-image", "harness": "codex", "designer": "yes", "model": "x" })
    )
    .contains("an image agent (designer true) or not"));
    let image = add(
        json!({ "name": "my-image", "harness": "codex", "designer": true, "model": "codex-image" }),
    )
    .unwrap();
    assert_eq!(
        (
            image.harness.as_deref(),
            image.designer,
            image.profile.model_label.as_str()
        ),
        (Some("codex"), true, "Codex Images")
    );
    let stored = home.raw()["agents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == "my-image")
        .unwrap()
        .clone();
    let mut keys: Vec<&str> = stored
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "createdAt",
            "designer",
            "id",
            "kind",
            "model",
            "name",
            "updatedAt"
        ]
    );
    assert!(roster
        .edit("my-image", &body(json!({ "effort": "high" })), &mut Now)
        .unwrap_err()
        .message
        .contains("no effort level"));
    roster
        .edit(
            "my-image",
            &body(json!({ "description": "still editable" })),
            &mut Now,
        )
        .unwrap();
    // No other Codex agent designs: the catalog's image agent and this one
    // are the only designers. (The JS asked it of `freya-2`, which the next
    // test adds: there it held of nothing.)
    let designers: Vec<_> = roster
        .list()
        .unwrap()
        .into_iter()
        .filter(|agent| agent.harness.as_deref() == Some("codex") && agent.designer)
        .filter_map(|agent| agent.name)
        .collect();
    assert_eq!(designers, ["pygmalion", "my-image"]);
}

#[test]
fn edits_and_removes_a_custom_agent_in_place() {
    let home = Home::new();
    let catalog = catalog();
    with_mine(&home, &catalog);
    let roster = home.roster(&catalog);
    roster
        .add(&body(json!({ "name": "freya-2", "harness": "codex", "model": "gpt-5.6-terra", "effort": "xhigh" })), &mut Now)
        .unwrap();
    // An edit that would leave it without a model is refused, and saves nothing.
    let before = home.text();
    for model in [json!(""), json!(42)] {
        let refused = roster
            .edit("freya-2", &body(json!({ "model": model })), &mut Now)
            .unwrap_err();
        assert!(refused.message.contains("an agent needs a model"));
    }
    assert_eq!(home.text(), before);
    roster
        .edit(
            "freya-2",
            &body(json!({ "model": "gpt-6-astra", "effort": "low" })),
            &mut Now,
        )
        .unwrap();
    let stored = home.raw()["agents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == "freya-2")
        .unwrap()
        .clone();
    assert_eq!(
        (
            stored["model"].as_str(),
            stored["effort"].as_str(),
            stored["kind"].as_str()
        ),
        (Some("gpt-6-astra"), Some("low"), Some("codex"))
    );
    roster.remove("freya-2").unwrap();
    assert!(!home.raw()["agents"]
        .as_array()
        .unwrap()
        .iter()
        .any(|row| row["id"] == "freya-2"));
    assert!(!home.by_name(&catalog).contains_key("freya-2"));
}

#[test]
fn defines_no_agent_on_kimi_and_reads_one_an_older_build_saved_as_a_harness_it_does_not_run() {
    let home = Home::new();
    let catalog = catalog();
    with_mine(&home, &catalog);
    let roster = home.roster(&catalog);
    let refused = roster
        .add(
            &body(json!({ "name": "my-kimi", "harness": "kimi", "model": "moonshot-ai/kimi-k3" })),
            &mut Now,
        )
        .unwrap_err();
    assert!(refused.message.contains(r#"unknown harness "kimi""#));
    let mut file = home.raw();
    file["agents"].as_array_mut().unwrap().push(json!({ "id": "old-kimi", "name": "Old-kimi", "kind": "kimi", "model": "moonshot-ai/kimi-k3" }));
    home.write(&file.to_string());
    assert!(home.by_name(&catalog)["old-kimi"].unsupported);
    roster.remove("old-kimi").unwrap();
}

#[test]
fn a_custom_row_that_took_a_catalog_name_on_another_harness_hides_that_entry() {
    let home = Home::new();
    let catalog = catalog();
    with_mine(&home, &catalog);
    let mut file = home.raw();
    file["agents"].as_array_mut().unwrap().push(json!({ "id": "zeus", "name": "Zeus", "kind": "opencode", "model": "opencode/muse-spark-1.3" }));
    home.write(&serde_json::to_string_pretty(&file).unwrap());
    let zeus = home.by_name(&catalog)["zeus"].clone();
    assert_eq!(
        (zeus.harness.as_deref(), zeus.custom, zeus.preset.as_deref()),
        (Some("opencode"), true, None)
    );
    let listed = home.roster(&catalog).list().unwrap();
    assert_eq!(
        listed
            .iter()
            .filter(|agent| agent.name.as_deref() == Some("zeus"))
            .count(),
        1
    );
    home.roster(&catalog).remove("zeus").unwrap();
    assert_eq!(
        home.by_name(&catalog)["zeus"].harness.as_deref(),
        Some("claude"),
        "the catalog entry is back"
    );
}

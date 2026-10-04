use super::*;
use crate::roster::testing::{body, file_with, sentence, Counted};
use serde_json::json;
use std::fs;

fn catalog() -> Catalog {
    Catalog::bundled().unwrap()
}

const FILE: &[u8] = br#"{"schemaVersion":1,"agents":[{"id":"nova","name":"Nova","kind":"codex","createdAt":"2026-09-01T10:00:00.000Z","updatedAt":"2026-09-01T10:00:00.000Z","model":"gpt-6-astra","effort":"high","thinking":"stale","skills":["a"]},{"id":"pip","kind":"pi","model":"m","thinking":"low"},{"id":"nova","kind":"codex","model":"second"},{"id":"painter","kind":"image","model":"gpt-image-2"},{"id":"kim","kind":"kimi","model":"k"},{"id":"thoth","name":"Thoth","kind":"devin","model":"claude-fable-5-1","preset":"thoth"}]}"#;

/// What editing `name` by `patch` answers, the file after, and how often the clock was read.
fn edited(name: &str, patch: serde_json::Value) -> (Result<AgentView, Refusal>, String, usize) {
    let (_home, path) = file_with(FILE);
    let mut clock = Counted::new();
    let answer = catalog().edit(&path, name, &body(patch), &mut clock);
    (answer, fs::read_to_string(&path).unwrap(), clock.readings)
}

fn rows_of(file: &str) -> Vec<serde_json::Value> {
    serde_json::from_str::<serde_json::Value>(file).unwrap()["agents"]
        .as_array()
        .unwrap()
        .clone()
}

#[test]
fn an_edit_changes_the_first_row_of_the_name_in_place_and_stamps_it_once() {
    let (answer, file, readings) = edited(
        "nova",
        json!({ "model": "gpt-x", "effort": "max", "description": null }),
    );
    assert_eq!(readings, 1);
    let rows = rows_of(&file);
    // Key order kept, the stale field gone, the other effort key gone, a null description kept.
    assert_eq!(
        cf_base::js::stringify(&rows[0]),
        r#"{"id":"nova","name":"Nova","kind":"codex","createdAt":"2026-09-01T10:00:00.000Z","updatedAt":"2026-10-04T12:00:00.000Z","model":"gpt-x","effort":"max","description":null}"#
    );
    assert_eq!(
        rows[2]["model"], "second",
        "the second row of the name is not touched"
    );
    let view = answer.unwrap();
    assert!(view.custom);
    assert_eq!(view.model.as_deref(), Some("gpt-x"));
}

#[test]
fn a_null_or_empty_effort_takes_the_effort_off_and_a_null_tier_the_tier() {
    let (_, file, _) = edited("pip", json!({ "effort": "" }));
    assert!(!rows_of(&file)[1]
        .as_object()
        .unwrap()
        .contains_key("thinking"));
    let (_, file, _) = edited("pip", json!({ "effort": null, "workTier": "light" }));
    assert_eq!(rows_of(&file)[1]["workTier"], "light");
    let (_, file, _) = edited("pip", json!({ "workTier": null }));
    assert!(!rows_of(&file)[1]
        .as_object()
        .unwrap()
        .contains_key("workTier"));
}

#[test]
fn each_refusal_is_said_in_nodes_order_and_reads_no_time() {
    let cases = [
        ("thoth", json!({ "model": "x" }), "thoth is a catalog agent and stays as the catalog has it: define your own with the settings you want"),
        ("zeus", json!({}), "zeus is a catalog agent and stays as the catalog has it: define your own with the settings you want"),
        ("nobody", json!({ "model": "" }), "no agent named nobody"),
        ("nova", json!({ "model": "" }), "an agent needs a model (any identifier its harness accepts)"),
        ("nova", json!({ "workTier": "huge" }), "Work tier must be critical, complex, standard or light"),
        ("painter", json!({ "effort": "high" }), "painter is an image agent: it has no effort level \u{2014} only its model and description can be edited"),
        ("kim", json!({ "effort": "high" }), "kim is a kimi agent, which this build does not run; only its model and description can be edited here"),
        ("nova", json!({ "effort": { "level": "max" } }), "an agent's effort is the name of a level, as text"),
        ("nova", json!({ "model": "", "effort": 7 }), "an agent needs a model (any identifier its harness accepts)"),
    ];
    for (name, patch, said) in cases {
        let (answer, after, readings) = edited(name, patch.clone());
        assert_eq!(answer.unwrap_err().message, said, "{name} {patch}");
        assert_eq!(after.as_bytes(), FILE, "{name} {patch}: the file as it was");
        assert_eq!(readings, 0, "{name} {patch}");
    }
}

#[test]
fn a_tier_is_refused_before_the_file_is_read_and_an_unknown_name_after() {
    let (_home, path) = file_with(b"{ broken");
    let catalog = catalog();
    let mut clock = Counted::new();
    let tier = catalog.edit(&path, "nova", &body(json!({ "workTier": 7 })), &mut clock);
    assert_eq!(
        tier.unwrap_err().message,
        "Work tier must be critical, complex, standard or light"
    );
    let name = catalog.edit(&path, "nobody", &body(json!({})), &mut clock);
    assert_eq!(
        name.unwrap_err().message,
        sentence(&path, "is not valid JSON")
    );
}

#[test]
fn an_image_agent_of_before_is_edited_as_the_designing_codex_agent_it_reads_as() {
    let (answer, file, _) = edited("painter", json!({ "model": "gpt-image-3" }));
    assert!(answer.unwrap().designer);
    assert_eq!(rows_of(&file)[3]["kind"], "codex");
    assert_eq!(rows_of(&file)[3]["designer"], true);
}

#[test]
fn a_removal_takes_the_first_row_of_the_name_alone() {
    let (_home, path) = file_with(FILE);
    let catalog = catalog();
    catalog.remove(&path, "nova").unwrap();
    let rows = rows_of(&fs::read_to_string(&path).unwrap());
    let ids: Vec<_> = rows.iter().map(|row| row["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["pip", "nova", "painter", "kim", "thoth"]);
    assert_eq!(rows[1]["model"], "second");
    assert_eq!(
        catalog.remove(&path, "thoth").unwrap_err().message,
        "thoth is a catalog agent: it is not yours to remove"
    );
    assert_eq!(
        catalog.remove(&path, "nobody").unwrap_err().message,
        "no agent named nobody"
    );
}

#[test]
fn a_custom_row_under_a_catalog_name_is_the_humans_to_edit_and_remove() {
    let (_home, path) = file_with(br#"{"agents":[{"id":"thoth","kind":"codex","model":"gpt-x"}]}"#);
    let catalog = catalog();
    let mut clock = Counted::new();
    let view = catalog
        .edit(
            &path,
            "thoth",
            &body(json!({ "model": "gpt-y" })),
            &mut clock,
        )
        .unwrap();
    assert_eq!(view.model.as_deref(), Some("gpt-y"));
    catalog.remove(&path, "thoth").unwrap();
    assert_eq!(
        rows_of(&fs::read_to_string(&path).unwrap()),
        Vec::<serde_json::Value>::new()
    );
}

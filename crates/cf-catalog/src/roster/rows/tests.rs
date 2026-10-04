use std::collections::BTreeMap;

use super::*;
use crate::roster::document::load_document;
use crate::roster::testing::file_with;

fn preset(name: &str, kind: &str, model: &str) -> Preset {
    Preset {
        preset: name.to_owned(),
        id: name.to_owned(),
        name: name.to_uppercase(),
        label: format!("{name} headline"),
        description: format!("{name} paragraph"),
        kind: kind.to_owned(),
        model: model.to_owned(),
        effort: None,
        thinking: None,
        designer: false,
    }
}

fn catalog_of(presets: Vec<Preset>) -> Catalog {
    Catalog::new(presets, BTreeMap::new())
}

fn row(text: &str) -> AgentRow {
    AgentRow::from_value(serde_json::from_str(text).unwrap()).unwrap()
}

/// The document of a file whose agents are these rows' text.
fn document_of(rows: &str) -> Document {
    let (_home, path) = file_with(format!(r#"{{"schemaVersion":1,"agents":[{rows}]}}"#).as_bytes());
    load_document(&path).unwrap()
}

fn written(row: &AgentRow) -> String {
    serde_json::to_string(row).unwrap()
}

fn ids(rows: &[AgentRow]) -> Vec<&str> {
    rows.iter().filter_map(AgentRow::id).collect()
}

fn model_of(entry: Option<&Preset>) -> Option<&str> {
    entry.map(|entry| entry.model.as_str())
}

#[test]
fn a_row_is_a_copy_of_an_entry_only_when_its_preset_names_it_and_its_id_is_the_entrys() {
    let catalog = Catalog::bundled().unwrap();
    let copy = catalog.entry_of(&row(r#"{"id":"thoth","preset":"thoth"}"#));
    assert_eq!(copy.map(|entry| entry.id.as_str()), Some("thoth"));
    for own in [
        // Another id under the entry's provenance: the human's own agent.
        r#"{"id":"mine","preset":"thoth"}"#,
        // The entry's id and no provenance: one that took the name later.
        r#"{"id":"thoth"}"#,
        r#"{"id":"thoth","preset":null}"#,
        r#"{"id":"thoth","preset":"nobody"}"#,
        r#"{"id":"thoth","preset":""}"#,
        r#"{"id":"Thoth","preset":"thoth"}"#,
        r#"{"preset":"thoth"}"#,
        r#"{}"#,
    ] {
        assert!(catalog.entry_of(&row(own)).is_none(), "{own}");
    }
}

#[test]
fn the_entry_is_found_by_its_preset_and_the_id_is_then_compared_with_its_own() {
    let mut first = preset("a", "codex", "m1");
    first.id = "x".to_owned();
    let mut second = preset("b", "codex", "m2");
    second.id = "a".to_owned();
    let catalog = catalog_of(vec![first, second]);
    assert_eq!(
        model_of(catalog.entry_of(&row(r#"{"id":"x","preset":"a"}"#))),
        Some("m1")
    );
    assert_eq!(
        model_of(catalog.entry_of(&row(r#"{"id":"a","preset":"b"}"#))),
        Some("m2")
    );
    // The entry a preset names is the first's, whose id is `x`: an id another
    // entry has is not enough.
    assert!(catalog
        .entry_of(&row(r#"{"id":"a","preset":"a"}"#))
        .is_none());
}

#[test]
fn of_two_presets_with_one_name_the_later_is_the_entry_as_a_map_keeps_the_last_of_a_key() {
    let mut earlier = preset("twin", "codex", "m1");
    earlier.id = "first".to_owned();
    let mut later = preset("twin", "pi", "m2");
    later.id = "second".to_owned();
    let catalog = catalog_of(vec![earlier, later]);
    assert_eq!(
        model_of(catalog.entry_of(&row(r#"{"id":"second","preset":"twin"}"#))),
        Some("m2")
    );
    assert!(catalog
        .entry_of(&row(r#"{"id":"first","preset":"twin"}"#))
        .is_none());
}

#[test]
fn a_catalog_row_has_its_keys_in_the_order_the_roster_builds_them() {
    let mut nova = preset("nova", "codex", "gpt-6-astra");
    nova.effort = Some("high".to_owned());
    assert_eq!(
        written(&catalog_row(&nova)),
        concat!(
            r#"{"id":"nova","name":"NOVA","kind":"codex","model":"gpt-6-astra","effort":"high","#,
            r#""description":"nova headline","preset":"nova"}"#
        )
    );
}

#[test]
fn a_catalog_row_calls_itself_by_the_label_and_not_by_the_cards_paragraph() {
    let nova = preset("nova", "codex", "m");
    let row = catalog_row(&nova);
    assert_eq!(row.get("description"), Some(&Value::from("nova headline")));
    assert!(!written(&row).contains("paragraph"));
}

#[test]
fn an_image_preset_says_designer_after_its_kind() {
    let mut painter = preset("painter", "codex", "codex-image");
    painter.designer = true;
    assert_eq!(
        written(&catalog_row(&painter)),
        concat!(
            r#"{"id":"painter","name":"PAINTER","kind":"codex","designer":true,"model":"codex-image","#,
            r#""description":"painter headline","preset":"painter"}"#
        )
    );
}

#[test]
fn a_pi_presets_effort_is_kept_under_thinking_and_every_other_kinds_under_effort() {
    for (kind, key) in [
        ("pi", "thinking"),
        ("claude-code", "effort"),
        ("codex", "effort"),
        ("opencode", "effort"),
        ("devin", "effort"),
    ] {
        let mut with_effort = preset("a", kind, "m");
        with_effort.effort = Some("max".to_owned());
        let mut with_thinking = preset("b", kind, "m");
        with_thinking.thinking = Some("max".to_owned());
        for entry in [with_effort, with_thinking] {
            let row = catalog_row(&entry);
            assert_eq!(row.get(key), Some(&Value::from("max")), "{kind} {key}");
            let other = if key == "effort" {
                "thinking"
            } else {
                "effort"
            };
            assert_eq!(row.get(other), None, "{kind} has no {other}");
        }
    }
}

#[test]
fn a_presets_effort_is_read_before_its_thinking_and_an_empty_one_hides_it() {
    let mut both = preset("both", "pi", "m");
    both.effort = Some("low".to_owned());
    both.thinking = Some("max".to_owned());
    assert_eq!(
        catalog_row(&both).get("thinking"),
        Some(&Value::from("low")),
        "`effort ?? thinking`"
    );
    let mut empty = preset("empty", "pi", "m");
    empty.effort = Some(String::new());
    empty.thinking = Some("max".to_owned());
    let row = catalog_row(&empty);
    assert_eq!(
        row.get("thinking"),
        None,
        "`'' ?? 'max'` is `''`, which is falsy"
    );
    assert_eq!(row.get("effort"), None);
    let none = catalog_row(&preset("none", "codex", "m"));
    assert_eq!((none.get("effort"), none.get("thinking")), (None, None));
}

#[test]
fn with_no_stored_rows_the_roster_is_the_catalog_in_the_presets_order() {
    let catalog = Catalog::bundled().unwrap();
    let rows = catalog.rows(&document_of(""));
    let listed: Vec<&str> = catalog
        .presets()
        .iter()
        .map(|preset| preset.id.as_str())
        .collect();
    assert_eq!(ids(&rows), listed);
    assert_eq!(rows.len(), 119);
    assert!(rows.iter().all(|row| row.get("custom").is_none()));
}

#[test]
fn a_stored_copy_of_a_catalog_entry_is_ignored_edited_or_not() {
    let catalog = Catalog::bundled().unwrap();
    let document = document_of(
        r#"{"id":"gefjon","name":"Edited","kind":"codex","model":"gpt-x","effort":"low","preset":"gefjon"},
           {"id":"zeus","kind":"claude-code","model":"claude-opus-5","preset":"zeus"}"#,
    );
    let rows = catalog.rows(&document);
    assert_eq!(rows.len(), 119);
    let gefjon: Vec<&AgentRow> = rows
        .iter()
        .filter(|row| row.id() == Some("gefjon"))
        .collect();
    assert_eq!(gefjon.len(), 1);
    assert_eq!(
        gefjon[0].model(),
        Some("opencode/muse-spark-1.3-contributor-free")
    );
    assert_eq!(gefjon[0].get("custom"), None);
}

#[test]
fn a_custom_row_that_took_a_catalog_name_hides_that_entry_and_comes_after_the_catalog() {
    let catalog = Catalog::bundled().unwrap();
    let rows = catalog.rows(&document_of(
        r#"{"id":"zeus","name":"Zeus","kind":"opencode","model":"opencode/muse-spark-1.3"}"#,
    ));
    assert_eq!(rows.len(), 119, "one entry out, one row of the human's in");
    let zeus: Vec<&AgentRow> = rows.iter().filter(|row| row.id() == Some("zeus")).collect();
    assert_eq!(zeus.len(), 1);
    assert_eq!(zeus[0].kind(), Some("opencode"));
    assert_eq!(zeus[0].get("custom"), Some(&Value::Bool(true)));
    assert_eq!(rows.last().and_then(AgentRow::id), Some("zeus"));
}

#[test]
fn the_catalog_rows_come_first_the_customs_after_them_in_the_files_order() {
    let catalog = catalog_of(vec![
        preset("a", "codex", "m1"),
        preset("b", "codex", "m2"),
        preset("c", "codex", "m3"),
    ]);
    let rows = catalog.rows(&document_of(
        r#"{"id":"x","kind":"codex"},{"id":"b","kind":"pi"},{"id":"y","kind":"codex"}"#,
    ));
    assert_eq!(ids(&rows), ["a", "c", "x", "b", "y"]);
    let customs: Vec<bool> = rows.iter().map(|row| row.get("custom").is_some()).collect();
    assert_eq!(customs, [false, false, true, true, true]);
}

#[test]
fn custom_goes_in_place_when_the_row_has_it_and_last_when_it_does_not() {
    let catalog = catalog_of(vec![]);
    let rows = catalog.rows(&document_of(
        r#"{"id":"a","custom":false,"kind":"codex"},
           {"id":"b","kind":"codex"},
           {"custom":"yes","id":"c"}"#,
    ));
    let text: Vec<String> = rows.iter().map(written).collect();
    assert_eq!(
        text,
        [
            r#"{"id":"a","custom":true,"kind":"codex"}"#,
            r#"{"id":"b","kind":"codex","custom":true}"#,
            r#"{"custom":true,"id":"c"}"#,
        ]
    );
}

#[test]
fn two_custom_rows_of_one_name_are_both_listed() {
    let catalog = catalog_of(vec![preset("a", "codex", "m")]);
    let rows = catalog.rows(&document_of(
        r#"{"id":"nova","model":"first"},{"id":"nova","model":"second"}"#,
    ));
    let models: Vec<Option<&str>> = rows.iter().map(AgentRow::model).collect();
    assert_eq!(models, [Some("m"), Some("first"), Some("second")]);
}

#[test]
fn a_custom_row_with_no_id_is_listed_and_hides_nothing() {
    let catalog = catalog_of(vec![preset("a", "codex", "m")]);
    let rows = catalog.rows(&document_of(r#"{"kind":"codex"}"#));
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].id(), Some("a"));
    assert_eq!(written(&rows[1]), r#"{"kind":"codex","custom":true}"#);
}

#[test]
fn a_row_under_an_entrys_preset_and_another_id_is_the_humans_and_hides_only_its_own_name() {
    let catalog = catalog_of(vec![preset("a", "codex", "m")]);
    let rows = catalog.rows(&document_of(r#"{"id":"mine","preset":"a","kind":"codex"}"#));
    assert_eq!(ids(&rows), ["a", "mine"]);
}

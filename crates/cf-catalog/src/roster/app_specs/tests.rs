use serde_json::json;
use tempfile::tempdir;

use super::*;
use crate::roster::testing::file_with;
use crate::{Catalog, Roster};

fn read(text: &str) -> AppSpecs {
    let (_home, path) = file_with(text.as_bytes());
    app_specs_of(&load_document(&path).unwrap())
}

/// What the human says for every project: on or off, and the folder and the variant named.
fn says(on: bool, folder: Option<&str>, variant: Option<Variant>) -> AppSpecs {
    AppSpecs {
        on,
        folder: folder.map(str::to_owned),
        variant,
    }
}

/// What a set of `said` answers on a file that holds `text`, and the file's `preferences` after.
fn set_on(text: &str, said: Value) -> (Result<AppSpecs, String>, Value) {
    let (_home, path) = file_with(text.as_bytes());
    let set = set_app_specs(&path, &said).map_err(|refusal| refusal.message);
    let kept: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    (set, kept["preferences"].clone())
}

#[test]
fn a_file_that_says_nothing_of_them_is_off_and_names_no_folder_or_variant() {
    for text in [
        "{}",
        r#"{"preferences":{}}"#,
        r#"{"preferences":{"ownHarnessOnly":true}}"#,
        r#"{"preferences":null}"#,
        r#"{"preferences":[true]}"#,
    ] {
        assert_eq!(read(text), AppSpecs::default(), "{text}");
    }
    let home = tempdir().unwrap();
    let missing = load_document(&home.path().join("agents.json")).unwrap();
    assert_eq!(app_specs_of(&missing), AppSpecs::default());
}

#[test]
fn each_part_is_read_apart_and_only_true_itself_turns_them_on() {
    assert_eq!(
        read(r#"{"preferences":{"specs":true}}"#),
        says(true, None, None)
    );
    assert_eq!(
        read(r#"{"preferences":{"specsFolder":"docs/specs","specsVariant":"tdd-html"}}"#),
        says(false, Some("docs/specs"), Some(Variant::TddHtml)),
        "named while off, they wait for the day they are on"
    );
    for other in ["false", r#""true""#, "1", "[true]", "null", "{}"] {
        let text = format!(r#"{{"preferences":{{"specs":{other}}}}}"#);
        assert!(!read(&text).on, "{other}");
    }
}

#[test]
fn a_folder_or_a_variant_a_file_edited_by_hand_cannot_have_is_read_as_none() {
    for folder in [
        r#""../outside""#,
        r#""/etc""#,
        r#""C:\\specs""#,
        r#""a/../../b""#,
        r#""""#,
        r#""   ""#,
        "5",
        "true",
        "null",
        r#"["docs"]"#,
    ] {
        let text = format!(r#"{{"preferences":{{"specsFolder":{folder}}}}}"#);
        assert_eq!(read(&text).folder, None, "{folder}");
    }
    for variant in [
        r#""Core""#,
        r#""html""#,
        r#""""#,
        "1",
        "null",
        r#"["core"]"#,
    ] {
        let text = format!(r#"{{"preferences":{{"specsVariant":{variant}}}}}"#);
        assert_eq!(read(&text).variant, None, "{variant}");
    }
    // A folder written by hand in another way is read as the project would keep it.
    assert_eq!(
        read(r#"{"preferences":{"specsFolder":".\\docs\\specs\\"}}"#).folder,
        Some("docs/specs".to_owned())
    );
}

#[test]
fn each_part_said_is_kept_only_where_it_says_something_and_the_roster_s_choice_stays() {
    let (set, kept) = set_on(
        r#"{"preferences":{"ownHarnessOnly":true}}"#,
        json!({ "on": true, "folder": " ./docs//specs/ ", "variant": "core-html" }),
    );
    let named = says(true, Some("docs/specs"), Some(Variant::CoreHtml));
    assert_eq!(set, Ok(named.clone()));
    assert_eq!(
        kept,
        json!({
            "ownHarnessOnly": true,
            "specs": true,
            "specsFolder": "docs/specs",
            "specsVariant": "core-html",
        }),
        "the folder as a project keeps its own, after the roster's choice"
    );
    let file = r#"{"preferences":{"ownHarnessOnly":true,"specs":true,"specsFolder":"docs/specs","specsVariant":"core-html"}}"#;
    // A part said alone leaves the others.
    let (set, _) = set_on(file, json!({ "variant": "tdd" }));
    assert_eq!(set, Ok(says(true, Some("docs/specs"), Some(Variant::Tdd))));
    // Off is not kept, and keeps the folder and the variant for the day it is on.
    let (set, kept) = set_on(file, json!({ "on": false }));
    assert_eq!(
        set,
        Ok(says(false, Some("docs/specs"), Some(Variant::CoreHtml)))
    );
    assert_eq!(
        kept,
        json!({ "ownHarnessOnly": true, "specsFolder": "docs/specs", "specsVariant": "core-html" })
    );
    // `null`, and a blank folder, name none again.
    for said in [
        json!({ "folder": null, "variant": null }),
        json!({ "folder": "   ", "variant": null }),
    ] {
        let (set, kept) = set_on(file, said.clone());
        assert_eq!(set, Ok(says(true, None, None)), "{said}");
        assert_eq!(
            kept,
            json!({ "ownHarnessOnly": true, "specs": true }),
            "{said}"
        );
    }
    // Nothing said changes nothing, and the whole is still written; a file is made where there is none.
    let (set, kept) = set_on(file, json!({}));
    assert_eq!(set, Ok(named));
    assert_eq!(kept["specsFolder"], "docs/specs");
    let home = tempdir().unwrap();
    let path = home.path().join("agents.json");
    assert_eq!(
        set_app_specs(&path, &json!({ "on": true })),
        Ok(says(true, None, None))
    );
    let made: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(
        made["preferences"],
        json!({ "ownHarnessOnly": false, "specs": true })
    );
}

#[test]
fn a_part_refused_is_said_in_a_project_s_words_with_its_own_code_and_keeps_nothing() {
    let file =
        r#"{"preferences":{"ownHarnessOnly":false,"specsFolder":"plans","specsVariant":"tdd"}}"#;
    let refused = |said: Value| {
        let (_home, path) = file_with(file.as_bytes());
        let refusal = set_app_specs(&path, &said).unwrap_err();
        let kept: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(
            kept["preferences"],
            json!({ "ownHarnessOnly": false, "specsFolder": "plans", "specsVariant": "tdd" }),
            "{said}: the file is as it was"
        );
        (refusal.code, refusal.message)
    };
    let inside = "the specs folder is a folder inside the project: ";
    for (folder, why) in [
        (json!("../elsewhere"), "it stays inside it, with no `..`"),
        (json!("/etc"), "it is relative to the project, not absolute"),
        (
            json!("C:\\specs"),
            "it is relative to the project, not absolute",
        ),
        (json!("."), "it is a folder of the project's, not its own"),
        (json!(3), "it is text"),
    ] {
        assert_eq!(
            refused(json!({ "folder": folder })),
            ("preference-specs-folder", format!("{inside}{why}")),
            "{folder}"
        );
    }
    for variant in ["Core", "html", "", "specmint-core"] {
        assert_eq!(
            refused(json!({ "variant": variant })),
            (
                "preference-specs-variant",
                format!(
                    "the specs variant is one of core, core-html, tdd, tdd-html, not {variant:?}"
                )
            )
        );
    }
    assert_eq!(
        refused(json!({ "variant": 3 })),
        (
            "preference-specs-variant",
            "the specs variant is a name".to_owned()
        )
    );
    for on in [json!("yes"), json!(null), json!(1)] {
        assert_eq!(
            refused(json!({ "on": on })),
            (
                "preference-specs",
                "specs for every project are on (true) or off (false)".to_owned()
            ),
            "{on}"
        );
    }
    // A request refused in its last part keeps nothing of its first.
    assert_eq!(
        refused(json!({ "on": true, "folder": "docs", "variant": "full" })).0,
        "preference-specs-variant"
    );
}

#[test]
fn they_are_read_and_set_through_the_roster_and_survive_the_fold_it_makes_at_start() {
    let (_home, path) = file_with(br#"{"preferences":{"ownHarnessOnly":true}}"#);
    let catalog = Catalog::bundled().unwrap();
    let roster = Roster::new(&catalog, path);
    let set = roster
        .set_app_specs(&json!({ "on": true, "folder": "docs/specs", "variant": "tdd" }))
        .unwrap();
    roster.normalize().unwrap();
    assert_eq!(roster.app_specs().unwrap(), set);
    assert!(
        roster.preferences().unwrap().own_harness_only,
        "and the roster's choice beside them"
    );
    // The roster's choice set keeps them.
    roster
        .set_preferences(Some(&json!({ "ownHarnessOnly": false })))
        .unwrap();
    assert_eq!(roster.app_specs().unwrap(), set);
}

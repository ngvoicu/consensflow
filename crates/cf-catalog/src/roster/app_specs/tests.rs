use serde_json::json;
use tempfile::tempdir;

use super::*;
use crate::roster::testing::file_with;
use crate::{Catalog, Roster};

mod released;

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

/// The file at `path`, read back.
fn file_at(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

/// What the file keeps of the human's choices: its `preferences`, and the
/// folder and the variant every project uses (`specsDefaults`), `null` where
/// it has none.
fn choices_in(file: &Value) -> (Value, Value) {
    (file["preferences"].clone(), file[DEFAULTS].clone())
}

/// What a set of `said` answers on a file that holds `text`, and what the file keeps after.
fn set_on(text: &str, said: Value) -> (Result<AppSpecs, String>, (Value, Value)) {
    let (_home, path) = file_with(text.as_bytes());
    let set = set_app_specs(&path, &said).map_err(|refusal| refusal.message);
    (set, choices_in(&file_at(&path)))
}

#[test]
fn a_file_that_says_nothing_of_them_is_off_and_names_no_folder_or_variant() {
    for text in [
        "{}",
        r#"{"preferences":{}}"#,
        r#"{"preferences":{"ownHarnessOnly":true}}"#,
        r#"{"preferences":null}"#,
        r#"{"preferences":[true]}"#,
        r#"{"specsDefaults":{}}"#,
        r#"{"specsDefaults":null}"#,
        r#"{"specsDefaults":"docs"}"#,
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
        read(r#"{"specsDefaults":{"folder":"docs/specs","variant":"tdd-html"}}"#),
        says(false, Some("docs/specs"), Some(Variant::TddHtml)),
        "named while off, they wait for the day they are on"
    );
    for other in ["false", r#""true""#, "1", "[true]", "null", "{}"] {
        let text = format!(r#"{{"preferences":{{"specs":{other}}}}}"#);
        assert!(!read(&text).on, "{other}");
    }
}

#[test]
fn where_an_earlier_build_of_this_one_kept_them_they_are_read_unless_specs_defaults_says_them() {
    // An earlier build kept them in `preferences`.
    assert_eq!(
        read(r#"{"preferences":{"specs":true,"specsFolder":"plans","specsVariant":"tdd"}}"#),
        says(true, Some("plans"), Some(Variant::Tdd))
    );
    // `specsDefaults` says the part it says; the other is the earlier place's.
    assert_eq!(
        read(
            r#"{"preferences":{"specsFolder":"plans","specsVariant":"tdd"},"specsDefaults":{"variant":"core-html"}}"#
        ),
        says(false, Some("plans"), Some(Variant::CoreHtml))
    );
    // A write moves them where they are kept now, and takes them out of `preferences`.
    let (set, (preferences, defaults)) = set_on(
        r#"{"preferences":{"ownHarnessOnly":true,"specs":true,"specsFolder":"plans","specsVariant":"tdd"}}"#,
        json!({ "on": true }),
    );
    assert_eq!(set, Ok(says(true, Some("plans"), Some(Variant::Tdd))));
    assert_eq!(
        preferences,
        json!({ "ownHarnessOnly": true, "specs": true })
    );
    assert_eq!(defaults, json!({ "folder": "plans", "variant": "tdd" }));
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
        for text in [
            format!(r#"{{"specsDefaults":{{"folder":{folder}}}}}"#),
            format!(r#"{{"preferences":{{"specsFolder":{folder}}}}}"#),
        ] {
            assert_eq!(read(&text).folder, None, "{text}");
        }
    }
    for variant in [
        r#""Core""#,
        r#""html""#,
        r#""""#,
        "1",
        "null",
        r#"["core"]"#,
    ] {
        for text in [
            format!(r#"{{"specsDefaults":{{"variant":{variant}}}}}"#),
            format!(r#"{{"preferences":{{"specsVariant":{variant}}}}}"#),
        ] {
            assert_eq!(read(&text).variant, None, "{text}");
        }
    }
    // A folder written by hand in another way is read as the project would keep it.
    assert_eq!(
        read(r#"{"specsDefaults":{"folder":".\\docs\\specs\\"}}"#).folder,
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
        (
            json!({ "ownHarnessOnly": true, "specs": true }),
            json!({ "folder": "docs/specs", "variant": "core-html" })
        ),
        "the folder as a project keeps its own, the two under a key of their own"
    );
    let file = r#"{"preferences":{"ownHarnessOnly":true,"specs":true},"specsDefaults":{"folder":"docs/specs","variant":"core-html"}}"#;
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
        (
            json!({ "ownHarnessOnly": true }),
            json!({ "folder": "docs/specs", "variant": "core-html" })
        )
    );
    // `null`, and a blank folder, name none again, and where neither is named the key goes.
    for said in [
        json!({ "folder": null, "variant": null }),
        json!({ "folder": "   ", "variant": null }),
    ] {
        let (set, kept) = set_on(file, said.clone());
        assert_eq!(set, Ok(says(true, None, None)), "{said}");
        assert_eq!(
            kept,
            (
                json!({ "ownHarnessOnly": true, "specs": true }),
                Value::Null
            ),
            "{said}"
        );
    }
    // Nothing said changes nothing, and the whole is still written; a file is made where there is none.
    let (set, (_, defaults)) = set_on(file, json!({}));
    assert_eq!(set, Ok(named));
    assert_eq!(defaults["folder"], "docs/specs");
    let home = tempdir().unwrap();
    let path = home.path().join("agents.json");
    assert_eq!(
        set_app_specs(&path, &json!({ "on": true })),
        Ok(says(true, None, None))
    );
    assert_eq!(
        choices_in(&file_at(&path)),
        (
            json!({ "ownHarnessOnly": false, "specs": true }),
            Value::Null
        )
    );
}

#[test]
fn a_part_refused_is_said_in_a_project_s_words_with_its_own_code_and_keeps_nothing() {
    let file = r#"{"preferences":{"ownHarnessOnly":false},"specsDefaults":{"folder":"plans","variant":"tdd"}}"#;
    let refused = |said: Value| {
        let (_home, path) = file_with(file.as_bytes());
        let refusal = set_app_specs(&path, &said).unwrap_err();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            file,
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
fn what_a_later_build_keeps_in_preferences_and_beside_the_defaults_survives_every_write_of_this_one(
) {
    let file = r#"{"preferences":{"ownHarnessOnly":true,"colour":"green","specs":true,"later":{"x":1}},"specsDefaults":{"folder":"plans","mode":"strict"},"elsewhere":[1]}"#;
    let catalog = Catalog::bundled().unwrap();
    for write in ["the app's specs", "the roster's choice"] {
        let (_home, path) = file_with(file.as_bytes());
        let roster = Roster::new(&catalog, path.clone());
        match write {
            "the app's specs" => {
                roster.set_app_specs(&json!({ "variant": "tdd" })).unwrap();
            }
            _ => {
                roster
                    .set_preferences(Some(&json!({ "ownHarnessOnly": false })))
                    .unwrap();
            }
        }
        let after = file_at(&path);
        let (preferences, defaults) = choices_in(&after);
        assert_eq!(preferences["colour"], "green", "{write}");
        assert_eq!(preferences["later"], json!({ "x": 1 }), "{write}");
        assert_eq!(defaults["mode"], "strict", "{write}");
        assert_eq!(defaults["folder"], "plans", "{write}");
        assert_eq!(after["elsewhere"], json!([1]), "{write}");
        // This build's own keys come first, as Node and 3.0.4 wrote them; a later build's after.
        let keys: Vec<&str> = preferences
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            ["ownHarnessOnly", "specs", "colour", "later"],
            "{write}"
        );
    }
}

#[test]
fn a_file_this_build_wrote_keeps_the_defaults_once_the_released_3_0_4_has_saved_it() {
    let home = tempdir().unwrap();
    let path = home.path().join("agents.json");
    let catalog = Catalog::bundled().unwrap();
    let roster = Roster::new(&catalog, path.clone());
    let set = roster
        .set_app_specs(&json!({ "on": true, "folder": "docs/specs", "variant": "tdd" }))
        .unwrap();
    // 3.0.4 reads the switch where it always did.
    assert_eq!(
        released::preferences_of(&load_document(&path).unwrap()),
        released::Choices {
            own_harness_only: false,
            specs: true
        }
    );
    // The human keeps to the harnesses' own models in 3.0.4, which saves its two choices.
    released::set_preferences(&path, Some(&json!({ "ownHarnessOnly": true }))).unwrap();
    assert_eq!(roster.app_specs().unwrap(), set);
    assert_eq!(
        choices_in(&file_at(&path)),
        (
            json!({ "ownHarnessOnly": true, "specs": true }),
            json!({ "folder": "docs/specs", "variant": "tdd" })
        )
    );
    // 3.0.4 turns specs off for every project: the folder and the variant wait for the day they are on.
    released::set_preferences(&path, Some(&json!({ "specs": false }))).unwrap();
    assert_eq!(
        roster.app_specs().unwrap(),
        says(false, Some("docs/specs"), Some(Variant::Tdd))
    );
    // And this build writes again what 3.0.4 left.
    roster
        .set_preferences(Some(&json!({ "ownHarnessOnly": false })))
        .unwrap();
    assert_eq!(
        roster.app_specs().unwrap(),
        says(false, Some("docs/specs"), Some(Variant::Tdd))
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

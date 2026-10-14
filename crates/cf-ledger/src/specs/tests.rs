use serde_json::json;

use super::*;

fn folder(text: &str) -> Result<Option<String>, String> {
    folder_of(text).map_err(|refused| refused.to_string())
}

#[test]
fn a_folder_is_kept_as_the_project_s_own_in_slashes() {
    for (given, kept) in [
        (".specs", ".specs"),
        ("docs/specs", "docs/specs"),
        ("./docs/specs/", "docs/specs"),
        ("docs//specs", "docs/specs"),
        (r"docs\specs", "docs/specs"),
        ("  plans  ", "plans"),
        ("a/./b", "a/b"),
        ("..specs", "..specs"),
        ("café/планы", "café/планы"),
    ] {
        assert_eq!(folder(given), Ok(Some(kept.to_owned())), "{given:?}");
    }
}

#[test]
fn a_blank_folder_is_none_and_the_project_keeps_the_default() {
    for blank in ["", "   ", "\t "] {
        assert_eq!(folder(blank), Ok(None), "{blank:?}");
    }
}

#[test]
fn a_folder_that_leaves_the_project_is_refused() {
    for (given, why) in [
        ("/etc", "it is relative to the project, not absolute"),
        (r"\etc", "it is relative to the project, not absolute"),
        (
            "//server/share",
            "it is relative to the project, not absolute",
        ),
        (
            r"\\server\share",
            "it is relative to the project, not absolute",
        ),
        ("C:/specs", "it is relative to the project, not absolute"),
        (r"c:\specs", "it is relative to the project, not absolute"),
        ("C:specs", "it is relative to the project, not absolute"),
        ("docs/a:b", "its name has no `:`"),
        ("x:y", "it is relative to the project, not absolute"),
        ("1:y", "its name has no `:`"),
        ("..", "it stays inside it, with no `..`"),
        ("../specs", "it stays inside it, with no `..`"),
        ("docs/../../specs", "it stays inside it, with no `..`"),
        (r"docs\..\specs", "it stays inside it, with no `..`"),
        ("docs/..", "it stays inside it, with no `..`"),
        (".", "it is a folder of the project's, not its own"),
        ("./", "it is a folder of the project's, not its own"),
        ("././", "it is a folder of the project's, not its own"),
        ("a\nb", "its name has no control characters"),
        ("a\0b", "its name has no control characters"),
    ] {
        assert_eq!(
            folder(given),
            Err(format!(
                "the specs folder is a folder inside the project: {why}"
            )),
            "{given:?}"
        );
    }
    let long = "x".repeat(FOLDER_MAX + 1);
    assert!(folder(&long)
        .unwrap_err()
        .ends_with("at most 200 characters"));
    assert_eq!(
        folder(&"x".repeat(FOLDER_MAX)),
        Ok(Some("x".repeat(FOLDER_MAX)))
    );
}

#[test]
fn a_request_is_read_part_by_part_and_what_it_leaves_out_is_none() {
    let change = |request: serde_json::Value| SpecsChange::from_json(&request);
    assert_eq!(change(json!({})).unwrap(), SpecsChange::default());
    assert_eq!(
        change(json!({ "on": null, "folder": null, "variant": null })).unwrap(),
        SpecsChange::default()
    );
    assert_eq!(
        change(json!({ "on": true, "folder": "docs/specs", "variant": "tdd-html" })).unwrap(),
        SpecsChange {
            on: Some(true),
            folder: Some("docs/specs".to_owned()),
            variant: Some(Variant::TddHtml),
        }
    );
    assert_eq!(change(json!({ "on": false })).unwrap().on, Some(false));
    assert_eq!(change(json!({ "folder": " " })).unwrap().folder, None);
}

#[test]
fn a_part_of_the_wrong_kind_or_the_wrong_word_is_refused_with_its_own_code() {
    let refused = |request: serde_json::Value| {
        let failed = SpecsChange::from_json(&request).unwrap_err();
        (failed.code(), failed.to_string())
    };
    for on in [json!("yes"), json!(1), json!(0), json!([]), json!({})] {
        assert_eq!(
            refused(json!({ "on": on })),
            (
                Some("invalid-specs"),
                "specs are on (true), off (false) or follow the app's setting (null)".to_owned()
            ),
            "{on}"
        );
    }
    for variant in ["Core", "core-html ", "specmint-core", "all", "", "html"] {
        assert_eq!(
            refused(json!({ "variant": variant })),
            (
                Some("invalid-specs-variant"),
                format!(
                    "the specs variant is one of core, core-html, tdd, tdd-html, not {variant:?}"
                )
            ),
            "{variant:?}"
        );
    }
    assert_eq!(
        refused(json!({ "variant": 3 })),
        (
            Some("invalid-specs-variant"),
            "the specs variant is a name".to_owned()
        )
    );
    assert_eq!(
        refused(json!({ "folder": 3 })),
        (
            Some("invalid-specs-folder"),
            "the specs folder is a folder inside the project: it is text".to_owned()
        )
    );
    assert_eq!(
        refused(json!({ "folder": "../x" })).0,
        Some("invalid-specs-folder")
    );
}

#[test]
fn a_change_that_says_nothing_is_no_view() {
    assert_eq!(SpecsChange::default().view(), None);
    let only_off = SpecsChange {
        on: Some(false),
        ..SpecsChange::default()
    };
    assert_eq!(
        only_off.view(),
        Some(SpecsView {
            on: Some(false),
            folder: None,
            variant: None
        })
    );
}

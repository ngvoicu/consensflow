use serde_json::json;

use super::*;

/// The folder a request says, as the project keeps it: the checks are
/// `cf_proto::specs::check`'s, which holds them to every folder a person can type.
#[test]
fn a_folder_is_kept_as_the_project_s_own_in_slashes_and_a_blank_one_is_none() {
    let folder = |text: &str| SpecsChange::from_json(&json!({ "folder": text })).map(|c| c.folder);
    assert_eq!(
        folder("./docs//specs/").unwrap(),
        Some("docs/specs".to_owned())
    );
    assert_eq!(folder("  ").unwrap(), None);
    assert_eq!(
        folder("../elsewhere").unwrap_err().to_string(),
        "the specs folder is a folder inside the project: it stays inside it, with no `..`"
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

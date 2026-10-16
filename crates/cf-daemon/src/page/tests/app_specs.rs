//! What the app says of specs for every project, on the page: read and set over
//! the bridge against a real agents file, each part checked as a project's own
//! is, kept beside the human's other choices, and shown again; and the Specmint
//! this build bundles, which the page never types. Not held to a Node recording:
//! these operations are this build's own.

use cf_proto::specs::{AppSpecs, Bundled, BundledVariant, Variant};

use super::reading::home_with;
use super::*;

/// The page on a bridge, over a home whose agents file holds no agents and, where
/// given, these `preferences`.
struct Held {
    home: tempfile::TempDir,
    _daemon: Bridge,
    app: Bridge,
    rig: Rig,
}

fn held(preferences: Option<Value>) -> Held {
    let (home, env) = home_with("[]");
    if let Some(preferences) = preferences {
        let file = home.path().join("agents.json");
        let mut document: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
        document["preferences"] = preferences;
        std::fs::write(&file, document.to_string()).unwrap();
    }
    let rig = rig_over(env, None);
    let (daemon, app) = bridge_pair_over(&rig.spawn);
    register(&daemon, &rig.page, &rig.spawn);
    Held {
        home,
        _daemon: daemon,
        app,
        rig,
    }
}

impl Held {
    /// What the agents file keeps of the human's choices.
    fn kept(&self) -> Value {
        let file = std::fs::read(self.home.path().join("agents.json")).unwrap();
        serde_json::from_slice::<Value>(&file).unwrap()["preferences"].clone()
    }
}

#[tokio::test]
async fn a_human_who_said_nothing_has_specs_off_and_the_page_is_told_what_a_project_that_names_none_uses(
) {
    LocalSet::new()
        .run_until(async {
            let held = held(None);
            let answer = ask(&held.app, "specs.get", json!({})).await;
            assert_eq!(answer["ok"], true, "{answer}");
            assert_eq!(answer["app"], json!({ "on": false }));
            assert_eq!(
                answer["applied"],
                json!({ "folder": ".specs", "variant": "core" })
            );
            assert_eq!(held.rig.kicks.get(), 0, "a read wakes nothing");
            assert_eq!(held.kept(), Value::Null, "and writes nothing");
        })
        .await;
}

#[tokio::test]
async fn each_part_the_human_says_is_kept_in_the_agents_file_beside_their_other_choices_and_shown_again(
) {
    LocalSet::new()
        .run_until(async {
            let held = held(Some(json!({ "ownHarnessOnly": true })));
            let set = |body: Value| ask(&held.app, "specs.set", body);
            let answer = set(json!({ "on": true })).await;
            assert_eq!(answer["app"], json!({ "on": true }), "{answer}");
            assert_eq!(
                held.kept(),
                json!({ "ownHarnessOnly": true, "specs": true })
            );
            // The folder as a project's own is kept, and the applied one follows it.
            let answer = set(json!({ "folder": " .\\docs//specs/ " })).await;
            assert_eq!(
                answer["app"],
                json!({ "on": true, "folder": "docs/specs" }),
                "{answer}"
            );
            assert_eq!(
                answer["applied"],
                json!({ "folder": "docs/specs", "variant": "core" })
            );
            let answer = set(json!({ "variant": "tdd-html" })).await;
            assert_eq!(
                answer["app"],
                json!({ "on": true, "folder": "docs/specs", "variant": "tdd-html" })
            );
            assert_eq!(
                answer["applied"],
                json!({ "folder": "docs/specs", "variant": "tdd-html" })
            );
            assert_eq!(
                held.kept(),
                json!({
                    "ownHarnessOnly": true,
                    "specs": true,
                    "specsFolder": "docs/specs",
                    "specsVariant": "tdd-html",
                })
            );
            // Read again, it is what is kept; a change wakes the dispatcher once each.
            let read = ask(&held.app, "specs.get", json!({})).await;
            assert_eq!(read["app"], answer["app"]);
            assert_eq!(read["applied"], answer["applied"]);
            assert_eq!(held.rig.kicks.get(), 3);
            // Off keeps the folder and the variant for the day it is on again.
            let off = set(json!({ "on": false })).await;
            assert_eq!(
                off["app"],
                json!({ "on": false, "folder": "docs/specs", "variant": "tdd-html" })
            );
            // `null` and a blank folder name none again, and what is left out stays.
            let cleared = set(json!({ "folder": null, "variant": null })).await;
            assert_eq!(cleared["app"], json!({ "on": false }), "{cleared}");
            let blank = set(json!({ "folder": "docs", "variant": "tdd" })).await;
            assert_eq!(
                blank["app"],
                json!({ "on": false, "folder": "docs", "variant": "tdd" })
            );
            let blanked = set(json!({ "folder": "   " })).await;
            assert_eq!(
                blanked["app"],
                json!({ "on": false, "variant": "tdd" }),
                "{blanked}"
            );
            assert_eq!(
                held.kept(),
                json!({ "ownHarnessOnly": true, "specsVariant": "tdd" })
            );
        })
        .await;
}

#[tokio::test]
async fn a_folder_that_leaves_the_project_a_variant_that_is_none_and_a_switch_that_is_none_are_refused_and_keep_nothing(
) {
    LocalSet::new()
        .run_until(async {
            let before =
                json!({ "ownHarnessOnly": false, "specsFolder": "plans", "specsVariant": "tdd" });
            let held = held(Some(before.clone()));
            let inside = "the specs folder is a folder inside the project: ";
            let switch = "specs for every project are on (true) or off (false)";
            for (body, error) in [
                (
                    json!({ "folder": "../elsewhere" }),
                    format!("{inside}it stays inside it, with no `..`"),
                ),
                (
                    json!({ "folder": "/etc/specs" }),
                    format!("{inside}it is relative to the project, not absolute"),
                ),
                (json!({ "folder": 4 }), format!("{inside}it is text")),
                (
                    json!({ "variant": "full" }),
                    "the specs variant is one of core, core-html, tdd, tdd-html, not \"full\""
                        .to_owned(),
                ),
                (
                    json!({ "variant": 3 }),
                    "the specs variant is a name".to_owned(),
                ),
                (json!({ "on": "yes" }), switch.to_owned()),
                (json!({ "on": null }), switch.to_owned()),
                // A request refused in its second part keeps nothing of its first.
                (
                    json!({ "on": true, "folder": "../x" }),
                    format!("{inside}it stays inside it, with no `..`"),
                ),
            ] {
                assert_eq!(
                    ask(&held.app, "specs.set", body.clone()).await,
                    json!({ "ok": false, "error": error }),
                    "{body}"
                );
            }
            assert_eq!(held.kept(), before, "the file is as it was");
            assert_eq!(
                held.rig.kicks.get(),
                0,
                "nothing succeeded, nothing is woken"
            );
        })
        .await;
}

#[tokio::test]
async fn the_specmint_the_page_is_told_of_is_read_from_the_bundled_files_in_the_order_the_page_offers_the_variants(
) {
    LocalSet::new()
        .run_until(async {
            let held = held(None);
            let bundled = cf_harness::bundled_specmint();
            for operation in ["specs.get", "specs.set"] {
                let answer = ask(&held.app, operation, json!({})).await;
                assert_eq!(
                    answer["specmint"],
                    serde_json::to_value(&bundled).unwrap(),
                    "{operation}"
                );
            }
            let answer = ask(&held.app, "specs.get", json!({})).await;
            let names: Vec<&str> = answer["specmint"]["variants"]
                .as_array()
                .unwrap()
                .iter()
                .map(|variant| variant["name"].as_str().unwrap())
                .collect();
            assert_eq!(names, ["core", "core-html", "tdd", "tdd-html"]);
            let core = &answer["specmint"]["variants"][0];
            assert_eq!(core["commit"].as_str().unwrap().len(), 7, "{core}");
            assert!(
                answer["specmint"]["version"]
                    .as_str()
                    .unwrap()
                    .contains('.'),
                "{answer}"
            );
        })
        .await;
}

#[test]
fn the_human_s_settings_come_back_whatever_the_bundled_files_say_of_their_version() {
    // A build whose metadata says nothing: no variant names a version or a commit.
    let unknown = Bundled {
        version: None,
        variants: Variant::ALL
            .into_iter()
            .map(|variant| BundledVariant {
                name: variant.as_str().to_owned(),
                version: None,
                commit: None,
                about: variant.about().to_owned(),
            })
            .collect(),
    };
    let app = AppSpecs {
        on: true,
        folder: Some("docs/specs".to_owned()),
        variant: Some(Variant::Tdd),
    };
    let fields = crate::page::app_specs::answered(&app, &unknown).expect("an answer");
    assert_eq!(
        fields["app"],
        json!({ "on": true, "folder": "docs/specs", "variant": "tdd" })
    );
    assert_eq!(
        fields["applied"],
        json!({ "folder": "docs/specs", "variant": "tdd" })
    );
    assert_eq!(fields["specmint"]["version"], Value::Null);
    assert_eq!(fields["specmint"]["variants"][2]["version"], Value::Null);
    assert_eq!(fields["specmint"]["variants"][2]["commit"], Value::Null);
    assert_eq!(
        fields["specmint"]["variants"].as_array().map(Vec::len),
        Some(4)
    );
}

#[tokio::test]
async fn a_home_with_no_agents_file_has_specs_off_and_the_first_word_said_makes_the_file() {
    LocalSet::new()
        .run_until(async {
            let held = held(None);
            let file = held.home.path().join("agents.json");
            std::fs::remove_file(&file).unwrap();
            let read = ask(&held.app, "specs.get", json!({})).await;
            assert_eq!(read["app"], json!({ "on": false }), "{read}");
            assert!(!file.exists(), "a read makes nothing");
            let set = ask(&held.app, "specs.set", json!({ "on": true })).await;
            assert_eq!(set["app"], json!({ "on": true }), "{set}");
            assert_eq!(
                held.kept(),
                json!({ "ownHarnessOnly": false, "specs": true })
            );
        })
        .await;
}

#[tokio::test]
async fn an_agents_file_that_cannot_be_read_is_refused_in_its_words_and_nothing_is_made() {
    LocalSet::new()
        .run_until(async {
            let held = held(None);
            std::fs::write(held.home.path().join("agents.json"), "{ not json").unwrap();
            for (operation, body) in [
                ("specs.get", json!({})),
                ("specs.set", json!({ "on": true })),
            ] {
                let answer = ask(&held.app, operation, body).await;
                assert_eq!(answer["ok"], false, "{operation}: {answer}");
                assert!(
                    answer["error"].as_str().unwrap().contains("agents.json"),
                    "{answer}"
                );
            }
            assert_eq!(
                std::fs::read_to_string(held.home.path().join("agents.json")).unwrap(),
                "{ not json",
                "the file is left as it is"
            );
        })
        .await;
}

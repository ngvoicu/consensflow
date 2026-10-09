//! The rule of the feeds: the manifest it reads, which release comes before
//! which, the feeds a release moves, and what the apps before the bridge
//! require of an archive. (The plan from the command line is
//! tests/feeds_plan.rs, the checks against GitHub are
//! tests/feeds_prerequisites.rs, tests/feeds_check.rs and tests/feeds_cli.rs.)

// A failure in a test is the test's answer, in its helpers too.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::fs;

use cf_publish::assets::{members_of, missing_for_old_apps, ReleaseAssets};
use cf_publish::manifest::Manifest;
use cf_publish::rule::{feeds_moved, plan_feeds, role_of, Role};
use cf_publish::testing::{
    archive_of, latest_json, node_free_layout, old_apps_layout, rule, rule_with, v, TempDir,
    BRIDGE, LATER, OLDER,
};
use cf_publish::version::{later_release, Channel, Version};
use serde_json::{json, Value};

mod the_manifest_the_app_and_the_release_workflow_read {
    use super::*;

    #[test]
    fn names_the_feeds_of_both_generations_as_an_installed_app_reads_them() {
        let manifest = Manifest::embedded().unwrap();
        assert_eq!(manifest.feeds.of(Channel::Alpha), "feed-alpha");
        assert_eq!(manifest.feeds.of(Channel::Stable), "feed-stable");
        assert_eq!(manifest.legacy.of(Channel::Alpha), "update-alpha");
        assert_eq!(manifest.legacy.of(Channel::Stable), "update-stable");
    }

    #[test]
    fn records_the_bridge_and_the_old_channels_in_use_alpha_for_stable_has_no_release_yet() {
        let manifest = Manifest::embedded().unwrap();
        let bridge = manifest.bridge.version.as_str();
        let number = bridge.strip_prefix("3.0.0-alpha.").unwrap();
        assert!(
            !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit()),
            "{bridge}"
        );
        assert_eq!(manifest.bridge.legacy, [Channel::Alpha]);
    }

    #[test]
    fn refuses_a_manifest_that_does_not_say_what_the_rule_needs() {
        let made = |change: fn(&mut Value)| {
            let mut manifest = json!({
                "feeds": { "alpha": "feed-alpha", "stable": "feed-stable" },
                "legacy": { "alpha": "update-alpha", "stable": "update-stable" },
                "bridge": { "version": BRIDGE, "legacy": ["alpha"] },
            });
            change(&mut manifest);
            manifest
        };
        type Change = fn(&mut Value);
        let cases: [(Change, &str); 7] = [
            (
                |m| drop(m["feeds"].as_object_mut().unwrap().remove("stable")),
                "feeds.stable names no feed",
            ),
            (
                |m| drop(m["legacy"].as_object_mut().unwrap().remove("alpha")),
                "legacy.alpha names no feed",
            ),
            (
                |m| m["legacy"]["alpha"] = json!("feed-alpha"),
                "a feed is named twice",
            ),
            (
                |m| drop(m.as_object_mut().unwrap().remove("bridge")),
                "bridge.version names no release",
            ),
            (
                |m| m["bridge"]["version"] = json!("v3.0.0"),
                "not a semantic version",
            ),
            (
                |m| m["bridge"]["legacy"] = json!([]),
                "bridge.legacy names the channels in use",
            ),
            (
                |m| m["bridge"]["legacy"] = json!(["stable"]),
                "bridge.legacy names the channels in use that 3.0.0-alpha.81 belongs to (alpha)",
            ),
        ];
        for (change, says) in cases {
            let refused = Manifest::from_json(&made(change)).unwrap_err().to_string();
            assert!(refused.contains(says), "{refused} does not say {says}");
        }
        let whole = Manifest::from_json(&made(|_| {})).unwrap();
        assert_eq!(whole.bridge.version.as_str(), BRIDGE);
    }
}

mod which_release_comes_before_which {
    use super::*;

    #[test]
    fn orders_versions_as_semantic_versioning_does() {
        let ascending = [
            "1.0.0-alpha",
            "1.0.0-alpha.1",
            "1.0.0-alpha.beta",
            "1.0.0-beta",
            "1.0.0-beta.2",
            "1.0.0-beta.11",
            "1.0.0-rc.1",
            "1.0.0",
            "1.0.1",
            "1.1.0",
            "2.0.0",
            "10.0.0",
        ];
        for (at, version) in ascending.iter().enumerate() {
            assert_eq!(v(version), v(version), "{version}");
            for larger in &ascending[at + 1..] {
                assert!(v(version) < v(larger), "{version} < {larger}");
                assert!(v(larger) > v(version), "{larger} > {version}");
            }
        }
        assert!(v("3.0.0-alpha.9") < v("3.0.0-alpha.10"));
        assert!(v(BRIDGE) < v("3.0.0"));
        assert!(v(OLDER) < v(BRIDGE), "the release before the bridge");
        assert!(v(LATER) > v(BRIDGE), "the release after it");
        assert!(Version::parse("3.0")
            .unwrap_err()
            .to_string()
            .contains("not a semantic version"));
    }

    #[test]
    fn says_which_release_a_feed_names_where_that_one_is_after_a_given_release_and_none_where_it_is_not(
    ) {
        let feed = |version: &str| latest_json("http://example.test", version, "notes");
        let later = |body: &str, version: &str| {
            later_release(body.as_bytes(), &v(version)).map(|named| named.to_string())
        };
        assert_eq!(later(&feed(LATER), BRIDGE).as_deref(), Some(LATER));
        assert_eq!(
            later(&feed("3.0.0-alpha.10"), "3.0.0-alpha.9").as_deref(),
            Some("3.0.0-alpha.10")
        );
        assert_eq!(
            later(&feed("3.0.0"), "3.0.0-alpha.99").as_deref(),
            Some("3.0.0"),
            "a release is after its alphas"
        );
        assert_eq!(
            later(&feed("3.1.0-alpha.1"), "3.0.1").as_deref(),
            Some("3.1.0-alpha.1")
        );
        for (name, body, version) in [
            ("the same release", feed(BRIDGE), BRIDGE),
            ("an earlier one", feed(OLDER), BRIDGE),
            (
                "an alpha, for the release it leads to",
                feed("3.0.0-alpha.99"),
                "3.0.0",
            ),
            (
                "an earlier alpha number",
                feed("3.0.0-alpha.9"),
                "3.0.0-alpha.10",
            ),
            (
                "something that is not JSON",
                "<html>an error page</html>".to_string(),
                BRIDGE,
            ),
            ("an empty body", String::new(), BRIDGE),
            ("JSON of nothing", "null".to_string(), BRIDGE),
            ("JSON of a list", "[]".to_string(), BRIDGE),
            (
                "JSON with no version",
                r#"{"notes":"none"}"#.to_string(),
                BRIDGE,
            ),
            (
                "a version that is not a string",
                r#"{"version":9}"#.to_string(),
                BRIDGE,
            ),
            (
                "a version that is not a semantic one",
                r#"{"version":"99"}"#.to_string(),
                BRIDGE,
            ),
            (
                "a version with a prefix",
                r#"{"version":"v99.0.0"}"#.to_string(),
                BRIDGE,
            ),
        ] {
            assert_eq!(later(&body, version), None, "{name}");
        }
    }
}

mod the_feeds_a_release_moves {
    use super::*;

    #[test]
    fn takes_every_release_into_alpha_and_a_stable_one_into_stable_as_well() {
        assert_eq!(v(BRIDGE).channels(), [Channel::Alpha]);
        assert_eq!(v("3.0.0").channels(), [Channel::Alpha, Channel::Stable]);
        for refused in ["v3.0.0", "3.0"] {
            let said = Version::parse(refused).unwrap_err().to_string();
            assert!(said.contains("not a semantic version"), "{said}");
        }
    }

    #[test]
    fn tells_the_bridge_from_a_later_release_and_refuses_one_before_it() {
        let rule = rule();
        assert_eq!(role_of(&v(BRIDGE), &rule).unwrap(), Role::Bridge);
        assert_eq!(role_of(&v(LATER), &rule).unwrap(), Role::Later);
        assert_eq!(role_of(&v("3.0.0"), &rule).unwrap(), Role::Later);
        let refused = role_of(&v(OLDER), &rule).unwrap_err().to_string();
        assert!(
            refused.starts_with("3.0.0-alpha.80 comes before the bridge 3.0.0-alpha.81")
                && refused.ends_with("set bridge.version to the first release made from this tree"),
            "{refused}"
        );
    }

    #[test]
    fn moves_the_bridge_to_the_new_feed_and_the_old_feed_of_the_channel_in_use_the_new_first() {
        assert_eq!(
            plan_feeds(&v(BRIDGE), &[], &rule()).unwrap(),
            ["feed-alpha", "update-alpha"]
        );
    }

    #[test]
    fn moves_a_later_release_to_the_new_feeds_only_no_old_feed_is_so_much_as_named() {
        assert_eq!(plan_feeds(&v(LATER), &[], &rule()).unwrap(), ["feed-alpha"]);
        assert_eq!(
            feeds_moved(&v("3.0.1"), &rule()).unwrap(),
            ["feed-alpha", "feed-stable"]
        );
    }

    #[test]
    fn moves_the_old_feeds_of_the_channels_in_use_only_even_where_the_bridge_is_a_stable_release() {
        let stable = rule_with("3.0.0", &["alpha"]);
        assert_eq!(
            feeds_moved(&v("3.0.0"), &stable).unwrap(),
            ["feed-alpha", "feed-stable", "update-alpha"]
        );
        let both = rule_with("3.0.0", &["alpha", "stable"]);
        assert_eq!(
            feeds_moved(&v("3.0.0"), &both).unwrap(),
            ["feed-alpha", "feed-stable", "update-alpha", "update-stable"]
        );
    }

    #[test]
    fn refuses_a_release_before_the_bridge_nothing_is_planned_for_it() {
        let refused = plan_feeds(&v(OLDER), &[], &rule()).unwrap_err().to_string();
        assert!(refused.contains("comes before"), "{refused}");
    }

    #[test]
    fn refuses_a_bridge_the_old_apps_would_download_and_then_refuse() {
        let missing = missing_for_old_apps(&node_free_layout());
        let refused = plan_feeds(&v(BRIDGE), &missing, &rule())
            .unwrap_err()
            .to_string();
        assert!(
            refused.contains("must keep the layout") && refused.contains("MacOS/node"),
            "{refused}"
        );
        // A later release may be laid out any way: the old feeds are not its to move.
        assert_eq!(
            plan_feeds(&v("3.0.0-alpha.90"), &missing, &rule()).unwrap(),
            ["feed-alpha"]
        );
    }

    #[test]
    fn names_the_files_a_release_publishes_in_the_order_it_uploads_them() {
        let assets = ReleaseAssets::of(&v("3.0.0-alpha.81"));
        assert_eq!(
            assets.all(),
            [
                "ConsensFlow_3.0.0-alpha.81_aarch64.dmg",
                "ConsensFlow_3.0.0-alpha.81_aarch64.app.tar.gz",
                "ConsensFlow_3.0.0-alpha.81_aarch64.app.tar.gz.sig",
                "latest.json",
                "nsis/ConsensFlow_3.0.0-alpha.81_x64-setup.exe",
                "portable/ConsensFlow_3.0.0-alpha.81_x64-portable.exe",
            ]
        );
        assert_eq!(assets.metadata, "latest.json");
    }
}

mod what_the_apps_before_the_bridge_require_of_an_archive {
    use super::*;

    #[test]
    fn is_nothing_missing_from_the_layout_they_install() {
        assert_eq!(
            missing_for_old_apps(&old_apps_layout()),
            Vec::<String>::new()
        );
        let listed: Vec<String> = old_apps_layout()
            .iter()
            .map(|name| format!("./{name}"))
            .collect();
        assert_eq!(
            missing_for_old_apps(&listed),
            Vec::<String>::new(),
            "as tar lists them with a leading ./"
        );
    }

    #[test]
    fn names_each_file_or_folder_that_is_not_there() {
        type Gone = fn(&str) -> bool;
        let cases: [(&str, Gone); 6] = [
            ("MacOS/node", |n| n != "ConsensFlow.app/Contents/MacOS/node"),
            ("package.json", |n| !n.ends_with("/package.json")),
            ("cf.mjs", |n| !n.ends_with("/cf.mjs")),
            ("bin/cf", |n| !n.ends_with("/bin/cf")),
            ("hosts", |n| !n.contains("/hosts/")),
            ("src", |n| !n.contains("/src/")),
        ];
        for (name, gone) in cases {
            let kept: Vec<String> = old_apps_layout().into_iter().filter(|n| gone(n)).collect();
            let missing = missing_for_old_apps(&kept);
            assert_eq!(missing.len(), 1, "{name}: {missing:?}");
            assert!(
                missing[0].ends_with(name) || missing[0].ends_with(&format!("{name}/")),
                "{name}: {missing:?}"
            );
        }
    }

    #[test]
    #[cfg_attr(windows, ignore = "the release pipeline is macOS and Linux")]
    fn is_read_off_a_real_archive() {
        let dir = TempDir::new("feeds");
        let archive = |name: &str, files: &[String]| {
            let names: Vec<&str> = files.iter().map(String::as_str).collect();
            let target = dir.path().join(name);
            fs::write(&target, archive_of(&names)).unwrap();
            target
        };
        let old = archive("old.tar.gz", &old_apps_layout());
        assert_eq!(
            missing_for_old_apps(&members_of(&old).unwrap()),
            Vec::<String>::new()
        );
        let free = archive("free.tar.gz", &node_free_layout());
        let missing = missing_for_old_apps(&members_of(&free).unwrap());
        assert_eq!(missing.len(), 5, "{missing:?}");
    }
}

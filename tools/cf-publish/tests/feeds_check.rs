//! After a release is published: the files and the feeds serve what the rule
//! says, read back from GitHub as the simulator has it. (The same from the
//! command line is tests/feeds_cli.rs.)

// A failure in a test is the test's answer, in its helpers too.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use cf_publish::checks::{check_feeds, FEEDS_BEFORE};
use cf_publish::testing::worlds::{after_publishing, after_publishing_with, feed, serving, After};
use cf_publish::testing::{
    archive_name as archive, download, installer_name as installer, latest_json,
    portable_name as portable, quick, rule, v, with_archive_url, Github, Served, BRIDGE, LATER,
    OLDER,
};

/// What `check_feeds` finds for `version`, once `change` has altered GitHub.
fn found(version: &str, change: impl FnOnce(&Github, &After)) -> Vec<String> {
    let world = after_publishing(version);
    change(&world.github, &world);
    world.check()
}

const DOES_NOT_SERVE: &str = "feed-alpha does not serve this release's latest.json";

mod after_a_release_is_published_the_files_and_the_feeds_serve_what_the_rule_says {
    use super::*;

    #[test]
    fn passes_a_later_release_its_files_and_its_feed_the_old_feed_pinned_to_the_bridge() {
        assert_eq!(found(LATER, |_, _| {}), Vec::<String>::new());
    }

    #[test]
    fn passes_the_bridge_the_new_feed_and_the_old_feed_both_serve_it() {
        assert_eq!(found(BRIDGE, |_, _| {}), Vec::<String>::new());
    }

    #[test]
    fn finds_the_bridge_not_carried_to_the_old_feed_it_still_serves_the_release_before() {
        let problems = found(BRIDGE, |github, _| serving(github, "update-alpha", OLDER));
        assert_eq!(
            problems,
            ["update-alpha serves 3.0.0-alpha.80, not the bridge 3.0.0-alpha.81: the apps that read it do not reach the bridge"]
        );
    }

    #[test]
    fn finds_the_old_feed_no_longer_pinned_to_the_bridge_after_a_later_release() {
        let problems = found(LATER, |github, _| serving(github, "update-alpha", LATER));
        assert!(
            problems[0].starts_with("update-alpha serves 3.0.0-alpha.82, not the bridge"),
            "{problems:?}"
        );
    }

    #[test]
    fn finds_a_required_old_feed_that_is_not_there_or_fails_to_load() {
        for (instead, says) in [
            (
                Served::Status(404),
                "update-alpha cannot be read (HTTP 404)",
            ),
            (
                Served::Status(503),
                "update-alpha cannot be read (HTTP 503)",
            ),
            (Served::Reset, "update-alpha cannot be read (unreachable: "),
        ] {
            let problems = found(LATER, |github, _| {
                github.serve("/update-alpha/latest.json", instead.clone());
            });
            assert_eq!(problems.len(), 1, "{instead:?}: {problems:?}");
            assert!(problems[0].starts_with(says), "{problems:?}");
        }
    }

    #[test]
    fn finds_a_file_of_the_release_that_does_not_download_as_the_one_built_whichever_it_is() {
        for name in [
            archive(LATER),
            installer(LATER),
            portable(LATER),
            format!("ConsensFlow_{LATER}_aarch64.dmg"),
        ] {
            let problems = found(LATER, |github, _| {
                github.serve(&download(LATER, &name), "another file");
            });
            assert_eq!(
                problems,
                [format!(
                    "{name} does not download from v{LATER} as the one built"
                )],
                "{name}"
            );
        }
    }

    #[test]
    fn finds_a_file_of_the_release_that_is_not_served_or_whose_address_fails() {
        let gone = found(LATER, |github, _| {
            github.serve(&download(LATER, &portable(LATER)), Served::Status(404));
        });
        assert_eq!(
            gone,
            [format!(
                "{} cannot be downloaded from v{LATER} (HTTP 404)",
                portable(LATER)
            )]
        );
        let failing = found(LATER, |github, _| {
            github.serve(&download(LATER, &installer(LATER)), Served::Status(502));
        });
        assert_eq!(
            failing,
            [format!(
                "{} cannot be downloaded from v{LATER} (HTTP 502)",
                installer(LATER)
            )]
        );
    }

    #[test]
    fn finds_a_new_feed_that_keeps_serving_another_release_is_not_there_or_fails() {
        let other = found(LATER, |github, _| serving(github, "feed-alpha", OLDER));
        assert_eq!(other, [DOES_NOT_SERVE]);
        for (instead, says) in [
            (Served::Status(404), format!("{DOES_NOT_SERVE} (HTTP 404)")),
            (Served::Status(503), format!("{DOES_NOT_SERVE} (HTTP 503)")),
        ] {
            let problems = found(LATER, |github, _| {
                github.serve("/feed-alpha/latest.json", instead.clone())
            });
            assert_eq!(problems, [says]);
        }
        let dropped = found(LATER, |github, _| {
            github.serve("/feed-alpha/latest.json", Served::Reset)
        });
        assert!(
            dropped[0].starts_with(&format!("{DOES_NOT_SERVE} (unreachable: ")),
            "{dropped:?}"
        );
    }

    #[test]
    fn accepts_a_new_feed_that_names_a_later_release_which_a_run_of_this_release_left_alone_and_does_not_wait_for_it(
    ) {
        let world = after_publishing(LATER);
        serving(&world.github, "feed-alpha", "3.0.0-alpha.83");
        assert_eq!(world.check(), Vec::<String>::new());
        let reads = world
            .github
            .requests()
            .iter()
            .filter(|path| *path == "/feed-alpha/latest.json")
            .count();
        assert_eq!(
            reads, 1,
            "read once: no wait turns a later release into this one"
        );
    }

    #[test]
    fn does_not_approve_a_feed_that_names_an_earlier_release_this_one_with_other_bytes_or_none() {
        for (what, body) in [
            (
                "an earlier release",
                Box::new(|base: &str| latest_json(base, BRIDGE, "notes"))
                    as Box<dyn Fn(&str) -> String>,
            ),
            (
                "this release with other bytes",
                Box::new(|base: &str| latest_json(base, LATER, "other notes")),
            ),
            (
                "not a latest.json",
                Box::new(|_: &str| "<html>an error page</html>".to_string()),
            ),
            (
                "a version that is not a semantic one",
                Box::new(|_: &str| r#"{"version":"banana"}"#.to_string()),
            ),
            (
                "no version",
                Box::new(|_: &str| r#"{"notes":"none"}"#.to_string()),
            ),
        ] {
            let problems = found(LATER, |github, _| {
                feed(github, "feed-alpha", body(github.base()))
            });
            assert_eq!(problems, [DOES_NOT_SERVE], "{what}");
        }
    }

    #[test]
    fn holds_each_feed_of_a_stable_release_alone_the_later_one_is_accepted_the_earlier_is_not() {
        let world = after_publishing("3.0.1");
        let latest = world.files.text("latest.json");
        feed(&world.github, "feed-stable", &latest);
        assert_eq!(world.check(), Vec::<String>::new(), "both serve it");
        serving(&world.github, "feed-alpha", "3.1.0-alpha.1");
        assert_eq!(
            world.check(),
            Vec::<String>::new(),
            "the alpha line is ahead: that feed is left alone"
        );
        serving(&world.github, "feed-stable", "3.0.0");
        assert_eq!(
            world.check(),
            ["feed-stable does not serve this release's latest.json"]
        );
    }

    #[test]
    fn does_not_approve_a_feed_that_names_an_earlier_release_than_it_did_when_the_publisher_read_it(
    ) {
        let world = after_publishing(LATER);
        let record = |text: &str| fs::write(world.dir.path().join(FEEDS_BEFORE), text).unwrap();
        record(r#"{"feed-alpha":"3.0.0-alpha.83"}"#);
        assert_eq!(
            world.check(),
            [format!(
                "feed-alpha went backward: it named 3.0.0-alpha.83 when this release was published, and names {LATER} now"
            )]
        );
        for named in [
            format!("\"{LATER}\""),
            format!("\"{BRIDGE}\""),
            "null".to_string(),
            "\"banana\"".to_string(),
        ] {
            record(&format!("{{\"feed-alpha\":{named}}}"));
            assert_eq!(
                world.check(),
                Vec::<String>::new(),
                "it named {named}: it did not go back"
            );
        }
        record("{}");
        assert_eq!(
            world.check(),
            Vec::<String>::new(),
            "no record of this feed"
        );
        fs::remove_file(world.dir.path().join(FEEDS_BEFORE)).unwrap();
        assert_eq!(
            world.check(),
            Vec::<String>::new(),
            "no record at all: a check run by hand"
        );

        // A feed that does not serve this release is told once, as it was.
        record(r#"{"feed-alpha":"3.0.0-alpha.83"}"#);
        serving(&world.github, "feed-alpha", BRIDGE);
        assert_eq!(world.check(), [DOES_NOT_SERVE]);
    }

    #[test]
    fn holds_each_new_feed_to_what_it_named_before_the_stable_feed_went_backward_the_alpha_feed_did_not(
    ) {
        let world = after_publishing("3.0.1");
        feed(
            &world.github,
            "feed-stable",
            world.files.text("latest.json"),
        );
        fs::write(
            world.dir.path().join(FEEDS_BEFORE),
            r#"{"feed-alpha":"3.0.0","feed-stable":"3.0.2"}"#,
        )
        .unwrap();
        assert_eq!(
            world.check(),
            ["feed-stable went backward: it named 3.0.2 when this release was published, and names 3.0.1 now"]
        );
    }

    #[test]
    fn finds_a_latest_json_that_names_no_archive_of_this_release() {
        let world = after_publishing_with(LATER, |files| {
            let latest = with_archive_url(
                &files.text("latest.json"),
                "https://example.invalid/a.tar.gz",
            );
            files.with("latest.json", latest)
        });
        let problems = world.check();
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(
            problems[0].contains("names https://example.invalid/a.tar.gz for the Mac, not "),
            "{problems:?}"
        );
    }

    #[test]
    fn waits_out_a_feed_that_is_served_stale_for_a_moment() {
        let asked = Arc::new(AtomicUsize::new(0));
        let problems = found(LATER, |github, world| {
            let asked = Arc::clone(&asked);
            let stale = latest_json(github.base(), OLDER, "notes");
            let latest = world.files.text("latest.json");
            github.serve_with("/feed-alpha/latest.json", move || {
                let times = asked.fetch_add(1, Ordering::SeqCst) + 1;
                Some(Served::Body(
                    if times < 2 {
                        stale.clone()
                    } else {
                        latest.clone()
                    }
                    .into_bytes(),
                ))
            });
        });
        assert_eq!(problems, Vec::<String>::new());
        assert_eq!(
            asked.load(Ordering::SeqCst),
            2,
            "read until it served this release"
        );
    }

    #[test]
    fn lets_the_old_feed_of_a_channel_not_in_use_be_absent_and_finds_it_serving_this_release() {
        assert_eq!(
            found(LATER, |_, _| {}),
            Vec::<String>::new(),
            "update-stable is not there"
        );
        let serves = found(LATER, |github, world| {
            feed(github, "update-stable", world.files.text("latest.json"));
        });
        assert_eq!(
            serves,
            ["update-stable serves this release, which the rule does not move it to"]
        );
        let failing = found(LATER, |github, _| {
            github.serve("/update-stable/latest.json", Served::Status(503))
        });
        assert_eq!(failing, ["update-stable cannot be read (HTTP 503)"]);
        let junk = found(LATER, |github, _| feed(github, "update-stable", "oops"));
        assert_eq!(
            junk,
            ["update-stable serves something that is not a latest.json"]
        );
    }

    #[test]
    fn downloads_each_file_once_though_the_bridge_is_the_release_being_checked() {
        let world = after_publishing(BRIDGE);
        assert_eq!(world.check(), Vec::<String>::new());
        let path = download(BRIDGE, &archive(BRIDGE));
        let reads = world
            .github
            .requests()
            .iter()
            .filter(|asked| **asked == path)
            .count();
        assert_eq!(reads, 1);
    }

    #[test]
    fn refuses_a_release_before_the_bridge() {
        let world = after_publishing(LATER);
        let problems = check_feeds(
            world.dir.path(),
            &v(OLDER),
            world.github.base(),
            &rule(),
            quick(),
        )
        .unwrap();
        assert!(
            problems[0].contains("comes before the bridge"),
            "{problems:?}"
        );
    }
}

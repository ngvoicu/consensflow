//! A run that is run again after a later release was published: no feed is
//! ever moved backward. The job of a release run again from the Actions page
//! does what is left of it and leaves a feed that names the later release as
//! it is, and says so.

// A failure in a test is the test's answer, in its helpers too.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::fs;

use cf_publish::testing::worlds::{before, before_with, changes, did, feed, releasing};
use cf_publish::testing::{latest_json, Served, BRIDGE, LATER, NEWER, OLD_LAYOUT};
use serde_json::json;

const LATEST: &str = "latest.json";

#[test]
fn leaves_feed_alpha_at_alpha_82_when_alpha_81s_failed_publish_is_run_again_and_finishes_what_is_left(
) {
    let world = releasing();
    let github = &world.github;
    let bridge = world.latest(BRIDGE);
    // 1. alpha.81 reaches both feeds, and the edit that marks update-alpha as pinned fails.
    github.fail(|args, _| {
        (args[1] == "edit" && args[2] == "update-alpha").then(|| "the edit failed".to_string())
    });
    let cut = world.publish(BRIDGE).unwrap_err().to_string();
    assert!(
        cut.contains("marking update-alpha as pinned: the edit failed"),
        "{cut}"
    );
    assert_eq!(github.asset_text("feed-alpha", LATEST), bridge);
    assert_eq!(github.asset_text("update-alpha", LATEST), bridge);
    assert_eq!(
        github.title("update-alpha"),
        "update-alpha",
        "not marked as pinned"
    );

    // 2. alpha.82 finds the bridge delivered, and moves feed-alpha.
    github.clear_fail();
    let next = world.publish(LATER).unwrap();
    assert_eq!(did(&next), ["feed-alpha replaced"]);
    assert_eq!(github.asset_text("feed-alpha", LATEST), world.latest(LATER));

    // 3. alpha.81's publish is run again with its original artifacts.
    let asked = github.calls().len();
    let again = world.publish(BRIDGE).unwrap();
    assert_eq!(
        github.asset_text("feed-alpha", LATEST),
        world.latest(LATER),
        "feed-alpha still names alpha.82"
    );
    assert_eq!(again.version.as_str(), BRIDGE);
    assert_eq!(again.release.to_string(), "kept");
    assert_eq!(
        did(&again),
        [
            format!("feed-alpha superseded by {LATER}"),
            "update-alpha kept".to_string()
        ]
    );
    let said = format!(
        "feed-alpha names {LATER}, which comes after {BRIDGE}: it is left alone, and nothing is moved backward"
    );
    assert!(world.log().contains(&said), "{:?}", world.log());
    let done: Vec<Vec<String>> = changes(&github.calls()[asked..])
        .iter()
        .map(|args| args[..3].to_vec())
        .collect();
    assert_eq!(
        done,
        [["release", "edit", "update-alpha"]],
        "the only change is the pin that was left to finish"
    );
    assert_eq!(github.title("update-alpha"), "Alpha update feed, pinned");
    assert_eq!(
        github.asset_text("update-alpha", LATEST),
        bridge,
        "still the bridge"
    );

    // The check holds both to the rule: alpha.81 finds feed-alpha at a later release, alpha.82 finds it serving itself.
    assert_eq!(world.check(BRIDGE), Vec::<String>::new());
    assert_eq!(world.check(LATER), Vec::<String>::new());
    assert_eq!(
        world.recorded(BRIDGE),
        json!({ "feed-alpha": LATER }),
        "what it read before"
    );

    // What a publisher that moved feed-alpha back would leave: it serves what alpha.81 published, so only what the
    // feed named before says it went backward. The check does not approve it, and neither does the later release's.
    github.put("feed-alpha", LATEST, &bridge);
    assert_eq!(
        world.check(BRIDGE),
        [format!(
            "feed-alpha went backward: it named {LATER} when this release was published, and names {BRIDGE} now"
        )]
    );
    assert_eq!(
        world.check(LATER),
        ["feed-alpha does not serve this release's latest.json"]
    );
}

#[test]
fn leaves_the_check_a_record_of_what_each_new_feed_named_before_the_run_changed_it_and_none_of_the_old_feed(
) {
    let world = releasing();
    world.publish(BRIDGE).unwrap();
    assert_eq!(
        world.recorded(BRIDGE),
        json!({ "feed-alpha": null }),
        "no feed yet"
    );
    world.publish(LATER).unwrap();
    assert_eq!(world.recorded(LATER), json!({ "feed-alpha": BRIDGE }));
    world.publish(NEWER).unwrap();
    assert_eq!(world.recorded(NEWER), json!({ "feed-alpha": LATER }));
    world.publish(LATER).unwrap();
    assert_eq!(
        world.recorded(LATER),
        json!({ "feed-alpha": NEWER }),
        "as of the run again"
    );
    world.publish("3.0.1").unwrap();
    assert_eq!(
        world.recorded("3.0.1"),
        json!({ "feed-alpha": NEWER, "feed-stable": null })
    );
}

#[test]
fn writes_the_record_as_one_line_of_json_with_the_feeds_in_the_order_they_were_moved() {
    let world = releasing();
    world.publish(BRIDGE).unwrap();
    world.publish("3.0.1").unwrap();
    let dir = world.folder("3.0.1");
    assert_eq!(
        fs::read_to_string(dir.join("feeds-before.json")).unwrap(),
        format!("{{\"feed-alpha\":\"{BRIDGE}\",\"feed-stable\":null}}\n")
    );
}

#[test]
fn leaves_feed_alpha_at_alpha_83_when_alpha_82_is_run_again_which_has_nothing_left_to_do() {
    let world = releasing();
    let github = &world.github;
    world.publish(BRIDGE).unwrap();
    world.publish(LATER).unwrap();
    assert_eq!(
        world.check(LATER),
        Vec::<String>::new(),
        "the job that published it was right"
    );
    world.publish(NEWER).unwrap();
    let asked = github.calls().len();
    let again = world.publish(LATER).unwrap();
    assert_eq!(again.version.as_str(), LATER);
    assert_eq!(again.release.to_string(), "kept");
    assert_eq!(did(&again), [format!("feed-alpha superseded by {NEWER}")]);
    assert_eq!(github.asset_text("feed-alpha", LATEST), world.latest(NEWER));
    assert_eq!(
        changes(&github.calls()[asked..]),
        Vec::<Vec<String>>::new(),
        "nothing was changed"
    );
    let said = format!(
        "feed-alpha names {NEWER}, which comes after {LATER}: it is left alone, and nothing is moved backward"
    );
    assert!(world.log().contains(&said), "{:?}", world.log());
    assert_eq!(world.check(LATER), Vec::<String>::new());
    assert_eq!(world.check(NEWER), Vec::<String>::new());
}

#[test]
fn leaves_both_feeds_of_a_stable_release_that_two_later_ones_are_past_and_changes_nothing() {
    let world = releasing();
    let github = &world.github;
    world.publish(BRIDGE).unwrap();
    let first = world.publish("3.0.1").unwrap();
    assert_eq!(did(&first), ["feed-alpha replaced", "feed-stable created"]);
    world.publish("3.0.2").unwrap();
    let asked = github.calls().len();
    let again = world.publish("3.0.1").unwrap();
    assert_eq!(
        did(&again),
        [
            "feed-alpha superseded by 3.0.2",
            "feed-stable superseded by 3.0.2"
        ]
    );
    for feed in ["feed-alpha", "feed-stable"] {
        assert_eq!(
            github.asset_text(feed, LATEST),
            world.latest("3.0.2"),
            "{feed}"
        );
    }
    assert_eq!(changes(&github.calls()[asked..]), Vec::<Vec<String>>::new());
    assert_eq!(world.check("3.0.1"), Vec::<String>::new());
}

#[test]
fn moves_the_stable_feed_of_a_stable_fix_and_leaves_the_alpha_feed_alone_while_the_alpha_line_is_ahead(
) {
    let world = releasing();
    let github = &world.github;
    world.publish(BRIDGE).unwrap();
    world.publish("3.0.0").unwrap();
    world.publish("3.1.0-alpha.1").unwrap();
    assert_eq!(
        github.asset_text("feed-stable", LATEST),
        world.latest("3.0.0")
    );
    let fix = world.publish("3.0.1").unwrap();
    assert_eq!(
        did(&fix),
        [
            "feed-alpha superseded by 3.1.0-alpha.1",
            "feed-stable replaced"
        ]
    );
    assert_eq!(
        github.asset_text("feed-alpha", LATEST),
        world.latest("3.1.0-alpha.1")
    );
    assert_eq!(
        github.asset_text("feed-stable", LATEST),
        world.latest("3.0.1")
    );
    assert_eq!(world.check("3.0.1"), Vec::<String>::new());
}

#[test]
fn leaves_the_old_feed_alone_where_it_is_past_the_bridge_and_the_check_says_what_it_is_only_a_person_moves_a_pinned_feed(
) {
    let world = releasing();
    let github = &world.github;
    let past = latest_json(github.base(), "3.0.0-alpha.85", "notes");
    feed(github, "update-alpha", &past);
    let done = world.publish(BRIDGE).unwrap();
    assert_eq!(
        did(&done),
        [
            "feed-alpha created",
            "update-alpha superseded by 3.0.0-alpha.85"
        ]
    );
    assert_eq!(
        github.asset_text("update-alpha", LATEST),
        past,
        "not moved back"
    );
    let touched: Vec<Vec<String>> = changes(&github.calls())
        .into_iter()
        .filter(|args| args.iter().any(|arg| arg == "update-alpha") && args[1] != "edit")
        .collect();
    assert_eq!(touched, Vec::<Vec<String>>::new());
    assert_eq!(
        world.check(BRIDGE),
        [format!(
            "update-alpha serves 3.0.0-alpha.85, not the bridge {BRIDGE}: the apps that read it do not reach the bridge"
        )]
    );
}

#[test]
fn moves_a_feed_that_names_this_very_release_with_other_bytes_for_equal_is_not_later() {
    let world = before(BRIDGE);
    feed(
        &world.github,
        "feed-alpha",
        latest_json(world.github.base(), BRIDGE, "other notes"),
    );
    let done = world.publish().unwrap();
    assert_eq!(did(&done)[0], "feed-alpha replaced");
    assert_eq!(
        world.github.asset_text("feed-alpha", LATEST),
        world.files.text(LATEST)
    );
}

#[test]
fn moves_a_feed_whose_latest_json_is_not_a_latest_json_it_names_no_release() {
    moves_a_feed_that_names_no_release("<html>an error page</html>");
}

#[test]
fn moves_a_feed_whose_latest_json_is_json_of_no_release_it_names_no_release() {
    moves_a_feed_that_names_no_release("null");
}

#[test]
fn moves_a_feed_whose_latest_json_has_no_version_it_names_no_release() {
    moves_a_feed_that_names_no_release(r#"{"notes":"no version"}"#);
}

#[test]
fn moves_a_feed_whose_latest_json_names_a_version_that_is_not_a_semantic_one_it_names_no_release() {
    moves_a_feed_that_names_no_release(r#"{"version":"banana"}"#);
}

#[test]
fn moves_a_feed_whose_latest_json_names_a_version_that_is_not_a_string_it_names_no_release() {
    moves_a_feed_that_names_no_release(r#"{"version":7}"#);
}

fn moves_a_feed_that_names_no_release(body: &str) {
    let world = before(LATER);
    feed(&world.github, "feed-alpha", body);
    let done = world.publish().unwrap();
    assert_eq!(did(&done), ["feed-alpha replaced"]);
    assert_eq!(
        world.github.asset_text("feed-alpha", LATEST),
        world.files.text(LATEST)
    );
    let record = fs::read_to_string(world.dir.path().join("feeds-before.json")).unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&record).unwrap(),
        json!({ "feed-alpha": null }),
        "it named none"
    );
    assert_eq!(world.check(), Vec::<String>::new());
}

#[test]
fn changes_no_feed_it_cannot_read_which_may_name_a_later_release_and_says_so_run_again_it_finishes()
{
    for (instead, says) in [
        (
            Served::Status(503),
            format!(
                "feed-alpha cannot be read (HTTP 503): it is not known whether it names a release after {LATER}, and no feed is moved backward"
            ),
        ),
        (Served::Reset, "feed-alpha cannot be read (unreachable: ".to_string()),
    ] {
        let world = before_with(LATER, None, &OLD_LAYOUT);
        let github = &world.github;
        feed(github, "feed-alpha", latest_json(github.base(), BRIDGE, "notes"));
        github.serve("/feed-alpha/latest.json", instead);
        let refused = world.publish().unwrap_err().to_string();
        assert!(refused.contains(&says), "{refused}");
        let touched: Vec<Vec<String>> = changes(&github.calls())
            .into_iter()
            .filter(|args| args.iter().any(|arg| arg == "feed-alpha"))
            .collect();
        assert_eq!(touched, Vec::<Vec<String>>::new(), "feed-alpha was not touched");
        github.unserve("/feed-alpha/latest.json");
        let done = world.publish().unwrap();
        assert_eq!(did(&done), ["feed-alpha replaced"]);
        assert_eq!(github.asset_text("feed-alpha", LATEST), world.files.text(LATEST));
        assert_eq!(world.check(), Vec::<String>::new());
    }
}

#[test]
fn reads_a_feed_again_past_a_blip_and_swaps_a_latest_json_that_is_listed_and_not_served_at_all() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    let world = before(LATER);
    let github = &world.github;
    feed(
        github,
        "feed-alpha",
        latest_json(github.base(), BRIDGE, "notes"),
    );
    let asked = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&asked);
    github.serve_with("/feed-alpha/latest.json", move || {
        (counted.fetch_add(1, Ordering::SeqCst) + 1 < 2).then_some(Served::Status(503))
    });
    let done = world.publish().unwrap();
    assert_eq!(did(&done), ["feed-alpha replaced"]);
    assert_eq!(asked.load(Ordering::SeqCst), 2, "read again after the blip");

    let world = before(LATER);
    let github = &world.github;
    feed(
        github,
        "feed-alpha",
        latest_json(github.base(), BRIDGE, "notes"),
    );
    github.serve("/feed-alpha/latest.json", Served::Status(404));
    let done = world.publish().unwrap();
    assert_eq!(
        did(&done),
        ["feed-alpha replaced"],
        "a broken one is mended"
    );
    assert_eq!(
        github.asset_text("feed-alpha", LATEST),
        world.files.text(LATEST)
    );
}

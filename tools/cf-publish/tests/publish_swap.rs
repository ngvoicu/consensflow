//! Replacing a feed's latest.json: the new one is uploaded beside the old and
//! the names swapped by renames, so the feed never lacks the file for an
//! upload; every state a swap cut short can be left in is settled by running
//! again.

// A failure in a test is the test's answer, in its helpers too.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use cf_publish::testing::worlds::{before, did, gap, Before};
use cf_publish::testing::{latest_json, Spec, BRIDGE, OLDER};

const LATEST: &str = "latest.json";
const NEXT: &str = "latest.next.json";
const PREVIOUS: &str = "latest.previous.json";

/// The latest.json the feed served before the bridge.
fn old() -> String {
    latest_json("http://example.test", OLDER, "notes")
}

/// The bridge, about to be published, against an update-alpha that holds `assets`.
fn replacing(assets: &[(&str, &str)]) -> Before {
    let world = before(BRIDGE);
    world.github.release(
        "update-alpha",
        Spec::new().prerelease().assets(assets.iter().copied()),
    );
    world
}

fn holds(world: &Before) -> Vec<String> {
    world.github.names("update-alpha").unwrap()
}

fn serves(world: &Before) -> Option<String> {
    world
        .github
        .asset("update-alpha", LATEST)
        .map(|data| String::from_utf8_lossy(&data).into_owned())
}

/// A call in a few words: what it did, to what.
fn brief(args: &[String]) -> String {
    if args[0] == "api" {
        let name = args.last().map(String::as_str).unwrap_or_default();
        return format!("rename to {}", name.strip_prefix("name=").unwrap_or(name));
    }
    let named = args
        .iter()
        .find(|arg| arg.starts_with("latest"))
        .map(String::as_str);
    [
        Some(args[1].as_str()),
        args.get(2).map(String::as_str),
        named,
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" ")
}

fn args_include(args: &[String], word: &str) -> bool {
    args.iter().any(|arg| arg == word)
}

#[test]
fn uploads_the_new_one_beside_the_old_and_swaps_them_by_rename_deleting_nothing_live() {
    let old = old();
    let world = replacing(&[(LATEST, &old)]);
    world.publish().unwrap();
    let calls = world.github.calls();
    let start = calls
        .iter()
        .position(|args| args[1] == "view" && args[2] == "update-alpha")
        .unwrap();
    let briefs: Vec<String> = calls[start..].iter().map(|args| brief(args)).collect();
    assert_eq!(
        briefs,
        [
            "view update-alpha".to_string(),
            format!("upload update-alpha {NEXT}"),
            "view update-alpha".to_string(),
            format!("rename to {PREVIOUS}"),
            format!("rename to {LATEST}"),
            format!("delete-asset update-alpha {PREVIOUS}"),
            "edit update-alpha".to_string(),
        ]
    );
    assert_eq!(holds(&world), [LATEST]);
    assert_eq!(serves(&world).unwrap(), world.files.text(LATEST));
}

#[test]
fn leaves_the_old_latest_json_where_it_was_when_the_upload_of_the_new_one_fails() {
    let old = old();
    let world = replacing(&[(LATEST, &old)]);
    world.github.fail(|args, _| {
        (args[1] == "upload" && args_include(args, NEXT)).then(|| "the upload broke".to_string())
    });
    let refused = world.publish().unwrap_err().to_string();
    assert!(
        refused.contains("uploading the new latest.json to update-alpha: the upload broke"),
        "{refused}"
    );
    assert_eq!(serves(&world).unwrap(), old);
    assert_eq!(
        gap(&world.github, "update-alpha", true),
        0,
        "never without it"
    );
    assert_eq!(holds(&world), [LATEST]);
}

#[test]
fn leaves_it_where_it_was_when_the_first_rename_fails_and_clears_what_it_uploaded() {
    let old = old();
    let world = replacing(&[(LATEST, &old)]);
    world.github.fail(|args, _| {
        (args[0] == "api" && args_include(args, &format!("name={PREVIOUS}")))
            .then(|| "the rename broke".to_string())
    });
    let refused = world.publish().unwrap_err().to_string();
    assert!(
        refused.contains("renaming latest.json to latest.previous.json: the rename broke"),
        "{refused}"
    );
    assert_eq!(serves(&world).unwrap(), old);
    assert_eq!(gap(&world.github, "update-alpha", true), 0);
    assert_eq!(holds(&world), [LATEST], "the new file is cleared");
}

#[test]
fn says_what_is_missing_when_the_live_latest_json_is_gone_before_it_can_be_set_aside() {
    let old = old();
    let world = replacing(&[(LATEST, &old)]);
    // Somebody deletes it between the upload of the new one and the first rename.
    let handle = world.github.handle();
    let mut views = 0;
    world.github.fail(move |args, _| {
        if args[1] == "view" && args[2] == "update-alpha" {
            views += 1;
            if views == 2 {
                handle.remove("update-alpha", LATEST);
            }
        }
        None
    });
    let refused = world.publish().unwrap_err().to_string();
    assert!(
        refused.contains("there is no file to rename to latest.previous.json"),
        "{refused}"
    );
    assert_eq!(
        holds(&world),
        Vec::<String>::new(),
        "the new file is cleared, and nothing else was touched"
    );
}

#[test]
fn puts_the_old_latest_json_back_when_the_second_rename_fails() {
    let old = old();
    let world = replacing(&[(LATEST, &old)]);
    let mut failed = false;
    world.github.fail(move |args, _| {
        if args[0] == "api" && args_include(args, &format!("name={LATEST}")) && !failed {
            failed = true;
            return Some("the rename broke".to_string());
        }
        None
    });
    let refused = world.publish().unwrap_err().to_string();
    assert!(
        refused.contains("renaming latest.next.json to latest.json: the rename broke"),
        "{refused}"
    );
    assert_eq!(serves(&world).unwrap(), old, "the feed serves what it did");
    assert_eq!(
        gap(&world.github, "update-alpha", true),
        2,
        "lacking it for the failed call, and until the one that puts it back"
    );
    assert_eq!(holds(&world), [LATEST, NEXT], "the new one waits beside it");
}

#[test]
fn says_the_feed_lacks_its_latest_json_when_the_old_one_cannot_be_put_back_either_and_a_rerun_mends_it(
) {
    let old = old();
    let world = replacing(&[(LATEST, &old)]);
    world.github.fail(|args, _| {
        (args[0] == "api" && args_include(args, &format!("name={LATEST}")))
            .then(|| "the network is gone".to_string())
    });
    let refused = world.publish().unwrap_err().to_string();
    assert!(
        refused.contains("the network is gone; and update-alpha lacks its latest.json until this is run again, for putting latest.previous.json back failed: ")
            && refused.ends_with("the network is gone"),
        "{refused}"
    );
    assert_eq!(holds(&world), [NEXT, PREVIOUS]);
    world.github.clear_fail();
    let done = world.publish().unwrap();
    assert_eq!(did(&done)[1], "update-alpha replaced");
    assert_eq!(holds(&world), [LATEST]);
    assert_eq!(serves(&world).unwrap(), world.files.text(LATEST));
    assert_eq!(world.check(), Vec::<String>::new());
}

#[test]
fn does_not_fail_the_release_for_a_previous_latest_json_it_could_not_delete_and_the_next_run_clears_it(
) {
    let old = old();
    let world = replacing(&[(LATEST, &old)]);
    world.github.fail(|args, _| {
        (args[1] == "delete-asset" && args_include(args, PREVIOUS))
            .then(|| "could not delete".to_string())
    });
    world.publish().unwrap();
    assert_eq!(holds(&world), [LATEST, PREVIOUS]);
    assert!(
        world
            .log()
            .iter()
            .any(|line| line.contains(&format!("{PREVIOUS} of update-alpha was not cleared"))),
        "{:?}",
        world.log()
    );
    world.github.clear_fail();
    world.publish().unwrap();
    assert_eq!(holds(&world), [LATEST]);
}

/// A swap cut short, left as `assets` says: running it again settles it, and moves the feed.
fn settles(assets: &[(&str, &str)]) {
    let world = replacing(assets);
    let done = world.publish().unwrap();
    let update = did(&done)[1].clone();
    assert!(
        update == "update-alpha replaced" || update == "update-alpha uploaded",
        "{update}"
    );
    assert_eq!(holds(&world), [LATEST], "nothing is left beside it");
    assert_eq!(serves(&world).unwrap(), world.files.text(LATEST));
    assert_eq!(world.check(), Vec::<String>::new());
}

#[test]
fn settles_an_upload_of_the_new_one_that_was_cut_short_and_moves_the_feed() {
    let old = old();
    settles(&[(LATEST, &old), (NEXT, "half")]);
}

#[test]
fn settles_the_old_one_set_aside_and_the_new_one_not_yet_in_place_and_moves_the_feed() {
    let old = old();
    settles(&[(PREVIOUS, &old), (NEXT, "new")]);
}

#[test]
fn settles_the_new_one_in_place_and_the_old_one_not_yet_deleted_and_moves_the_feed() {
    let old = old();
    settles(&[(PREVIOUS, &old), (LATEST, "new")]);
}

#[test]
fn settles_the_old_one_set_aside_and_nothing_else_and_moves_the_feed() {
    let old = old();
    settles(&[(PREVIOUS, &old)]);
}

#[test]
fn settles_the_new_one_beside_nothing_the_feed_lost_its_latest_json_and_moves_the_feed() {
    settles(&[(NEXT, "new")]);
}

#[test]
fn settles_a_feed_with_no_files_at_all_and_moves_the_feed() {
    settles(&[]);
}

#[test]
fn puts_the_previous_latest_json_back_first_when_the_feed_has_none_so_it_is_served_again_at_once() {
    let old = old();
    let world = replacing(&[(PREVIOUS, &old), (NEXT, "new")]);
    world.publish().unwrap();
    let first = world
        .github
        .calls()
        .into_iter()
        .find(|args| args[0] == "api")
        .unwrap();
    assert_eq!(
        first.last().unwrap(),
        &format!("name={LATEST}"),
        "the first change to the feed puts the old one back"
    );
}

#[test]
fn leaves_a_feed_that_already_serves_this_latest_json_untouched() {
    let world = before(BRIDGE);
    world.github.release(
        "update-alpha",
        Spec::new()
            .prerelease()
            .assets([(LATEST, world.files.text(LATEST))]),
    );
    let done = world.publish().unwrap();
    assert_eq!(did(&done)[1], "update-alpha kept");
    let touched: Vec<Vec<String>> = world
        .github
        .calls()
        .into_iter()
        .filter(|args| args_include(args, "update-alpha") && args[1] == "upload")
        .collect();
    assert_eq!(touched, Vec::<Vec<String>>::new());
}

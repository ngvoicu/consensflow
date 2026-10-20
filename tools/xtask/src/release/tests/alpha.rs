//! `release alpha`, run on a checkout with real remotes and GitHub as a script.

use super::fake::{git, Fake, World};
use super::refusal;
use crate::release::{alpha, Error, Options};

const ALPHA: &str = "3.0.6-alpha.1";

fn options(world: &World, dry_run: bool) -> Options {
    Options {
        version: None,
        notes: world.notes.clone(),
        dry_run,
    }
}

/// A checkout at the alpha, CI green on its main, and the feeds at 3.0.5.
fn ready() -> (World, Fake) {
    let world = World::new(ALPHA);
    let fake = Fake::with_ci_on(&world, &world.head(), "3.0.5", "3.0.5");
    (world, fake)
}

fn released(world: &World, fake: &mut Fake, dry_run: bool) -> (Result<String, Error>, String) {
    world.run(fake, alpha::release, &options(world, dry_run))
}

#[test]
fn an_alpha_is_tagged_with_its_notes_watched_and_served_by_feed_alpha_alone() {
    let (world, mut fake) = ready();
    // Runs show a look late and end after two, and the feed serves the
    // release on the third read: each is waited for.
    fake.running_for = 2;
    fake.unlisted_for = 1;
    fake.feed_lag = 2;
    let stable = fake.feeds["feed-stable"].clone();
    let head = world.head();
    let (outcome, said) = released(&world, &mut fake, false);
    assert_eq!(
        outcome.unwrap(),
        format!(
            "released {ALPHA} on {}: feed-alpha serves it, feed-stable is as it was\n",
            &head[..8]
        )
    );
    let tag = format!("v{ALPHA}");
    for remote in [&world.origin, &world.upstream] {
        assert_eq!(World::tagged(remote, &tag), Some(head.clone()));
    }
    // An annotated tag, whose message is the notes.
    assert_eq!(git(&world.root, &["cat-file", "-t", &tag]), "tag");
    assert_eq!(
        git(&world.root, &["tag", "-l", "--format=%(contents)", &tag]),
        "The title of the release\n\nWhat changed, for the people updating."
    );
    assert_eq!(fake.serves("feed-alpha").as_deref(), Some(ALPHA));
    assert_eq!(fake.feeds["feed-stable"], stable);
    // CI, then the dry run on main, then the tag to origin before upstream, then the release.
    let ci = fake.when("gh run view 1000").unwrap();
    let dry = fake
        .when("gh workflow run release.yml --repo ngvoicu/consensflow --ref main")
        .unwrap();
    let origin = fake.when(&format!("git push -q origin {tag}")).unwrap();
    let upstream = fake.when(&format!("git push -q upstream {tag}")).unwrap();
    assert!(
        ci < dry && dry < origin && origin < upstream,
        "{:#?}",
        fake.lines()
    );
    assert_eq!(fake.runs_of("release.yml", "workflow_dispatch").len(), 1);
    assert_eq!(fake.runs_of("release.yml", "push").len(), 1);
    assert!(fake.pauses > 0);
    for line in [
        "ci run 1000: success",
        "  gate (windows-latest) success",
        "dry run run 1001: success",
        "release run 1002: success",
        "feed-alpha serves 3.0.6-alpha.1",
    ] {
        assert!(said.contains(line), "{line}: {said}");
    }
}

#[test]
fn a_dry_run_does_everything_up_to_the_tag_and_tags_nothing() {
    let (world, mut fake) = ready();
    let head = world.head();
    let (outcome, _) = released(&world, &mut fake, true);
    let done = outcome.unwrap();
    assert!(
        done.starts_with(&format!(
            "dry run: every check passed; v{ALPHA} would be tagged on {}",
            &head[..8]
        )),
        "{done}"
    );
    assert_eq!(fake.runs_of("release.yml", "workflow_dispatch").len(), 1);
    assert!(
        fake.lines()
            .iter()
            .all(|line| !line.starts_with("git push") && !line.starts_with("git tag")),
        "{:#?}",
        fake.lines()
    );
    for remote in [&world.root, &world.origin, &world.upstream] {
        assert_eq!(World::tagged(remote, &format!("v{ALPHA}")), None);
    }
    assert_eq!(fake.serves("feed-alpha").as_deref(), Some("3.0.5"));
}

#[test]
fn sources_whose_version_is_not_an_alpha_are_refused_before_anything_runs() {
    for version in ["3.0.6", "3.0.6-rc.1", "3.0.6-alpha"] {
        let world = World::new(version);
        let mut fake = Fake::with_ci_on(&world, &world.head(), "3.0.5", "3.0.5");
        let said = refusal(released(&world, &mut fake, false).0);
        assert_eq!(
            said,
            format!("{version} is not an alpha: an alpha is X.Y.Z-alpha.N")
        );
        assert!(fake.ran.is_empty(), "{:#?}", fake.lines());
    }
}

#[test]
fn a_checkout_with_changes_is_refused() {
    let (world, mut fake) = ready();
    super::fake::write(&world.root.join("README.md"), "an edit not committed\n");
    let said = refusal(released(&world, &mut fake, true).0);
    assert_eq!(
        said,
        "the checkout has changes: a release is made from main as it was committed"
    );
}

#[test]
fn a_checkout_on_another_branch_is_refused() {
    let (world, mut fake) = ready();
    git(&world.root, &["checkout", "-q", "-b", "wt/elsewhere"]);
    let said = refusal(released(&world, &mut fake, true).0);
    assert_eq!(
        said,
        "the checkout is on wt/elsewhere, not main: a release is made from main"
    );
}

#[test]
fn main_that_is_not_upstreams_is_refused() {
    let (world, mut fake) = ready();
    super::fake::write(&world.root.join("README.md"), "a commit not pushed\n");
    world.commit("not pushed");
    let said = refusal(released(&world, &mut fake, true).0);
    assert!(said.contains("push or pull until they are one"), "{said}");
}

#[test]
fn a_version_tagged_in_the_checkout_on_origin_or_on_upstream_is_refused() {
    let tag = format!("v{ALPHA}");
    for (place, said) in [
        (
            "checkout",
            format!("{tag} is already a tag in the checkout: that version was released, or begun"),
        ),
        (
            "origin",
            format!("{tag} is already a tag on origin: that version was released, or begun"),
        ),
        (
            "upstream",
            format!("{tag} is already a tag on upstream: that version was released, or begun"),
        ),
    ] {
        let (world, mut fake) = ready();
        git(&world.root, &["tag", &tag]);
        if place != "checkout" {
            git(&world.root, &["push", "-q", place, &tag]);
            git(&world.root, &["tag", "-d", &tag]);
        }
        assert_eq!(
            refusal(released(&world, &mut fake, true).0),
            said,
            "{place}"
        );
    }
}

#[test]
fn a_version_feed_alpha_does_not_come_before_is_refused_before_ci_is_asked() {
    for (serves, said) in [
        (Some(ALPHA), format!("feed-alpha serves {ALPHA}, which {ALPHA} does not come after: set a later version")),
        (Some("3.0.6"), format!("feed-alpha serves 3.0.6, which {ALPHA} does not come after: set a later version")),
        (Some("3.0.6-alpha.2"), format!("feed-alpha serves 3.0.6-alpha.2, which {ALPHA} does not come after: set a later version")),
        (None, format!("feed-alpha serves no release, which {ALPHA} does not come after: set a later version")),
    ] {
        let (world, mut fake) = ready();
        match serves {
            Some(version) => {
                fake.feeds.insert("feed-alpha".into(), super::fake::latest(version));
            }
            None => {
                fake.feeds.remove("feed-alpha");
            }
        }
        assert_eq!(refusal(released(&world, &mut fake, true).0), said);
        assert!(fake.when("gh").is_none(), "{:#?}", fake.lines());
    }
}

#[test]
fn notes_without_a_title_are_refused_before_the_checkout_is_asked() {
    let (world, mut fake) = ready();
    super::fake::write(&world.notes, "\nno title\n");
    let said = refusal(released(&world, &mut fake, true).0);
    assert!(said.contains("have no title"), "{said}");
    assert!(fake.ran.is_empty());
}

#[test]
fn ci_that_is_not_green_on_both_platforms_stops_it_before_the_dry_run() {
    // CI failed.
    let world = World::new(ALPHA);
    let mut fake = Fake::new(&world, "3.0.5", "3.0.5");
    fake.conclusions.insert(("ci.yml", "push"), "failure");
    fake.start("ci.yml", &world.head(), "push");
    let said = refusal(released(&world, &mut fake, true).0);
    assert_eq!(said, "ci run 1000 ended failure: nothing after it was done");
    assert!(fake.when("gh workflow run").is_none());

    // CI passed on one platform alone.
    let world = World::new(ALPHA);
    let mut fake = Fake::new(&world, "3.0.5", "3.0.5");
    fake.ci_jobs = vec![
        ("gate (macos-latest)", "success"),
        ("gate (windows-latest)", "skipped"),
    ];
    fake.start("ci.yml", &world.head(), "push");
    let said = refusal(released(&world, &mut fake, true).0);
    assert_eq!(
        said,
        "ci run 1000 holds no green windows gate: both platforms must pass"
    );
    assert!(fake.when("gh workflow run").is_none());

    // No CI on main's commit at all.
    let (world, _) = ready();
    let mut fake = Fake::new(&world, "3.0.5", "3.0.5");
    let said = refusal(released(&world, &mut fake, true).0);
    assert_eq!(
        said,
        format!(
            "ci: no new run of ci.yml appeared on {}",
            &world.head()[..8]
        )
    );
    assert_eq!(fake.pauses, 5);
}

#[test]
fn a_dry_run_that_fails_stops_it_before_the_tag() {
    let (world, mut fake) = ready();
    fake.conclusions
        .insert(("release.yml", "workflow_dispatch"), "failure");
    let said = refusal(released(&world, &mut fake, false).0);
    assert_eq!(
        said,
        "dry run run 1001 ended failure: nothing after it was done"
    );
    assert!(fake.when("git tag").is_none());
    assert_eq!(World::tagged(&world.upstream, &format!("v{ALPHA}")), None);
}

#[test]
fn an_older_dry_run_of_the_same_commit_is_not_taken_for_the_new_one() {
    let (world, mut fake) = ready();
    // A green dry run of this commit from before; the one this run asks for fails.
    fake.start("release.yml", &world.head(), "workflow_dispatch");
    fake.conclusions
        .insert(("release.yml", "workflow_dispatch"), "failure");
    // The new one is listed a look after it was asked for, as GitHub lists
    // them: until then the old one is the newest.
    fake.unlisted_for = 1;
    let said = refusal(released(&world, &mut fake, false).0);
    assert_eq!(
        said,
        "dry run run 1002 ended failure: nothing after it was done"
    );
}

#[test]
fn a_release_run_that_fails_is_the_answer() {
    let (world, mut fake) = ready();
    fake.conclusions.insert(("release.yml", "push"), "failure");
    let said = refusal(released(&world, &mut fake, false).0);
    assert_eq!(
        said,
        "release run 1002 ended failure: nothing after it was done"
    );
    assert_eq!(fake.serves("feed-alpha").as_deref(), Some("3.0.5"));
}

#[test]
fn a_feed_alpha_that_never_serves_the_alpha_fails_the_release() {
    let (world, mut fake) = ready();
    fake.publishes = false;
    let said = refusal(released(&world, &mut fake, false).0);
    assert_eq!(
        said,
        format!("feed-alpha does not serve {ALPHA}: it serves 3.0.5")
    );
    // One look at each of the three runs while it goes on, and a wait between each two of five reads.
    assert_eq!(fake.pauses, 3 + 4);
}

#[test]
fn a_feed_stable_the_alpha_moved_fails_the_release() {
    let (world, mut fake) = ready();
    fake.alpha_moves_stable = true;
    let said = refusal(released(&world, &mut fake, false).0);
    assert_eq!(
        said,
        format!("feed-stable changed while {ALPHA} was released: an alpha never moves it")
    );
}

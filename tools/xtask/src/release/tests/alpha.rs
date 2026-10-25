//! `release alpha`, run on a checkout with real remotes and GitHub as a script.

use super::fake::{git, Fake, World};
use super::{advice, not_on_upstream, refusal};
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
    // CI's push run, then the tag to origin before upstream, then the release.
    let ci = fake.when("gh run view 1000").unwrap();
    let origin = fake.when(&format!("git push -q origin {tag}")).unwrap();
    let upstream = fake.when(&format!("git push -q upstream {tag}")).unwrap();
    assert!(ci < origin && origin < upstream, "{:#?}", fake.lines());
    assert!(
        fake.lines()
            .iter()
            .any(|line| line.starts_with("gh run list")
                && line.contains("--workflow ci.yml")
                && line.contains("--event push")),
        "{:#?}",
        fake.lines()
    );
    // CI is the gate: no Release dry run is started, and the tag's run is the one Release run.
    assert!(
        fake.when("gh workflow run").is_none(),
        "{:#?}",
        fake.lines()
    );
    assert!(fake.runs_of("release.yml", "workflow_dispatch").is_empty());
    assert_eq!(fake.runs_of("release.yml", "push").len(), 1);
    assert!(fake.pauses > 0);
    for line in [
        "ci run 1000: success",
        "  gate (windows-latest) success",
        "release run 1001: success",
        "feed-alpha serves 3.0.6-alpha.1",
    ] {
        assert!(said.contains(line), "{line}: {said}");
    }
}

#[test]
fn a_dry_run_watches_ci_and_starts_and_tags_nothing() {
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
    assert!(
        fake.when("gh run view 1000").is_some(),
        "{:#?}",
        fake.lines()
    );
    assert!(fake.runs_of("release.yml", "workflow_dispatch").is_empty());
    assert!(
        fake.lines().iter().all(|line| !line.starts_with("git push")
            && !line.starts_with("git tag")
            && !line.starts_with("gh workflow run")),
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
fn a_tag_in_the_checkout_or_on_origin_alone_is_refused_with_how_to_push_or_delete_it() {
    let tag = format!("v{ALPHA}");
    for (places, place, undo) in [
        (
            &["checkout"][..],
            "in the checkout",
            format!("git tag -d {tag}"),
        ),
        (
            &["origin"][..],
            "on origin",
            format!("git push origin --delete {tag}"),
        ),
        (
            &["checkout", "origin"][..],
            "in the checkout and on origin",
            format!("git tag -d {tag} && git push origin --delete {tag}"),
        ),
    ] {
        let (world, mut fake) = ready();
        git(&world.root, &["tag", &tag]);
        if places.contains(&"origin") {
            git(&world.root, &["push", "-q", "origin", &tag]);
        }
        if !places.contains(&"checkout") {
            git(&world.root, &["tag", "-d", &tag]);
        }
        assert_eq!(
            refusal(released(&world, &mut fake, true).0),
            not_on_upstream(&tag, place, &undo),
            "{place}"
        );
    }
}

#[test]
fn a_tag_on_upstream_is_refused_with_its_release_run_to_rerun() {
    let tag = format!("v{ALPHA}");
    // A light tag, and no run of it that GitHub lists.
    let (world, mut fake) = ready();
    git(&world.root, &["tag", &tag]);
    git(&world.root, &["push", "-q", "origin", &tag]);
    git(&world.root, &["push", "-q", "upstream", &tag]);
    let head = world.head();
    assert_eq!(
        refusal(released(&world, &mut fake, true).0),
        format!(
            "{tag} is already on upstream, on {}, so its Release run started; {}",
            &head[..8],
            advice(&tag, None)
        )
    );
    // An annotated tag, found by the commit it names, and its run.
    let (world, mut fake) = ready();
    world.released(ALPHA);
    let run = fake.start("release.yml", &world.head(), "push");
    assert_eq!(
        refusal(released(&world, &mut fake, true).0),
        format!(
            "{tag} is already on upstream, on {}, so its Release run started; {}",
            &world.head()[..8],
            advice(&tag, Some(run))
        )
    );
}

#[test]
fn a_version_feed_alpha_does_not_come_before_is_refused_before_ci_is_asked() {
    for (serves, said) in [
        (ALPHA, format!("feed-alpha serves {ALPHA}, which {ALPHA} does not come after: set a later version")),
        ("3.0.6", format!("feed-alpha serves 3.0.6, which {ALPHA} does not come after: set a later version")),
        ("3.0.6-alpha.2", format!("feed-alpha serves 3.0.6-alpha.2, which {ALPHA} does not come after: set a later version")),
    ] {
        let (world, mut fake) = ready();
        fake.feeds.insert("feed-alpha".into(), super::fake::latest(serves));
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

/// Whether nothing was tagged or pushed, in the checkout or on a remote.
fn nothing_tagged(world: &World, fake: &Fake) {
    assert!(
        fake.when("git tag").is_none() && fake.when("git push").is_none(),
        "{:#?}",
        fake.lines()
    );
    for remote in [&world.root, &world.origin, &world.upstream] {
        assert_eq!(World::tagged(remote, &format!("v{ALPHA}")), None);
    }
    assert!(fake.runs_of("release.yml", "push").is_empty());
}

#[test]
fn ci_that_ended_red_is_refused_and_nothing_is_tagged() {
    for ended in ["failure", "cancelled"] {
        let world = World::new(ALPHA);
        let mut fake = Fake::new(&world, "3.0.5", "3.0.5");
        fake.conclusions.insert(("ci.yml", "push"), ended);
        fake.start("ci.yml", &world.head(), "push");
        let said = refusal(released(&world, &mut fake, false).0);
        assert_eq!(
            said,
            format!("ci run 1000 ended {ended}: nothing after it was done")
        );
        nothing_tagged(&world, &fake);
    }
}

#[test]
fn ci_without_both_platforms_summaries_green_is_refused_and_nothing_is_tagged() {
    for (jobs, missing) in [
        // A summary that did not pass, though the run says success.
        (
            vec![
                ("gate (macos-latest)", "success"),
                ("gate (windows-latest)", "skipped"),
            ],
            "gate (windows-latest)",
        ),
        // A platform's jobs green but no summary: a job that holds its name is not it.
        (
            vec![
                ("gate (macos-latest)", "success"),
                ("test (windows-latest, rig)", "success"),
            ],
            "gate (windows-latest)",
        ),
        (
            vec![("gate (windows-latest)", "success")],
            "gate (macos-latest)",
        ),
    ] {
        let world = World::new(ALPHA);
        let mut fake = Fake::new(&world, "3.0.5", "3.0.5");
        fake.ci_jobs = jobs;
        fake.start("ci.yml", &world.head(), "push");
        let said = refusal(released(&world, &mut fake, false).0);
        assert_eq!(
            said,
            format!("ci run 1000 holds no green {missing}: both platforms must pass; nothing after it was done")
        );
        nothing_tagged(&world, &fake);
    }
}

#[test]
fn a_commit_ci_never_ran_on_is_refused_and_nothing_is_tagged() {
    let (world, _) = ready();
    let mut fake = Fake::new(&world, "3.0.5", "3.0.5");
    let said = refusal(released(&world, &mut fake, false).0);
    assert_eq!(
        said,
        format!(
            "ci: no new run of ci.yml appeared on {}: nothing after it was done",
            &world.head()[..8]
        )
    );
    assert_eq!(fake.pauses, 5);
    nothing_tagged(&world, &fake);
}

#[test]
fn ci_green_on_another_commit_is_not_main_s_and_nothing_is_tagged() {
    // CI passed main's commit before the last; main's own has none.
    let world = World::new(ALPHA);
    let earlier = world.head();
    super::fake::write(
        &world.root.join("README.md"),
        "a landing CI has not run on
",
    );
    world.commit("a landing");
    for remote in ["origin", "upstream"] {
        git(&world.root, &["push", "-q", remote, "main"]);
    }
    let mut fake = Fake::with_ci_on(&world, &earlier, "3.0.5", "3.0.5");
    let said = refusal(released(&world, &mut fake, false).0);
    assert_eq!(
        said,
        format!(
            "ci: no new run of ci.yml appeared on {}: nothing after it was done",
            &world.head()[..8]
        )
    );
    nothing_tagged(&world, &fake);
}

#[test]
fn ci_of_a_pull_request_on_main_s_commit_is_not_its_gate() {
    // A pull request's run tests a merge of the commit, not it: only a push's counts.
    let (world, _) = ready();
    let mut fake = Fake::new(&world, "3.0.5", "3.0.5");
    fake.start("ci.yml", &world.head(), "pull_request");
    let said = refusal(released(&world, &mut fake, false).0);
    assert!(
        said.starts_with("ci: no new run of ci.yml appeared on "),
        "{said}"
    );
    nothing_tagged(&world, &fake);
}

#[test]
fn a_release_run_that_fails_says_to_rerun_it_and_a_run_again_says_the_same() {
    let (world, mut fake) = ready();
    fake.conclusions.insert(("release.yml", "push"), "failure");
    let tag = format!("v{ALPHA}");
    let said = refusal(released(&world, &mut fake, false).0);
    assert_eq!(
        said,
        format!(
            "release run 1001 ended failure: {tag} stays on upstream; {}",
            advice(&tag, Some(1001))
        )
    );
    assert_eq!(fake.serves("feed-alpha").as_deref(), Some("3.0.5"));
    // Run again, it refuses the tag, and names the same run.
    let said = refusal(released(&world, &mut fake, false).0);
    assert_eq!(
        said,
        format!(
            "{tag} is already on upstream, on {}, so its Release run started; {}",
            &world.head()[..8],
            advice(&tag, Some(1001))
        )
    );
}

#[test]
fn an_older_release_run_of_the_same_commit_is_not_taken_for_the_new_one() {
    let (world, mut fake) = ready();
    // A green release run of this commit from before (another tag on it); the new one fails.
    fake.start("release.yml", &world.head(), "push");
    fake.conclusions.insert(("release.yml", "push"), "failure");
    fake.unlisted_for = 1;
    let said = refusal(released(&world, &mut fake, false).0);
    assert!(said.starts_with("release run 1002 ended failure"), "{said}");
}

#[test]
fn a_feed_alpha_that_never_serves_the_alpha_after_a_green_run_says_the_release_is_published() {
    let (world, mut fake) = ready();
    fake.publishes = false;
    let said = refusal(released(&world, &mut fake, false).0);
    assert_eq!(
        said,
        format!(
            "release run 1001 ended green, its own check of the feeds with it, yet feed-alpha does not serve \
             {ALPHA} after 5 reads (it serves 3.0.5): the release is published, and GitHub's download links \
             can lag; read it again in a few minutes, \
             `curl -sL https://github.com/ngvoicu/consensflow/releases/download/feed-alpha/latest.json`, \
             and run nothing again"
        )
    );
    // One look at each of the two runs while it goes on, and a wait between each two of five reads.
    assert_eq!(fake.pauses, 2 + 4);
}

#[test]
fn a_feed_stable_the_alpha_moved_fails_the_release() {
    let (world, mut fake) = ready();
    fake.alpha_moves_stable = true;
    let said = refusal(released(&world, &mut fake, false).0);
    assert_eq!(
        said,
        format!(
            "feed-stable serves {ALPHA}, and served 3.0.5 before {ALPHA} was released: \
             an alpha never moves it; read the publish job of run 1001"
        )
    );
}

#[test]
fn a_feed_stable_that_answers_no_release_after_a_green_run_says_the_release_is_published() {
    for (code, last) in [
        (6, "curl ended 6: curl: (6) the read failed"),
        (22, "it serves no release"),
    ] {
        let (world, mut fake) = ready();
        fake.failing_after_the_tag = vec![("feed-stable", code); 5];
        let said = refusal(released(&world, &mut fake, false).0);
        assert_eq!(
            said,
            format!(
                "feed-stable answered no release in 5 reads ({last}): an outage of the feed or of GitHub, \
                 not a version to change; release run 1001 ended green, so the release is published; \
                 read feed-stable by hand, \
                 `curl -sL https://github.com/ngvoicu/consensflow/releases/download/feed-stable/latest.json`, \
                 which served 3.0.5 before it, and run nothing again"
            )
        );
        assert_eq!(fake.serves("feed-alpha").as_deref(), Some(ALPHA));
    }
}

#[test]
fn a_feed_that_answers_404_or_no_network_for_a_while_is_read_again_and_is_no_alarm() {
    let (world, mut fake) = ready();
    fake.feed_lag = 1;
    // Before the tag: feed-alpha times out once, feed-stable answers 404 once.
    fake.failing_reads = vec![("feed-alpha", 28), ("feed-stable", 22)];
    // After it: feed-alpha cannot be resolved once, feed-stable answers 404 twice.
    fake.failing_after_the_tag = vec![("feed-alpha", 6), ("feed-stable", 22), ("feed-stable", 22)];
    let done = released(&world, &mut fake, false).0.unwrap();
    assert!(
        done.contains("feed-alpha serves it, feed-stable is as it was"),
        "{done}"
    );
}

#[test]
fn a_feed_alpha_that_answers_no_release_is_an_outage_not_a_version_to_change() {
    for (failing, last) in [
        (None, "it serves no release".to_owned()),
        (
            Some(6),
            "curl ended 6: curl: (6) the read failed".to_owned(),
        ),
    ] {
        let (world, mut fake) = ready();
        match failing {
            None => {
                fake.feeds.remove("feed-alpha");
            }
            Some(code) => fake.failing_reads = vec![("feed-alpha", code); 5],
        }
        assert_eq!(
            refusal(released(&world, &mut fake, true).0),
            format!(
                "feed-alpha answered no release in 5 reads ({last}): an outage of the feed or of GitHub, \
                 not a version to change; nothing was done, run again later"
            )
        );
        assert!(fake.when("gh").is_none(), "{:#?}", fake.lines());
    }
}

#[test]
fn a_tag_upstream_refuses_says_what_origin_holds_and_a_run_again_says_how_to_finish() {
    let (world, mut fake) = ready();
    fake.refuses_pushes = Some("upstream");
    let tag = format!("v{ALPHA}");
    let said = refusal(released(&world, &mut fake, false).0);
    assert_eq!(
        said,
        format!(
            "upstream refused {tag} (! [remote rejected] (pre-receive hook declined)), which origin has, \
             and no Release run started. Push it again, `git push upstream {tag}`, and watch its Release run; \
             or delete it, `git tag -d {tag} && git push origin --delete {tag}`, and run again"
        )
    );
    assert!(World::tagged(&world.origin, &tag).is_some());
    assert!(fake.runs_of("release.yml", "push").is_empty());
    // Run again with upstream taking pushes: the half pushed tag is named, and how to finish it.
    fake.refuses_pushes = None;
    let said = refusal(released(&world, &mut fake, false).0);
    assert_eq!(
        said,
        not_on_upstream(
            &tag,
            "in the checkout and on origin",
            &format!("git tag -d {tag} && git push origin --delete {tag}")
        )
    );
}

#[test]
fn a_tag_origin_refuses_reaches_no_remote_and_says_to_delete_it() {
    let (world, mut fake) = ready();
    fake.refuses_pushes = Some("origin");
    let tag = format!("v{ALPHA}");
    let said = refusal(released(&world, &mut fake, false).0);
    assert_eq!(
        said,
        format!(
            "origin refused {tag} (! [remote rejected] (pre-receive hook declined)): no remote has it; \
             delete it, `git tag -d {tag}`, and run again"
        )
    );
    assert_eq!(World::tagged(&world.upstream, &tag), None);
}

#[test]
fn a_tag_whose_release_run_never_appears_says_what_to_do_with_the_tag() {
    let (world, mut fake) = ready();
    fake.tags_start_runs = false;
    let tag = format!("v{ALPHA}");
    let said = refusal(released(&world, &mut fake, false).0);
    assert_eq!(
        said,
        format!(
            "release: no new run of release.yml appeared on {}: {tag} is on upstream; {}",
            &world.head()[..8],
            advice(&tag, None)
        )
    );
}

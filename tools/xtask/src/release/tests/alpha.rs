//! `release alpha`, run on a checkout with real remotes and GitHub as a script.

use super::fake::{git, Fake, World};
use super::{advice, not_on_upstream, red_run, refusal, TRIAL_NEXT, TRIAL_NEXT_WITHOUT_A_RUN};
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
fn an_alpha_is_trialled_by_hand_then_tagged_with_its_notes_watched_and_served_by_feed_alpha_alone()
{
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
    // CI asked as the tag's gate asks it, then the hand run of the Release workflow on
    // main (the one branch the `release` environment admits besides tags) watched to
    // its end, then the tag to origin before upstream, then the release.
    let ci = fake
        .when(&format!(
            "gh api repos/ngvoicu/consensflow/actions/workflows/ci.yml/runs?head_sha={head}&"
        ))
        .unwrap();
    let jobs = fake
        .when("gh api repos/ngvoicu/consensflow/actions/runs/1000/jobs?")
        .unwrap();
    let lines = fake.lines();
    let dispatched = lines
        .iter()
        .position(|line| {
            line == "gh workflow run release.yml --repo ngvoicu/consensflow --ref main"
        })
        .unwrap();
    let trial_ended = lines
        .iter()
        .rposition(|line| line.starts_with("gh run view 1001 "))
        .unwrap();
    let origin = fake.when(&format!("git push -q origin {tag}")).unwrap();
    let upstream = fake.when(&format!("git push -q upstream {tag}")).unwrap();
    assert!(
        ci < jobs
            && jobs < dispatched
            && dispatched < trial_ended
            && trial_ended < origin
            && origin < upstream,
        "{lines:#?}"
    );
    // One hand run, on this commit, and the tag's run is the one push run of the workflow.
    assert_eq!(
        lines
            .iter()
            .filter(|line| line.starts_with("gh workflow run"))
            .count(),
        1
    );
    let trials = fake.runs_of("release.yml", "workflow_dispatch");
    assert_eq!(
        (trials.len(), trials[0].id, &trials[0].commit),
        (1, 1001, &head)
    );
    assert_eq!(fake.runs_of("release.yml", "push").len(), 1);
    assert!(fake.pauses > 0);
    for line in [
        &format!(
            "ci: waiting until CI's run of {} ends (CI did not pass",
            &head[..8]
        ),
        &format!("ci: run 1000 passed {} on both platforms", &head[..8]),
        "trial: run 1001 of release.yml, until it ends",
        "trial run 1001: success",
        "release run 1002: success",
        "feed-alpha serves 3.0.6-alpha.1",
    ] {
        assert!(said.contains(line), "{line}: {said}");
    }
}

#[test]
fn a_published_release_is_checked_once_the_feeds_serve_it_and_what_held_is_said() {
    let (world, mut fake) = ready();
    let (outcome, said) = released(&world, &mut fake, false);
    outcome.unwrap();
    // After the tag's run, and after the feeds were read: the release's files, then the feed.
    let lines = fake.lines();
    let tagged = lines
        .iter()
        .position(|line| line.starts_with(&format!("git push -q upstream v{ALPHA}")))
        .unwrap();
    let downloaded = lines
        .iter()
        .position(|line| line.starts_with(&format!("gh release download v{ALPHA} ")))
        .unwrap();
    let last_feed_read = lines
        .iter()
        .rposition(|line| line.starts_with("curl") && line.ends_with("feed-stable/latest.json"))
        .unwrap();
    assert!(tagged < downloaded, "{lines:#?}");
    assert!(last_feed_read < downloaded, "{lines:#?}");
    for line in [
        "verify: SHA256SUMS lists 5 files, each in the release and hashing as listed".to_owned(),
        format!("verify: ConsensFlow_{ALPHA}_aarch64.app.tar.gz is signed by the key v{ALPHA}'s tauri.conf.json pins"),
        format!("verify: a copy of ConsensFlow_{ALPHA}_aarch64.app.tar.gz with one byte changed is refused"),
        format!("verify: feed-alpha serves {ALPHA}: its latest.json is the release's"),
    ] {
        assert!(said.contains(&line), "{line}: {said}");
    }
}

#[test]
fn a_release_that_does_not_verify_once_published_says_it_is_published_and_never_to_run_again() {
    let (world, mut fake) = ready();
    // The installer is changed after SHA256SUMS was written.
    fake.published_with = Some(|version, files| {
        files
            .get_mut(&format!("ConsensFlow_{version}_aarch64.dmg"))
            .unwrap()[0] ^= 1;
    });
    let said = refusal(released(&world, &mut fake, false).0);
    assert!(
        said.starts_with(&format!(
            "release run 1002 ended green and the feeds serve {ALPHA}, so the release is published, \
             yet checking it found v{ALPHA} does not verify, 1 problem:\n- ConsensFlow_{ALPHA}_aarch64.dmg hashes to "
        )),
        "{said}"
    );
    assert!(
        said.ends_with(&format!(
            "; run nothing again: `cargo xtask release verify {ALPHA}` checks it again"
        )),
        "{said}"
    );
    // The tag stays, and so does the release: nothing was undone.
    assert!(World::tagged(&world.upstream, &format!("v{ALPHA}")).is_some());
    assert_eq!(fake.serves("feed-alpha").as_deref(), Some(ALPHA));
}

#[test]
fn a_release_that_cannot_be_read_once_published_is_told_as_published_too() {
    let (world, mut fake) = ready();
    fake.publishes_files = false;
    let said = refusal(released(&world, &mut fake, false).0);
    assert!(
        said.starts_with(&format!(
            "release run 1002 ended green and the feeds serve {ALPHA}, so the release is published, \
             yet it could not be checked (v{ALPHA} has no release to download: "
        )),
        "{said}"
    );
    assert!(
        said.ends_with(&format!(
            "; run nothing again: `cargo xtask release verify {ALPHA}` checks it again"
        )),
        "{said}"
    );
}

#[test]
fn a_dry_run_watches_ci_says_it_would_trial_and_tag_and_starts_and_tags_nothing() {
    let (world, mut fake) = ready();
    let head = world.head();
    let (outcome, _) = released(&world, &mut fake, true);
    let done = outcome.unwrap();
    assert_eq!(
        done,
        format!(
            "dry run: every check passed; a hand run of release.yml would be started on {short}, \
             and once it ended green v{ALPHA} would be tagged on it with {} and pushed to origin and upstream\n",
            world.notes.display(),
            short = &head[..8]
        )
    );
    assert!(
        fake.when("gh api repos/ngvoicu/consensflow/actions/runs/1000/jobs?")
            .is_some(),
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
            format!(
                "CI did not pass {} on both platforms: run 1000 ended {ended}; nothing after it was done",
                &world.head()[..8]
            )
        );
        nothing_tagged(&world, &fake);
    }
}

#[test]
fn ci_without_both_platforms_summaries_green_is_refused_and_nothing_is_tagged() {
    for (jobs, unmet) in [
        // A summary that did not pass, though the run says success.
        (
            vec![
                ("gate (macos-latest)", "success"),
                ("gate (windows-latest)", "skipped"),
            ],
            "gate (windows-latest) ended skipped",
        ),
        // A platform's jobs green but no summary: a job that holds its name is not it.
        (
            vec![
                ("gate (macos-latest)", "success"),
                ("test (windows-latest, rig)", "success"),
            ],
            "it has no job gate (windows-latest)",
        ),
        (
            vec![("gate (windows-latest)", "success")],
            "it has no job gate (macos-latest)",
        ),
    ] {
        let world = World::new(ALPHA);
        let mut fake = Fake::new(&world, "3.0.5", "3.0.5");
        fake.ci_jobs = jobs;
        fake.start("ci.yml", &world.head(), "push");
        let said = refusal(released(&world, &mut fake, false).0);
        assert_eq!(
            said,
            format!(
                "CI did not pass {} on both platforms: run 1000 ended success, but {unmet}; \
                 nothing after it was done",
                &world.head()[..8]
            )
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
            "CI (ci.yml) never ran on {}; nothing after it was done",
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
        "a landing CI has not run on\n",
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
            "CI (ci.yml) never ran on {}; nothing after it was done",
            &world.head()[..8]
        )
    );
    nothing_tagged(&world, &fake);
}

#[test]
fn ci_of_a_pull_request_on_main_s_commit_is_not_its_gate() {
    // A pull request's run tests a merge of the commit, not it: a push's or a hand run's counts.
    let (world, _) = ready();
    let mut fake = Fake::new(&world, "3.0.5", "3.0.5");
    fake.start("ci.yml", &world.head(), "pull_request");
    let said = refusal(released(&world, &mut fake, false).0);
    assert_eq!(
        said,
        format!(
            "CI (ci.yml) never ran on {} (1 run of a pull request tested a merge of it, which is not it); \
             nothing after it was done",
            &world.head()[..8]
        )
    );
    nothing_tagged(&world, &fake);
}

#[test]
fn a_green_run_by_hand_passes_main_s_commit_beside_a_red_push_run_as_the_tags_gate_would() {
    // The push's run of main failed on a flake, and CI was run again by hand,
    // as the gate's refusal advises: the tag's gate takes the hand run, and so
    // does the command.
    let (world, _) = ready();
    let mut fake = Fake::new(&world, "3.0.5", "3.0.5");
    fake.conclusions.insert(("ci.yml", "push"), "failure");
    fake.start("ci.yml", &world.head(), "push");
    let by_hand = fake.start("ci.yml", &world.head(), "workflow_dispatch");
    let (outcome, said) = released(&world, &mut fake, false);
    outcome.unwrap();
    assert!(
        said.contains(&format!(
            "ci: run {by_hand} passed {} on both platforms",
            &world.head()[..8]
        )),
        "{said}"
    );
    assert!(
        said.contains("ci: run 1000 ended failure, beside it"),
        "{said}"
    );
    assert!(World::tagged(&world.upstream, &format!("v{ALPHA}")).is_some());
}

#[test]
fn a_ci_run_github_does_not_list_yet_is_waited_for() {
    // CI's run of main's commit shows in GitHub's list two looks late.
    let (world, _) = ready();
    let mut fake = Fake::new(&world, "3.0.5", "3.0.5");
    fake.unlisted_for = 2;
    fake.start("ci.yml", &world.head(), "push");
    fake.unlisted_for = 0;
    let (outcome, said) = released(&world, &mut fake, false);
    outcome.unwrap();
    assert!(said.contains("ci: run 1000 passed"), "{said}");
}

#[test]
fn a_ci_run_still_going_is_waited_for_beside_a_red_one() {
    // The push's run failed and a hand run goes on: no refusal until it ends.
    let (world, _) = ready();
    let mut fake = Fake::new(&world, "3.0.5", "3.0.5");
    fake.conclusions.insert(("ci.yml", "push"), "failure");
    fake.start("ci.yml", &world.head(), "push");
    fake.running_for = 4;
    fake.start("ci.yml", &world.head(), "workflow_dispatch");
    fake.running_for = 1;
    let (outcome, said) = released(&world, &mut fake, false);
    outcome.unwrap();
    assert!(said.contains("passed"), "{said}");
}

#[test]
fn a_github_that_cannot_be_asked_of_ci_stops_it_with_what_gh_said() {
    let (world, mut fake) = ready();
    fake.gh_fails = true;
    match released(&world, &mut fake, false).0 {
        Err(Error::Answer { what, said }) => {
            assert_eq!(what, format!("whether CI passed {}", &world.head()[..8]));
            assert!(said.contains("gh auth login"), "{said}");
        }
        other => panic!("not GitHub's answer: {other:?}"),
    }
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
        red_run(
            "release",
            1002,
            "failure",
            "job mac ended failure, job windows / build ended failure",
            &format!("{tag} stays on upstream; {}", advice(&tag, Some(1002)))
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
            advice(&tag, Some(1002))
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
    // 1001 is the older run, 1002 the trial, 1003 the tag's.
    assert!(said.starts_with("release run 1003 ended failure"), "{said}");
}

/// The hand run that fails at `step` of `job`, as the Release workflow's would.
fn trial_fails_at(fake: &mut Fake, ended: &'static str, job: &'static str, step: &'static str) {
    fake.conclusions
        .insert(("release.yml", "workflow_dispatch"), ended);
    fake.failed_steps
        .insert(("release.yml", "workflow_dispatch"), (job, step));
}

#[test]
fn a_trial_that_ends_red_names_its_job_its_step_and_its_run_and_nothing_is_tagged() {
    for (job, step, ended) in [
        ("mac", "The signed app passes the packaged smoke", "failure"),
        ("mac", "Sign and notarize the app and its DMG", "failure"),
        ("windows / build", "Build the app", "timed_out"),
    ] {
        let (world, mut fake) = ready();
        trial_fails_at(&mut fake, ended, job, step);
        let said = refusal(released(&world, &mut fake, false).0);
        assert_eq!(
            said,
            red_run(
                "trial",
                1001,
                ended,
                &format!("job {job}, step \"{step}\" ended {ended}"),
                TRIAL_NEXT
            )
        );
        // The run's address is the one to read it at, and the tag's run never started.
        assert!(said.contains("https://github.com/ngvoicu/consensflow/actions/runs/1001"));
        assert_eq!(
            fake.runs.len(),
            2,
            "CI's run and the trial: {:#?}",
            fake.runs
        );
        nothing_tagged(&world, &fake);
        assert_eq!(fake.serves("feed-alpha").as_deref(), Some("3.0.5"));
    }
}

#[test]
fn a_trial_cancelled_with_no_step_to_name_is_refused_with_its_jobs() {
    let (world, mut fake) = ready();
    fake.conclusions
        .insert(("release.yml", "workflow_dispatch"), "cancelled");
    let said = refusal(released(&world, &mut fake, false).0);
    assert_eq!(
        said,
        red_run(
            "trial",
            1001,
            "cancelled",
            "job mac ended cancelled, job windows / build ended cancelled",
            TRIAL_NEXT
        )
    );
    nothing_tagged(&world, &fake);
}

#[test]
fn an_older_hand_run_of_the_same_commit_is_not_taken_for_the_trial() {
    let (world, mut fake) = ready();
    // A green hand run of this commit from before; the trial fails, and shows a look late.
    fake.start("release.yml", &world.head(), "workflow_dispatch");
    trial_fails_at(
        &mut fake,
        "failure",
        "mac",
        "The signed app passes the packaged smoke",
    );
    fake.unlisted_for = 1;
    let said = refusal(released(&world, &mut fake, false).0);
    assert!(said.starts_with("trial run 1002 ended failure"), "{said}");
    nothing_tagged(&world, &fake);
}

#[test]
fn a_hand_run_that_gh_cannot_start_stops_it_with_what_gh_said_and_nothing_is_tagged() {
    let (world, mut fake) = ready();
    fake.dispatch_fails = true;
    match released(&world, &mut fake, false).0 {
        Err(Error::Program {
            command,
            code,
            said,
        }) => {
            assert_eq!(
                command,
                "gh workflow run release.yml --repo ngvoicu/consensflow --ref main"
            );
            assert_eq!(code, 1);
            assert!(said.contains("HTTP 403"), "{said}");
        }
        other => panic!("not gh's refusal: {other:?}"),
    }
    assert!(fake.runs_of("release.yml", "workflow_dispatch").is_empty());
    nothing_tagged(&world, &fake);
}

#[test]
fn a_trial_github_never_lists_is_refused_and_nothing_is_tagged() {
    let (world, mut fake) = ready();
    fake.hand_runs_start = false;
    let said = refusal(released(&world, &mut fake, false).0);
    assert_eq!(
        said,
        format!(
            "trial: no new run of release.yml appeared on {}: {TRIAL_NEXT_WITHOUT_A_RUN}",
            &world.head()[..8]
        )
    );
    nothing_tagged(&world, &fake);
}

/// The arguments that make `git commit` the commit of someone else, signed by no one.
const ANOTHER_AUTHOR: [&str; 6] = [
    "-c",
    "user.name=Other",
    "-c",
    "user.email=o@example.com",
    "-c",
    "commit.gpgsign=false",
];

/// Someone else lands a commit on upstream's main, from a clone of their own.
fn lands_on_upstream(upstream: &std::path::Path) {
    let other = tempfile::tempdir().unwrap();
    let clone = other.path().join("clone");
    git(
        other.path(),
        &["clone", "-q", &upstream.to_string_lossy(), "clone"],
    );
    let landing = [
        &ANOTHER_AUTHOR[..],
        &["commit", "-q", "--allow-empty", "-m", "a landing"],
    ]
    .concat();
    git(&clone, &landing);
    git(&clone, &["push", "-q", "origin", "main"]);
}

#[test]
fn main_that_moved_while_ci_was_watched_is_not_trialled_for_a_hand_run_is_of_main_s_head() {
    // Upstream's main moves on alone, or the checkout's with both remotes' ones.
    for together in [false, true] {
        let (world, mut fake) = ready();
        let (root, upstream) = (world.root.clone(), world.upstream.clone());
        fake.after_ci = Some(Box::new(move || {
            if together {
                let landing = [
                    &ANOTHER_AUTHOR[..],
                    &["commit", "-q", "--allow-empty", "-m", "a landing"],
                ]
                .concat();
                git(&root, &landing);
                for remote in ["origin", "upstream"] {
                    git(&root, &["push", "-q", remote, "main"]);
                }
            } else {
                lands_on_upstream(&upstream);
            }
        }));
        let said = refusal(released(&world, &mut fake, false).0);
        if together {
            assert!(
                said.starts_with(&format!(
                    "main moved on from {} to ",
                    &world.commit_of("HEAD~1")[..8]
                )),
                "{said}"
            );
        } else {
            assert!(said.contains("push or pull until they are one"), "{said}");
        }
        // No hand run was asked for, since it would be of the new head, and nothing was tagged.
        assert!(
            fake.when("gh workflow run").is_none(),
            "{:#?}",
            fake.lines()
        );
        nothing_tagged(&world, &fake);
    }
}

#[test]
fn a_hand_run_of_another_commit_is_not_taken_for_the_trial() {
    let (world, mut fake) = ready();
    trial_fails_at(
        &mut fake,
        "failure",
        "mac",
        "The signed app passes the packaged smoke",
    );
    // Ours shows a look late. Someone else's hand run, of another commit, newer than
    // ours and green, is there first: tagging on it would tag what was not tried.
    fake.unlisted_for = 1;
    fake.after_dispatch = Some(Box::new(|fake| {
        let key = ("release.yml", "workflow_dispatch");
        let red = fake.conclusions.remove(&key);
        fake.unlisted_for = 0;
        fake.start("release.yml", &"f".repeat(40), "workflow_dispatch");
        fake.conclusions.extend(red.map(|ended| (key, ended)));
    }));
    let said = refusal(released(&world, &mut fake, false).0);
    // The run watched is ours, 1001, not the other's, 1002.
    assert_eq!(
        said,
        red_run(
            "trial",
            1001,
            "failure",
            "job mac, step \"The signed app passes the packaged smoke\" ended failure",
            TRIAL_NEXT
        )
    );
    nothing_tagged(&world, &fake);
}

#[test]
fn a_run_of_another_commit_that_gh_lists_anyway_is_refused_for_what_it_says_of_itself() {
    let (world, mut fake) = ready();
    // GitHub's filter by commit does not hold, our own hand run never starts, and
    // another commit's is the newest there is.
    fake.ignores_commit_filter = true;
    fake.hand_runs_start = false;
    fake.after_dispatch = Some(Box::new(|fake| {
        fake.start("release.yml", &"f".repeat(40), "workflow_dispatch");
    }));
    let said = refusal(released(&world, &mut fake, false).0);
    assert_eq!(
        said,
        format!(
            "trial run 1001 (https://github.com/ngvoicu/consensflow/actions/runs/1001) is of ffffffff, not {}: {TRIAL_NEXT}",
            &world.head()[..8]
        )
    );
    nothing_tagged(&world, &fake);
}

#[test]
fn main_that_moved_between_the_check_and_the_dispatch_is_refused_at_once_with_the_run_of_the_new_head(
) {
    let (world, mut fake) = ready();
    let upstream = world.upstream.clone();
    // After the command asked main once more, and before GitHub took main's head for the run.
    fake.before_dispatch = Some(Box::new(move || lands_on_upstream(&upstream)));
    let ours = world.head();
    let said = refusal(released(&world, &mut fake, false).0);
    let new_head = git(&world.upstream, &["rev-parse", "main"]);
    assert_ne!(new_head, ours);
    assert_eq!(
        said,
        format!(
            "main moved on to {new}: run 1001 is of {new}, not {old}; nothing was tagged. Run the command \
             again, once that run (https://github.com/ngvoicu/consensflow/actions/runs/1001) has ended",
            new = &new_head[..8],
            old = &ours[..8]
        )
    );
    // At once: the one wait was CI's run, going on for a look. Not twenty minutes of
    // looks for a run of ours that GitHub will never list.
    assert_eq!(fake.pauses, 1);
    nothing_tagged(&world, &fake);
}

#[test]
fn a_main_that_moved_after_the_dispatch_does_not_stop_the_trial_of_the_commit_it_ran_on() {
    // An older hand run, of another commit, is the newest there is when ours is asked
    // for; then main moves on, and ours shows a look late, or two.
    for unlisted in [1, 2] {
        let (world, mut fake) = ready();
        fake.start("release.yml", &"e".repeat(40), "workflow_dispatch");
        fake.unlisted_for = unlisted;
        let upstream = world.upstream.clone();
        fake.after_dispatch = Some(Box::new(move |_| lands_on_upstream(&upstream)));
        let head = world.head();
        // The trial is 1002, of the commit asked for: green, and the tag follows on it.
        released(&world, &mut fake, false).0.unwrap();
        let trials = fake.runs_of("release.yml", "workflow_dispatch");
        assert_eq!(
            trials.last().map(|run| (run.id, &run.commit)),
            Some((1002, &head))
        );
        for remote in [&world.origin, &world.upstream] {
            assert_eq!(
                World::tagged(remote, &format!("v{ALPHA}")),
                Some(head.clone())
            );
        }
    }
}

#[test]
fn a_gh_that_fails_while_the_trial_is_watched_is_a_refusal_naming_the_run_and_that_nothing_was_tagged(
) {
    // While the run is looked at.
    let (world, mut fake) = ready();
    fake.view_fails = vec![1001];
    let said = refusal(released(&world, &mut fake, false).0);
    assert_eq!(
        said,
        format!(
            "trial run 1001 (https://github.com/ngvoicu/consensflow/actions/runs/1001): gh could not say how \
             it went (`gh run view 1001 --repo ngvoicu/consensflow --json status,conclusion,headSha,jobs` ended \
             with status 1: HTTP 502: Bad Gateway); {TRIAL_NEXT}"
        )
    );
    nothing_tagged(&world, &fake);
    // While the run is looked for, after it was asked for.
    let (world, mut fake) = ready();
    fake.after_dispatch = Some(Box::new(|fake| fake.gh_fails = true));
    let head = world.head();
    let said = refusal(released(&world, &mut fake, false).0);
    assert_eq!(
        said,
        format!(
            "trial: the run of release.yml on {} could not be looked for (`gh run list --repo ngvoicu/consensflow \
             --workflow release.yml --commit {head} --event workflow_dispatch --limit 1 --json databaseId,headSha` \
             ended with status 4: To get started with GitHub CLI, please run:  gh auth login); \
             {TRIAL_NEXT_WITHOUT_A_RUN}",
            &head[..8]
        )
    );
    nothing_tagged(&world, &fake);
}

#[test]
fn a_gh_that_fails_while_the_tags_run_is_watched_says_the_tag_stays_and_names_the_run() {
    let (world, mut fake) = ready();
    // The trial is 1001 and the tag's run 1002.
    fake.view_fails = vec![1002];
    let tag = format!("v{ALPHA}");
    let said = refusal(released(&world, &mut fake, false).0);
    assert_eq!(
        said,
        format!(
            "release run 1002 (https://github.com/ngvoicu/consensflow/actions/runs/1002): gh could not say how \
             it went (`gh run view 1002 --repo ngvoicu/consensflow --json status,conclusion,headSha,jobs` ended \
             with status 1: HTTP 502: Bad Gateway); {tag} stays on upstream; {}",
            advice(&tag, Some(1002))
        )
    );
    for remote in [&world.origin, &world.upstream] {
        assert!(World::tagged(remote, &tag).is_some());
    }
}

#[test]
fn a_feed_alpha_that_never_serves_the_alpha_after_a_green_run_says_the_release_is_published() {
    let (world, mut fake) = ready();
    fake.publishes = false;
    let said = refusal(released(&world, &mut fake, false).0);
    assert_eq!(
        said,
        format!(
            "release run 1002 ended green, its own check of the feeds with it, yet feed-alpha does not serve \
             {ALPHA} after 5 reads (it serves 3.0.5): the release is published, and GitHub's download links \
             can lag; read it again in a few minutes, \
             `curl -sL https://github.com/ngvoicu/consensflow/releases/download/feed-alpha/latest.json`, \
             and run nothing again"
        )
    );
    // One look at each of the three runs (CI's, the trial's and the tag's) while it goes on,
    // and a wait between each two of five reads.
    assert_eq!(fake.pauses, 3 + 4);
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
             an alpha never moves it; read the publish job of run 1002"
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
                 not a version to change; release run 1002 ended green, so the release is published; \
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

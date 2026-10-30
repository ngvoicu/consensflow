//! `release promote`, run on a checkout whose alpha was released, with real
//! remotes and GitHub as a script. A promotion always goes on `release/X.Y.Z`
//! and never writes main, whether main moved on from the alpha or not.

use super::fake::{git, git_says_yes, latest, version_files, write, Fake, World};
use super::{advice, not_on_upstream, red_run, refusal};
use crate::release::{promote, Error, Options};

const ALPHA: &str = "3.0.6-alpha.1";
const PLAIN: &str = "3.0.6";
const BRANCH: &str = "release/3.0.6";

fn options(world: &World, alpha: &str, dry_run: bool) -> Options {
    Options {
        version: Some(alpha.to_owned()),
        notes: world.notes.clone(),
        dry_run,
    }
}

/// A checkout whose alpha was released from its main, which feed-alpha serves.
fn released() -> (World, Fake) {
    let world = World::new(ALPHA);
    world.released(ALPHA);
    let fake = Fake::new(&world, ALPHA, "3.0.5");
    (world, fake)
}

/// The same, with main moved on past the alpha's commit since.
fn moved_on() -> (World, Fake) {
    let (world, fake) = released();
    write(&world.root.join("README.md"), "a landing after the alpha\n");
    world.commit("a landing after the alpha");
    world.push_main();
    (world, fake)
}

fn promoted(
    world: &World,
    fake: &mut Fake,
    alpha: &str,
    dry_run: bool,
) -> (Result<String, Error>, String) {
    world.run(fake, promote::release, &options(world, alpha, dry_run))
}

/// The files the commit `commit` changed from its parent, and what its
/// sources say their version is.
fn change_of(world: &World, commit: &str) -> (Vec<String>, String) {
    let names = git(
        &world.root,
        &["diff", "--name-only", &format!("{commit}^"), commit],
    );
    let package = git(&world.root, &["show", &format!("{commit}:package.json")]);
    let document: serde_json::Value = serde_json::from_str(&package).unwrap();
    (
        names.lines().map(str::to_owned).collect(),
        document["version"].as_str().unwrap().to_owned(),
    )
}

/// What the promotion's tree left: every worktree but the checkout is gone.
fn no_tree_left(world: &World) {
    let trees = git(&world.root, &["worktree", "list", "--porcelain"]);
    assert_eq!(
        trees
            .lines()
            .filter(|line| line.starts_with("worktree "))
            .count(),
        1,
        "{trees}"
    );
    assert!(!world
        .root
        .join("app/src-tauri/target/promote")
        .join(PLAIN)
        .exists());
}

/// Whether `repository` has the branch of the promotion.
fn has_branch(repository: &std::path::Path) -> bool {
    git_says_yes(
        repository,
        &[
            "rev-parse",
            "-q",
            "--verify",
            &format!("refs/heads/{BRANCH}"),
        ],
    )
}

const CHANGED: [&str; 5] = [
    "Cargo.lock",
    "Cargo.toml",
    "app/src-tauri/tauri.conf.json",
    "package-lock.json",
    "package.json",
];

#[test]
fn a_tested_alpha_is_promoted_from_a_branch_at_its_commit_and_main_is_never_written() {
    for moved in [false, true] {
        let (world, mut fake) = if moved { moved_on() } else { released() };
        fake.feed_lag = 1;
        let alpha_commit = world.commit_of(&format!("v{ALPHA}"));
        let main_before = world.head();
        let (outcome, said) = promoted(&world, &mut fake, ALPHA, false);
        let done = outcome.unwrap();
        let promoted = git(&world.upstream, &["rev-parse", BRANCH]);
        // The branch, in the checkout and on both remotes, holds the version commit on the alpha's.
        assert_eq!(git(&world.origin, &["rev-parse", BRANCH]), promoted);
        assert_eq!(git(&world.root, &["rev-parse", BRANCH]), promoted);
        assert_eq!(
            git(&world.root, &["rev-parse", &format!("{promoted}^")]),
            alpha_commit
        );
        let (names, version) = change_of(&world, &promoted);
        assert_eq!(names, CHANGED);
        assert_eq!(version, PLAIN);
        for (path, text) in version_files(PLAIN) {
            assert_eq!(
                git(&world.root, &["show", &format!("{promoted}:{path}")]),
                text.trim_end(),
                "{path}"
            );
        }
        // Main is as it was, everywhere, and the checkout is still on it.
        for repository in [&world.root, &world.origin, &world.upstream] {
            assert_eq!(
                git(repository, &["rev-parse", "main"]),
                main_before,
                "moved: {moved}"
            );
        }
        assert_eq!(
            git(&world.root, &["rev-parse", "--abbrev-ref", "HEAD"]),
            "main"
        );
        for remote in [&world.origin, &world.upstream] {
            assert_eq!(
                World::tagged(remote, &format!("v{PLAIN}")),
                Some(promoted.clone())
            );
        }
        // The branch's push ran CI on the version commit, watched green before the tag;
        // nothing is started by hand: a hand run of the Release workflow holds the signing
        // keys, and the `release` environment admits main and v* tags, not release/*.
        assert!(
            fake.when("gh workflow run").is_none(),
            "{:#?}",
            fake.lines()
        );
        let ci = fake.runs_of("ci.yml", "push");
        let ci: Vec<_> = ci.iter().filter(|run| run.commit == promoted).collect();
        assert_eq!(ci.len(), 1, "{:#?}", fake.runs);
        let watched = fake
            .when(&format!(
                "gh api repos/ngvoicu/consensflow/actions/runs/{}/jobs?",
                ci[0].id
            ))
            .unwrap();
        let branch_pushed = fake
            .when(&format!("git push -q upstream {BRANCH}"))
            .unwrap();
        let tagged = fake.when(&format!("git push -q origin v{PLAIN}")).unwrap();
        assert!(
            branch_pushed < watched && watched < tagged,
            "{:#?}",
            fake.lines()
        );
        assert!(
            said.contains(&format!(
                "ci: run {} passed {} on both platforms",
                ci[0].id,
                &promoted[..8]
            )),
            "{said}"
        );
        assert_eq!(fake.runs_of("release.yml", "push").len(), 1);
        assert_eq!(fake.serves("feed-alpha").as_deref(), Some(PLAIN));
        assert_eq!(fake.serves("feed-stable").as_deref(), Some(PLAIN));
        no_tree_left(&world);
        assert_eq!(
            done,
            format!(
                "released {PLAIN} from {ALPHA}'s commit {} as {} on {BRANCH}: feed-alpha and feed-stable serve it\n\
                 nothing goes back to main: its next alpha needs a version after {PLAIN} (3.0.7-alpha.1 or later)\n",
                &alpha_commit[..8],
                &promoted[..8]
            )
        );
        assert!(said.contains("feed-stable serves 3.0.6"), "{said}");
    }
}

#[test]
fn a_promoted_release_is_checked_once_both_feeds_serve_it_and_a_bad_one_is_told_as_published() {
    let (world, mut fake) = released();
    let (outcome, said) = promoted(&world, &mut fake, ALPHA, false);
    outcome.unwrap();
    // A plain version is served by both feeds, and both are held to the release.
    for feed in ["feed-alpha", "feed-stable"] {
        let line = format!("verify: {feed} serves {PLAIN}: its latest.json is the release's");
        assert!(said.contains(&line), "{line}: {said}");
    }
    let (world, mut fake) = released();
    fake.published_with = Some(|version, files| {
        files
            .get_mut(&format!("ConsensFlow_{version}_aarch64.dmg"))
            .unwrap()[0] ^= 1;
    });
    let said = refusal(promoted(&world, &mut fake, ALPHA, false).0);
    let run = fake.runs_of("release.yml", "push")[0].id;
    assert!(
        said.starts_with(&format!(
            "release run {run} ended green and the feeds serve {PLAIN}, so the release is published, \
             yet checking it found v{PLAIN} does not verify, 1 problem:"
        )),
        "{said}"
    );
    assert!(
        said.ends_with(&format!(
            "; run nothing again: `cargo xtask release verify {PLAIN}` checks it again"
        )),
        "{said}"
    );
}

#[test]
fn a_promotion_dry_run_makes_the_commit_and_pushes_and_tags_nothing() {
    for moved in [false, true] {
        let (world, mut fake) = if moved { moved_on() } else { released() };
        let main_before = world.head();
        let (outcome, _) = promoted(&world, &mut fake, ALPHA, true);
        let done = outcome.unwrap();
        assert!(done.starts_with("dry run: every check passed; "), "{done}");
        assert!(
            done.contains(&format!(
                "would be {BRANCH} in the checkout and on origin and upstream, tagged v{PLAIN}"
            )),
            "{done}"
        );
        assert!(
            fake.lines().iter().all(|line| !line.starts_with("git push")
                && !line.starts_with("git branch")
                && !line.starts_with("gh workflow run")
                && !line.starts_with("git tag")),
            "{:#?}",
            fake.lines()
        );
        for repository in [&world.root, &world.origin, &world.upstream] {
            assert_eq!(git(repository, &["rev-parse", "main"]), main_before);
            assert_eq!(World::tagged(repository, &format!("v{PLAIN}")), None);
            assert!(!has_branch(repository));
        }
        no_tree_left(&world);
    }
}

#[test]
fn a_gh_that_fails_stops_a_promotion_before_any_push_in_the_dry_run_too() {
    for dry_run in [true, false] {
        let (world, mut fake) = released();
        fake.gh_fails = true;
        match promoted(&world, &mut fake, ALPHA, dry_run).0 {
            Err(Error::Program { command, code, .. }) => {
                assert!(command.starts_with("gh run list "), "{command}");
                assert_eq!(code, 4);
            }
            other => panic!("not gh's error, dry run {dry_run}: {other:?}"),
        }
        assert!(
            fake.when("git push").is_none() && fake.when("git branch").is_none(),
            "{:#?}",
            fake.lines()
        );
        for repository in [&world.root, &world.origin, &world.upstream] {
            assert!(!has_branch(repository));
            assert_eq!(World::tagged(repository, &format!("v{PLAIN}")), None);
        }
        no_tree_left(&world);
    }
}

#[test]
fn a_promotion_asks_nothing_of_the_checkouts_own_branch_or_changes() {
    // It writes no main and reads none: a checkout elsewhere, with changes, promotes the same.
    let (world, mut fake) = released();
    git(&world.root, &["checkout", "-q", "-b", "wt/elsewhere"]);
    write(&world.root.join("README.md"), "an edit not committed\n");
    promoted(&world, &mut fake, ALPHA, false).0.unwrap();
    assert!(has_branch(&world.upstream));
}

#[test]
fn an_argument_that_is_no_alpha_is_refused_before_anything_runs() {
    for version in [PLAIN, "3.0.6-rc.1", "3.0.6-alpha"] {
        let (world, mut fake) = released();
        let said = refusal(promoted(&world, &mut fake, version, false).0);
        assert_eq!(
            said,
            format!("{version} is not an alpha: an alpha is X.Y.Z-alpha.N")
        );
        assert!(fake.ran.is_empty());
    }
}

#[test]
fn an_alpha_that_was_never_released_is_refused() {
    let (world, mut fake) = released();
    let said = refusal(promoted(&world, &mut fake, "3.0.6-alpha.2", false).0);
    assert_eq!(
        said,
        "v3.0.6-alpha.2 is not on upstream: 3.0.6-alpha.2 was never released"
    );
    // A tag the checkout alone has is not a release either.
    let (world, mut fake) = released();
    git(&world.root, &["tag", "v3.0.6-alpha.2"]);
    let said = refusal(promoted(&world, &mut fake, "3.0.6-alpha.2", false).0);
    assert_eq!(
        said,
        "v3.0.6-alpha.2 is not on upstream: 3.0.6-alpha.2 was never released"
    );
}

#[test]
fn a_fetch_of_the_alpha_that_fails_is_gits_error_never_not_released() {
    let (world, mut fake) = released();
    fake.tag_fetch_fails = true;
    match promoted(&world, &mut fake, ALPHA, false).0 {
        Err(Error::Program { command, said, .. }) => {
            assert!(
                command.starts_with("git fetch -q --no-tags upstream"),
                "{command}"
            );
            assert!(said.contains("Could not resolve host"), "{said}");
        }
        other => panic!("not git's error: {other:?}"),
    }
}

#[test]
fn an_alpha_feed_alpha_does_not_serve_is_refused() {
    for serves in ["3.0.6-alpha.2", "3.0.5", PLAIN] {
        let (world, mut fake) = released();
        fake.feeds.insert("feed-alpha".into(), latest(serves));
        let said = refusal(promoted(&world, &mut fake, ALPHA, false).0);
        assert_eq!(
            said,
            format!(
                "feed-alpha serves {serves}, not {ALPHA}: only the alpha it serves, which alpha installs run, is promoted"
            )
        );
        assert!(fake.when("git worktree add").is_none());
    }
    // A feed that answers no release is an outage, said as one.
    let (world, mut fake) = released();
    fake.feeds.remove("feed-alpha");
    let said = refusal(promoted(&world, &mut fake, ALPHA, false).0);
    assert!(
        said.starts_with("feed-alpha answered no release in 5 reads"),
        "{said}"
    );
}

#[test]
fn a_version_already_released_is_refused() {
    // Its tag, on upstream, with its run.
    let (world, mut fake) = released();
    git(&world.root, &["tag", "v3.0.6"]);
    git(&world.root, &["push", "-q", "upstream", "v3.0.6"]);
    git(&world.root, &["tag", "-d", "v3.0.6"]);
    let run = fake.start("release.yml", &world.head(), "push");
    let said = refusal(promoted(&world, &mut fake, ALPHA, false).0);
    assert_eq!(
        said,
        format!(
            "v3.0.6 is already on upstream, on {}, so its Release run started; {}",
            &world.head()[..8],
            advice("v3.0.6", Some(run))
        )
    );

    // feed-stable serves it, or a later one, with no tag here.
    for serves in [PLAIN, "3.0.7"] {
        let (world, mut fake) = released();
        fake.feeds.insert("feed-stable".into(), latest(serves));
        let said = refusal(promoted(&world, &mut fake, ALPHA, false).0);
        assert_eq!(
            said,
            format!("feed-stable already serves {serves}: 3.0.6 was released, or a later version")
        );
        assert!(fake.when("git worktree add").is_none());
    }
}

/// What a refusal says of the branch a promotion left: where it is, at which
/// commit, and how to start over, or finish when its parent is the alpha's and
/// CI passed it.
fn leftover(places: &str, at: &str, alpha_commit: &str, undo: &str) -> String {
    format!(
        "{BRANCH} is {places}, at {}: a promotion stopped before its tag. Start over: `{undo}`, and run again; \
         or, if its parent is {alpha_commit} and CI is green on it, finish it by hand: \
         `git tag -a v{PLAIN} -F <notes> {at} && git push origin v{PLAIN} && git push upstream v{PLAIN}`, \
         then watch its Release run and read both feeds",
        &at[..8]
    )
}

#[test]
fn a_branch_a_promotion_left_is_refused_with_how_to_start_over_or_finish() {
    for (remotes, local, places, undo) in [
        (
            &["upstream"][..],
            false,
            "on upstream",
            format!("git push upstream --delete {BRANCH}"),
        ),
        (
            &["origin"][..],
            true,
            "in the checkout and on origin",
            format!("git push origin --delete {BRANCH} && git branch -D {BRANCH}"),
        ),
        (
            &[][..],
            true,
            "in the checkout",
            format!("git branch -D {BRANCH}"),
        ),
    ] {
        let (world, mut fake) = moved_on();
        let at = world.head();
        let alpha_commit = world.commit_of(&format!("v{ALPHA}"));
        for remote in remotes {
            git(
                &world.root,
                &["push", "-q", remote, &format!("main:refs/heads/{BRANCH}")],
            );
        }
        if local {
            git(&world.root, &["branch", BRANCH, "main"]);
        }
        let said = refusal(promoted(&world, &mut fake, ALPHA, false).0);
        assert_eq!(
            said,
            leftover(places, &at, &alpha_commit, &undo),
            "{places}"
        );
        assert!(fake.when("git worktree add").is_none());
        // What the words tag is the branch's commit, wherever the branch is: in
        // the checkout, or on a remote alone.
        let tagged = said
            .split("-F <notes> ")
            .nth(1)
            .and_then(|rest| rest.split(' ').next())
            .unwrap();
        git(&world.root, &["tag", "-a", "v3.0.6", "-m", "notes", tagged]);
        assert_eq!(world.commit_of("v3.0.6"), at, "{places}");
    }
}

#[test]
fn a_branch_upstream_refuses_says_what_to_delete_and_once_deleted_a_run_again_promotes() {
    let (world, mut fake) = released();
    fake.refuses_pushes = Some("upstream");
    let said = refusal(promoted(&world, &mut fake, ALPHA, false).0);
    assert_eq!(
        said,
        format!(
            "upstream refused {BRANCH} (! [remote rejected] (pre-receive hook declined)), which origin has; \
             delete it, `git push origin --delete {BRANCH} && git branch -D {BRANCH}`, and run again"
        )
    );
    assert!(!has_branch(&world.upstream) && has_branch(&world.origin) && has_branch(&world.root));
    assert!(fake.when("git tag").is_none());
    // Run again: the half pushed branch is refused with the same deletion.
    fake.refuses_pushes = None;
    let at = git(&world.root, &["rev-parse", BRANCH]);
    // A version commit the promotion made: its parent is the alpha's, so it may be finished by hand.
    let alpha_commit = world.commit_of(&format!("v{ALPHA}"));
    assert_eq!(
        git(&world.root, &["rev-parse", &format!("{at}^")]),
        alpha_commit
    );
    let said = refusal(promoted(&world, &mut fake, ALPHA, false).0);
    assert_eq!(
        said,
        leftover(
            "in the checkout and on origin",
            &at,
            &alpha_commit,
            &format!("git push origin --delete {BRANCH} && git branch -D {BRANCH}")
        )
    );
    // Done as it says, the promotion goes through.
    git(&world.root, &["push", "-q", "origin", "--delete", BRANCH]);
    git(&world.root, &["branch", "-D", BRANCH]);
    promoted(&world, &mut fake, ALPHA, false).0.unwrap();
    assert!(has_branch(&world.upstream));
}

#[test]
fn a_branch_origin_refuses_reaches_no_remote_and_says_to_delete_the_checkouts() {
    let (world, mut fake) = released();
    fake.refuses_pushes = Some("origin");
    let said = refusal(promoted(&world, &mut fake, ALPHA, false).0);
    assert_eq!(
        said,
        format!(
            "origin refused {BRANCH} (! [remote rejected] (pre-receive hook declined)): no remote has it; \
             delete it, `git branch -D {BRANCH}`, and run again"
        )
    );
    assert!(!has_branch(&world.origin) && !has_branch(&world.upstream));
}

#[test]
fn a_tag_upstream_refuses_is_named_on_the_run_again_with_how_to_push_it() {
    let (world, mut fake) = released();
    let tag = format!("v{PLAIN}");
    // The branch goes to both remotes; the tag to origin, and upstream refuses it.
    fake.refuses_pushes = Some("upstream");
    fake.refused_refs = "v";
    let said = refusal(promoted(&world, &mut fake, ALPHA, false).0);
    assert!(
        said.starts_with(&format!("upstream refused {tag} (! [remote rejected]")),
        "{said}"
    );
    assert!(has_branch(&world.upstream));
    assert!(fake.runs_of("release.yml", "push").is_empty());
    fake.refuses_pushes = None;
    // A run again names the tag first: it is what the run stopped at.
    let said = refusal(promoted(&world, &mut fake, ALPHA, false).0);
    assert_eq!(
        said,
        not_on_upstream(
            &tag,
            "in the checkout and on origin",
            &format!("git tag -d {tag} && git push origin --delete {tag}")
        )
    );
    // Pushed by hand as it says (past GitHub's script, so no run is started),
    // the next run finds it on upstream, on the branch's commit.
    let at = git(&world.root, &["rev-parse", BRANCH]);
    git(&world.root, &["push", "-q", "upstream", &tag]);
    let said = refusal(promoted(&world, &mut fake, ALPHA, false).0);
    assert!(
        said.starts_with(&format!("{tag} is already on upstream, on {}", &at[..8])),
        "{said}"
    );
}

#[test]
fn a_release_run_that_fails_says_to_rerun_it_and_a_run_again_names_the_same_run() {
    let (world, mut fake) = released();
    fake.conclusions.insert(("release.yml", "push"), "failure");
    let tag = format!("v{PLAIN}");
    let said = refusal(promoted(&world, &mut fake, ALPHA, false).0);
    let run = fake.runs_of("release.yml", "push")[0].id;
    assert_eq!(
        said,
        red_run(
            "release",
            run,
            "failure",
            "job mac ended failure, job windows / build ended failure",
            &format!("{tag} stays on upstream; {}", advice(&tag, Some(run)))
        )
    );
    let promoted_commit = git(&world.upstream, &["rev-parse", BRANCH]);
    let said = refusal(promoted(&world, &mut fake, ALPHA, false).0);
    assert_eq!(
        said,
        format!(
            "{tag} is already on upstream, on {}, so its Release run started; {}",
            &promoted_commit[..8],
            advice(&tag, Some(run))
        )
    );
}

#[test]
fn feeds_that_do_not_serve_after_a_green_run_say_the_release_is_published() {
    let (world, mut fake) = released();
    fake.publishes = false;
    let said = refusal(promoted(&world, &mut fake, ALPHA, false).0);
    let run = fake.runs_of("release.yml", "push")[0].id;
    assert!(
        said.starts_with(&format!(
            "release run {run} ended green, its own check of the feeds with it, yet feed-alpha does not serve 3.0.6 after 5 reads (it serves {ALPHA})"
        )),
        "{said}"
    );
    assert!(said.ends_with("and run nothing again"), "{said}");
}

#[test]
fn an_alpha_tag_on_a_commit_of_another_version_is_refused() {
    let world = World::new("3.0.5");
    world.released(ALPHA);
    let mut fake = Fake::new(&world, ALPHA, "3.0.5");
    let said = refusal(promoted(&world, &mut fake, ALPHA, false).0);
    assert_eq!(
        said,
        format!(
            "v{ALPHA} is on {}, whose sources say 3.0.5: it is not that alpha's commit",
            &world.head()[..8]
        )
    );
    no_tree_left(&world);
}

#[test]
fn a_version_file_that_says_the_version_another_number_of_times_is_refused() {
    let world = World::new(ALPHA);
    let lockfile = world.root.join("package-lock.json");
    let text = std::fs::read_to_string(&lockfile).unwrap();
    write(
        &lockfile,
        &text.replacen(&format!("\"{ALPHA}\""), "\"3.0.5\"", 1),
    );
    world.commit("a lockfile out of step");
    world.push_main();
    world.released(ALPHA);
    let mut fake = Fake::new(&world, ALPHA, "3.0.5");
    let said = refusal(promoted(&world, &mut fake, ALPHA, false).0);
    assert!(
        said.starts_with(&format!("package-lock.json says \"{ALPHA}\" 1 times at")),
        "{said}"
    );
    no_tree_left(&world);
}

#[test]
fn a_lockfile_or_a_file_cargo_changed_beyond_the_version_is_refused_and_nothing_goes_out() {
    let (world, mut fake) = released();
    fake.cargo_adds_to_the_lockfile = Some("\n[[package]]\nname = \"new\"\nversion = \"0.1.0\"\n");
    let said = refusal(promoted(&world, &mut fake, ALPHA, false).0);
    assert_eq!(
        said,
        "Cargo.lock changed beyond the version: only \"3.0.6-alpha.1\" may become \"3.0.6\""
    );
    assert!(fake.when("git push").is_none() && fake.when("git commit").is_none());
    no_tree_left(&world);

    let (world, mut fake) = released();
    fake.cargo_writes = Some(("crates/cf/src/stray.rs", "// not a version\n"));
    let said = refusal(promoted(&world, &mut fake, ALPHA, false).0);
    assert_eq!(said, "the promotion changed crates/cf/src/stray.rs, which holds no version: only the version may change");
    assert!(fake.when("git push").is_none());
    no_tree_left(&world);
}

/// What a refusal says of the branch a promotion pushed when CI did not pass its
/// commit `at`: `rerun` is how to have CI run again.
fn ci_not_green(at: &str, rerun: &str) -> String {
    format!(
        "{BRANCH} is on origin and upstream at {}, and no tag was pushed. Its commit is the alpha's with the \
         version alone changed: {rerun}, and once CI is green on it finish by hand, \
         `git tag -a v{PLAIN} -F <notes> {at} && git push origin v{PLAIN} && git push upstream v{PLAIN}`, \
         then watch its Release run and read both feeds; or start over, \
         `git push origin --delete {BRANCH} && git push upstream --delete {BRANCH} && git branch -D {BRANCH}`, \
         and run again",
        &at[..8]
    )
}

/// Whether nothing was tagged, in the checkout or on a remote, and no Release run started.
fn no_tag(world: &World, fake: &Fake) {
    assert!(fake.when("git tag").is_none(), "{:#?}", fake.lines());
    for repository in [&world.root, &world.origin, &world.upstream] {
        assert_eq!(World::tagged(repository, &format!("v{PLAIN}")), None);
    }
    assert!(fake.runs_of("release.yml", "push").is_empty());
}

#[test]
fn ci_that_ended_red_on_the_version_commit_stops_it_before_the_tag() {
    for ended in ["failure", "cancelled"] {
        let (world, mut fake) = released();
        fake.conclusions.insert(("ci.yml", "push"), ended);
        let said = refusal(promoted(&world, &mut fake, ALPHA, false).0);
        let at = git(&world.upstream, &["rev-parse", BRANCH]);
        let run = fake.runs_of("ci.yml", "push")[0].id;
        assert_eq!(
            said,
            format!(
                "CI did not pass {} on both platforms: run {run} ended {ended}; {}",
                &at[..8],
                ci_not_green(
                    &at,
                    &format!("a flake: rerun it, `gh run rerun {run} --failed --repo ngvoicu/consensflow`")
                )
            )
        );
        no_tag(&world, &fake);
        // Done as it says once CI is green, the words tag the version commit.
        let tagged = said
            .split("-F <notes> ")
            .nth(1)
            .and_then(|rest| rest.split(' ').next())
            .unwrap();
        assert_eq!(tagged, at);
    }
}

#[test]
fn ci_without_both_platforms_summaries_green_on_the_version_commit_stops_it_before_the_tag() {
    let (world, mut fake) = released();
    fake.ci_jobs = vec![
        ("gate (macos-latest)", "success"),
        ("test (windows-latest, rig)", "success"),
    ];
    let said = refusal(promoted(&world, &mut fake, ALPHA, false).0);
    let at = git(&world.upstream, &["rev-parse", BRANCH]);
    let run = fake.runs_of("ci.yml", "push")[0].id;
    assert_eq!(
        said,
        format!(
            "CI did not pass {} on both platforms: run {run} ended success, but it has no job \
             gate (windows-latest); {}",
            &at[..8],
            ci_not_green(
                &at,
                &format!(
                    "a flake: rerun it, `gh run rerun {run} --failed --repo ngvoicu/consensflow`"
                )
            )
        )
    );
    no_tag(&world, &fake);
}

#[test]
fn a_version_commit_ci_never_ran_on_is_not_tagged_though_ci_passed_the_alpha() {
    // CI passed the alpha's own commit, and none ran on the version commit: the
    // alpha's green gate is another commit's.
    let (world, mut fake) = released();
    fake.start("ci.yml", &world.commit_of(&format!("v{ALPHA}")), "push");
    fake.branches_start_ci = false;
    let said = refusal(promoted(&world, &mut fake, ALPHA, false).0);
    let at = git(&world.upstream, &["rev-parse", BRANCH]);
    assert_eq!(
        said,
        format!(
            "CI (ci.yml) never ran on {}; {}",
            &at[..8],
            ci_not_green(
                &at,
                &format!(
                    "start its CI by hand, `gh workflow run ci.yml --ref {BRANCH} --repo ngvoicu/consensflow`"
                )
            )
        )
    );
    no_tag(&world, &fake);
}

#[test]
fn a_green_run_by_hand_of_the_version_commit_is_taken_beside_a_red_push_run() {
    // The branch's push run failed, and CI was run on the branch by hand, as a
    // refusal advises: the tag's gate takes the hand run, and so does the
    // promotion, which tags.
    let (world, mut fake) = released();
    fake.conclusions.insert(("ci.yml", "push"), "failure");
    fake.branch_ci_by_hand = true;
    let (outcome, said) = promoted(&world, &mut fake, ALPHA, false);
    outcome.unwrap();
    let at = git(&world.upstream, &["rev-parse", BRANCH]);
    let by_hand = fake.runs_of("ci.yml", "workflow_dispatch")[0].id;
    assert!(
        said.contains(&format!(
            "ci: run {by_hand} passed {} on both platforms",
            &at[..8]
        )),
        "{said}"
    );
    assert_eq!(
        World::tagged(&world.upstream, &format!("v{PLAIN}")),
        Some(at)
    );
}

#[test]
fn a_promotion_waits_for_a_run_by_hand_that_goes_on_beside_a_red_push_run() {
    let (world, mut fake) = released();
    fake.conclusions.insert(("ci.yml", "push"), "failure");
    fake.branch_ci_by_hand = true;
    fake.running_for = 3;
    promoted(&world, &mut fake, ALPHA, false).0.unwrap();
    assert!(World::tagged(&world.upstream, &format!("v{PLAIN}")).is_some());
}

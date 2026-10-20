//! `release promote`, run on a checkout whose alpha was released, with real
//! remotes and GitHub as a script.

use super::fake::{git, git_says_yes, latest, version_files, write, Fake, World};
use super::refusal;
use crate::release::{promote, Error, Options};

const ALPHA: &str = "3.0.6-alpha.1";
const PLAIN: &str = "3.0.6";

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

const CHANGED: [&str; 5] = [
    "Cargo.lock",
    "Cargo.toml",
    "app/src-tauri/tauri.conf.json",
    "package-lock.json",
    "package.json",
];

#[test]
fn an_alpha_main_still_is_is_promoted_on_main_and_served_by_both_feeds() {
    let (world, mut fake) = released();
    fake.feed_lag = 1;
    let alpha_commit = world.head();
    let (outcome, said) = promoted(&world, &mut fake, ALPHA, false);
    let done = outcome.unwrap();
    let main = git(&world.upstream, &["rev-parse", "main"]);
    // The version commit sits on the alpha's commit, on main of both remotes and of the checkout.
    assert_eq!(
        git(&world.root, &["rev-parse", &format!("{main}^")]),
        alpha_commit
    );
    assert_eq!(git(&world.origin, &["rev-parse", "main"]), main);
    assert_eq!(world.head(), main);
    let (names, version) = change_of(&world, &main);
    assert_eq!(names, CHANGED);
    assert_eq!(version, PLAIN);
    // Every version file is the plain version's, and nothing else changed.
    for (path, text) in version_files(PLAIN) {
        assert_eq!(
            git(&world.root, &["show", &format!("{main}:{path}")]),
            text.trim_end(),
            "{path}"
        );
    }
    for remote in [&world.origin, &world.upstream] {
        assert_eq!(
            World::tagged(remote, &format!("v{PLAIN}")),
            Some(main.clone())
        );
    }
    assert_eq!(fake.serves("feed-alpha").as_deref(), Some(PLAIN));
    assert_eq!(fake.serves("feed-stable").as_deref(), Some(PLAIN));
    // CI's push run on the version commit, the dry run on main, then the tag.
    assert_eq!(fake.runs_of("ci.yml", "push").len(), 1);
    assert_eq!(fake.runs_of("ci.yml", "push")[0].commit, main);
    let dry = fake
        .when("gh workflow run release.yml --repo ngvoicu/consensflow --ref main")
        .unwrap();
    let tag = fake.when(&format!("git tag -a v{PLAIN}")).unwrap();
    assert!(dry < tag);
    no_tree_left(&world);
    assert!(done.contains(&format!(
        "released {PLAIN} from {ALPHA}'s commit {} as {} on main: feed-alpha and feed-stable serve it",
        &alpha_commit[..8],
        &main[..8]
    )), "{done}");
    assert!(
        done.contains(
            "main is at 3.0.6: its next alpha needs a version after it (3.0.7-alpha.1 or later)"
        ),
        "{done}"
    );
    assert!(said.contains("feed-stable serves 3.0.6"), "{said}");
}

#[test]
fn an_alpha_main_has_moved_past_is_promoted_from_a_branch_at_its_commit() {
    let (world, mut fake) = moved_on();
    let alpha_commit = world.commit_of(&format!("v{ALPHA}"));
    let main_before = world.head();
    let (outcome, said) = promoted(&world, &mut fake, ALPHA, false);
    let done = outcome.unwrap();
    let branch = format!("release/{PLAIN}");
    let promoted = git(&world.upstream, &["rev-parse", &branch]);
    assert_eq!(git(&world.origin, &["rev-parse", &branch]), promoted);
    assert_eq!(git(&world.root, &["rev-parse", &branch]), promoted);
    assert_eq!(
        git(&world.root, &["rev-parse", &format!("{promoted}^")]),
        alpha_commit
    );
    let (names, version) = change_of(&world, &promoted);
    assert_eq!(names, CHANGED);
    assert_eq!(version, PLAIN);
    // Main is as it was, everywhere: nothing goes back to it.
    for repository in [&world.root, &world.origin, &world.upstream] {
        assert_eq!(git(repository, &["rev-parse", "main"]), main_before);
    }
    for remote in [&world.origin, &world.upstream] {
        assert_eq!(
            World::tagged(remote, &format!("v{PLAIN}")),
            Some(promoted.clone())
        );
    }
    // CI by hand on the branch, no dry run (the environment admits main and tags alone).
    let ci = fake.runs_of("ci.yml", "workflow_dispatch");
    assert_eq!(ci.len(), 1);
    assert_eq!(ci[0].commit, promoted);
    assert!(fake.runs_of("release.yml", "workflow_dispatch").is_empty());
    assert!(fake
        .when(&format!(
            "gh workflow run ci.yml --repo ngvoicu/consensflow --ref {branch}"
        ))
        .is_some());
    assert!(said.contains("no dry run on release/3.0.6"), "{said}");
    assert_eq!(fake.serves("feed-alpha").as_deref(), Some(PLAIN));
    assert_eq!(fake.serves("feed-stable").as_deref(), Some(PLAIN));
    no_tree_left(&world);
    assert!(
        done.contains(&format!("as {} on release/3.0.6", &promoted[..8])),
        "{done}"
    );
    assert!(done.contains("nothing goes back to main"), "{done}");
    assert!(done.contains("3.0.7-alpha.1 or later"), "{done}");
}

#[test]
fn a_promotion_dry_run_makes_the_commit_and_pushes_tags_and_dispatches_nothing() {
    for moved in [false, true] {
        let (world, mut fake) = if moved { moved_on() } else { released() };
        let main_before = world.head();
        let (outcome, _) = promoted(&world, &mut fake, ALPHA, true);
        let done = outcome.unwrap();
        assert!(done.starts_with("dry run: every check passed; "), "{done}");
        let place = if moved {
            "pushed as release/3.0.6"
        } else {
            "pushed to main"
        };
        assert!(done.contains(place), "{done}");
        assert!(
            fake.lines().iter().all(|line| !line.starts_with("git push")
                && !line.starts_with("gh workflow run")
                && !line.starts_with("git tag")),
            "{:#?}",
            fake.lines()
        );
        for repository in [&world.root, &world.origin, &world.upstream] {
            assert_eq!(git(repository, &["rev-parse", "main"]), main_before);
            assert_eq!(World::tagged(repository, &format!("v{PLAIN}")), None);
            assert!(!git_says_yes(
                repository,
                &["rev-parse", "-q", "--verify", "refs/heads/release/3.0.6"]
            ));
        }
        no_tree_left(&world);
    }
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
fn an_alpha_feed_alpha_does_not_serve_is_refused() {
    for serves in [Some("3.0.6-alpha.2"), Some("3.0.5"), Some(PLAIN), None] {
        let (world, mut fake) = released();
        match serves {
            Some(version) => {
                fake.feeds.insert("feed-alpha".into(), latest(version));
            }
            None => {
                fake.feeds.remove("feed-alpha");
            }
        }
        let said = refusal(promoted(&world, &mut fake, ALPHA, false).0);
        assert_eq!(
            said,
            format!(
                "feed-alpha serves {}, not {ALPHA}: only the alpha it serves, which alpha installs run, is promoted",
                serves.unwrap_or("no release")
            )
        );
        assert!(fake.when("git worktree add").is_none());
    }
}

#[test]
fn a_version_already_released_is_refused() {
    // Its tag, on upstream.
    let (world, mut fake) = released();
    git(&world.root, &["tag", "v3.0.6"]);
    git(&world.root, &["push", "-q", "upstream", "v3.0.6"]);
    git(&world.root, &["tag", "-d", "v3.0.6"]);
    let said = refusal(promoted(&world, &mut fake, ALPHA, false).0);
    assert_eq!(
        said,
        "v3.0.6 is already a tag on upstream: that version was released, or begun"
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

#[test]
fn a_branch_a_promotion_left_is_refused() {
    let (world, mut fake) = moved_on();
    git(
        &world.root,
        &["push", "-q", "upstream", "main:refs/heads/release/3.0.6"],
    );
    let said = refusal(promoted(&world, &mut fake, ALPHA, false).0);
    assert_eq!(
        said,
        "upstream already has release/3.0.6: a promotion of 3.0.6 was begun; finish it or delete the branch"
    );
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

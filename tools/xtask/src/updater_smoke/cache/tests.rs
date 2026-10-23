//! Where the builds are kept, whose folder it is, who holds it, and what the kept
//! update was built from.

use super::*;
use crate::updater_smoke::testing::{fake_bundle, hold_when_free, Fake};

/// A context for a checkout at `root`, whose user's home is `home`.
fn context_at(root: &str, home: &Path) -> Context {
    Context {
        root: PathBuf::from(root),
        env: Env::from_vars([("HOME", home.as_os_str())]),
    }
}

/// A git repository with a commit, and the means to change it.
struct Repo(tempfile::TempDir);

impl Repo {
    fn new() -> Self {
        let repo = Self(tempfile::tempdir().unwrap());
        repo.git(&["init", "-q"]);
        repo.write(".gitignore", "ignored\n");
        repo.write("f", "first\n");
        repo.commit("first");
        repo
    }

    fn path(&self) -> &Path {
        self.0.path()
    }

    fn git(&self, args: &[&str]) -> String {
        let ran = process::capture(
            &Invocation::new("git", self.path())
                .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
                .args(args.iter().copied()),
            &Env::from_process(),
        )
        .unwrap();
        assert_eq!(ran.code, 0, "git {args:?}: {}", ran.stderr);
        ran.stdout
    }

    fn write(&self, name: &str, text: &str) {
        fs::write(self.path().join(name), text).unwrap();
    }

    fn commit(&self, what: &str) {
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "-m", what]);
    }

    fn state(&self) -> State {
        State::of(self.path(), None, &Env::from_process()).unwrap()
    }

    fn older(&self, update: &Path) -> Option<String> {
        older_update(update, self.path(), &Env::from_process())
    }
}

#[test]
fn keeps_the_builds_by_default_outside_every_checkout() {
    // The release a run installs is exported into the cache and built there with its own
    // `.cargo/config.toml`. Inside a checkout, Cargo reads the checkout's too, and an alias
    // that both write is doubled: `cargo xtask stage` ran as `cargo xtask run … stage`.
    let home = tempfile::tempdir().unwrap();
    let context = Context::new(&Env::from_vars([("HOME", home.path().as_os_str())])).unwrap();
    let cache = default_cache(&context).unwrap();
    let checkout = cache
        .ancestors()
        .skip(1)
        .find(|folder| folder.join("Cargo.toml").exists());
    assert_eq!(
        checkout,
        None,
        "the default cache {} is inside a checkout",
        cache.display()
    );
    assert!(
        !cache.starts_with(&context.root),
        "the default cache {} is inside {}",
        cache.display(),
        context.root.display()
    );
    assert!(
        cache.starts_with(home.path().join("Library/Caches/ConsensFlow/updater-smoke")),
        "{}",
        cache.display()
    );
}

#[test]
fn keeps_each_checkouts_builds_in_a_folder_of_its_own_that_is_the_same_every_run() {
    // The update is this checkout's app, and a run with `--reuse` takes the one it finds:
    // another tree's would be taken for this one's. And two runs at once build in two places.
    let home = tempfile::tempdir().unwrap();
    let cache_of = |root: &str| default_cache(&context_at(root, home.path())).unwrap();
    let main = cache_of("/work/consensflow");
    let tree = cache_of("/work/consensflow-trees/smoke-cache");
    assert_eq!(main, cache_of("/work/consensflow"));
    assert_ne!(main, tree);
    // Two checkouts of one name, in two places.
    assert_ne!(cache_of("/a/tree"), cache_of("/b/tree"));
    // They sit side by side, each named for its checkout.
    assert_eq!(main.parent(), tree.parent());
    let named = |cache: &Path| cache.file_name().unwrap().to_string_lossy().into_owned();
    assert!(named(&main).starts_with("consensflow-"), "{}", named(&main));
    assert!(named(&tree).starts_with("smoke-cache-"), "{}", named(&tree));
    assert_eq!(named(&tree).len(), "smoke-cache-".len() + 8);
}

#[test]
fn says_there_is_no_home_to_keep_the_builds_in_and_that_a_cache_names_one() {
    let context = Context {
        root: PathBuf::from("/work/consensflow"),
        env: Env::default(),
    };
    assert_eq!(
        default_cache(&context).unwrap_err().to_string(),
        "there is no home to keep the builds in: name a folder with --cache"
    );
}

#[test]
fn names_the_checkout_a_cache_is_for_in_a_file_of_its_own_that_a_cleanup_reads() {
    let cache = tempfile::tempdir().unwrap();
    mark_checkout(
        cache.path(),
        Path::new("/work/consensflow-trees/smoke-cache"),
    )
    .unwrap();
    assert_eq!(
        fs::read_to_string(cache.path().join(".checkout")).unwrap(),
        "/work/consensflow-trees/smoke-cache\n"
    );
    // The folder of a checkout that was moved is another's: what it says is the newest.
    mark_checkout(cache.path(), Path::new("/work/consensflow")).unwrap();
    assert_eq!(
        fs::read_to_string(cache.path().join(".checkout")).unwrap(),
        "/work/consensflow\n"
    );
}

#[test]
fn a_cache_is_held_by_one_run_at_a_time_and_the_second_is_told_so() {
    let cache = tempfile::tempdir().unwrap();
    let first = hold(cache.path()).unwrap();
    let said = hold(cache.path()).unwrap_err().to_string();
    assert!(said.starts_with("another run of the smoke"), "{said}");
    assert!(
        said.ends_with(&format!(
            "holds {}: wait for it to end, or give this run a folder of its own with --cache",
            cache.path().display()
        )),
        "{said}"
    );
    // Whose it is: where a lock can be read by another (not Windows's, which keeps it from reading).
    if cfg!(unix) {
        assert!(
            said.contains(&format!("(process {})", std::process::id())),
            "{said}"
        );
    }
    // Another cache is not held by it.
    let other = tempfile::tempdir().unwrap();
    let _other = hold(other.path()).unwrap();
    // A run that ended lets go of its cache.
    drop(first);
    let _again = hold_when_free(cache.path());
}

#[test]
fn the_state_of_a_checkout_is_its_commit_and_what_is_changed_in_it_staged_or_not() {
    let repo = Repo::new();
    let clean = repo.state();
    assert_eq!(clean.commit, repo.git(&["rev-parse", "HEAD"]).trim());
    assert_eq!(clean.changes, None);

    // A tracked file changed, and changed again: other changes.
    repo.write("f", "changed\n");
    let changed = repo.state();
    assert_eq!(changed.commit, clean.commit);
    assert!(changed.changes.is_some());
    repo.write("f", "changed again\n");
    assert_ne!(repo.state().changes, changed.changes);
    // Staged or not, the same changes are the same.
    repo.write("f", "changed\n");
    repo.git(&["add", "f"]);
    assert_eq!(repo.state(), changed);
    // Put back, nothing is changed.
    repo.write("f", "first\n");
    assert_eq!(repo.state(), clean);

    // A file in no commit counts, by what it holds; an ignored one does not.
    repo.write("new", "x\n");
    let added = repo.state();
    assert!(added.changes.is_some());
    repo.write("new", "y\n");
    assert_ne!(repo.state().changes, added.changes);
    fs::remove_file(repo.path().join("new")).unwrap();
    repo.write("ignored", "anything\n");
    assert_eq!(repo.state(), clean);

    // Another commit is another state.
    repo.write("f", "second\n");
    repo.commit("second");
    let next = repo.state();
    assert_ne!(next.commit, clean.commit);
    assert_eq!(next.changes, None);
}

#[test]
fn a_folder_that_is_no_checkout_has_no_state() {
    let folder = tempfile::tempdir().unwrap();
    let said = State::of(folder.path(), None, &Env::from_process())
        .unwrap_err()
        .to_string();
    assert!(said.starts_with("git rev-parse HEAD failed: "), "{said}");
}

#[test]
fn reads_back_the_state_it_wrote_and_nothing_else() {
    let repo = Repo::new();
    let clean = repo.state();
    assert_eq!(State::read(&clean.text()), Some(clean.clone()));
    repo.write("f", "changed\n");
    let changed = repo.state();
    assert_eq!(State::read(&changed.text()), Some(changed));
    for not_one in [
        "",
        "commit abc\n",
        "commit abc\nchanges none\nmore\n",
        "abc\nchanges none\n",
        "commit abc\nnone\n",
    ] {
        assert_eq!(State::read(not_one), None, "{not_one:?}");
    }
}

#[test]
fn says_when_the_kept_update_is_not_of_the_checkout_as_it_is() {
    let repo = Repo::new();
    let cache = tempfile::tempdir().unwrap();
    let update = cache.path().join("update").join("ConsensFlow.app");
    let mark = cache.path().join("update").join(".built-from");
    fs::create_dir_all(mark.parent().unwrap()).unwrap();

    // No record: an update from before there were any, or one that was copied in.
    assert_eq!(
        repo.older(&update).unwrap(),
        "the kept update has no record of what it was built from: build it again without --reuse"
    );
    // One that is not ours.
    fs::write(&mark, "built last Tuesday\n").unwrap();
    let said = repo.older(&update).unwrap();
    assert!(
        said.contains("is not one this xtask reads") && said.contains(&mark.display().to_string()),
        "{said}"
    );

    // The checkout as it was: nothing to say.
    fs::write(&mark, repo.state().text()).unwrap();
    assert_eq!(repo.older(&update), None);

    // Changed since: the commit it was built from and the changes, and the commit and changes now.
    repo.write("f", "changed\n");
    let said = repo.older(&update).unwrap();
    assert!(
        said.starts_with(
            "the kept update is not of this checkout as it is: it was built from commit "
        ),
        "{said}"
    );
    assert!(
        said.contains(" with no changes, and the checkout is at commit "),
        "{said}"
    );
    assert!(said.contains(" with changes "), "{said}");
    assert!(said.ends_with(": build it again without --reuse"), "{said}");
    // Committed since.
    repo.write("f", "first\n");
    assert_eq!(repo.older(&update), None);
    repo.write("f", "second\n");
    repo.commit("second");
    assert!(repo
        .older(&update)
        .unwrap()
        .contains("it was built from commit "));

    // A checkout that cannot be read says so, and does not stop the run.
    let folder = tempfile::tempdir().unwrap();
    let said = older_update(&update, folder.path(), &Env::from_process()).unwrap();
    assert!(
        said.starts_with(
            "could not tell whether the kept update is of this checkout as it is: git "
        ),
        "{said}"
    );
}

#[test]
#[cfg_attr(
    not(target_os = "macos"),
    ignore = "a bundle is copied as macOS's ditto copies it"
)]
fn keeps_the_update_with_what_the_checkout_was_when_the_build_started_to_read_it() {
    let repo = Repo::new();
    repo.write("f", "changed before the build\n");
    let before = repo.state();
    let scratch = tempfile::tempdir().unwrap();
    let cache = scratch.path().join("cache");
    let update = cache.join("update").join("ConsensFlow.app");
    let built = fake_bundle(&scratch.path().join("built"), &Fake::default());

    // A checkout that changes while the build goes on is the one the build began with.
    let kept = keep_update(repo.path(), &update, &Env::from_process(), || {
        repo.write("f", "changed during the build\n");
        Ok(built.clone())
    })
    .unwrap();
    assert_eq!(kept, update);
    assert!(update.join("Contents/Info.plist").is_file());
    assert_eq!(
        fs::read_to_string(cache.join("update").join(".built-from")).unwrap(),
        before.text()
    );
    assert!(repo.older(&update).unwrap().contains("it was built from"));

    // Built again, the record is the new one.
    keep_update(repo.path(), &update, &Env::from_process(), || {
        Ok(built.clone())
    })
    .unwrap();
    assert_eq!(repo.older(&update), None);
}

#[test]
#[cfg_attr(
    not(target_os = "macos"),
    ignore = "a bundle is copied as macOS's ditto copies it"
)]
fn a_cache_in_the_checkout_that_git_does_not_ignore_is_no_change_in_the_checkout() {
    let repo = Repo::new();
    // The cache is a folder of the checkout, and nothing ignores it.
    let cache = repo.path().join("smoke-cache");
    let update = cache.join("update").join("ConsensFlow.app");
    fs::create_dir_all(cache.join("bridge-source")).unwrap();
    fs::write(cache.join("bridge-source").join("exported"), "a release\n").unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let built = fake_bundle(&scratch.path().join("built"), &Fake::default());

    // The build adds to the cache (the update, the record of it, a run's folder).
    keep_update(repo.path(), &update, &Env::from_process(), || {
        fs::create_dir_all(cache.join("run-1")).unwrap();
        fs::write(cache.join("run-1").join("key"), "a key\n").unwrap();
        Ok(built.clone())
    })
    .unwrap();
    // What it records is the checkout, with nothing changed in it, and it is the checkout still.
    assert_eq!(
        fs::read_to_string(cache.join("update").join(".built-from")).unwrap(),
        format!(
            "commit {}\nchanges none\n",
            repo.git(&["rev-parse", "HEAD"]).trim()
        )
    );
    assert_eq!(repo.older(&update), None);

    // A file of the checkout's own beside the cache is a change, all the same.
    repo.write("new", "x\n");
    assert!(repo.older(&update).unwrap().contains("with changes "));
}

#[test]
#[cfg_attr(
    not(target_os = "macos"),
    ignore = "a bundle is copied as macOS's ditto copies it"
)]
fn an_update_built_where_the_checkout_cannot_be_read_has_no_record_and_one_that_failed_keeps_its_own(
) {
    let scratch = tempfile::tempdir().unwrap();
    let cache = scratch.path().join("cache");
    let update = cache.join("update").join("ConsensFlow.app");
    let mark = cache.join("update").join(".built-from");
    let built = fake_bundle(&scratch.path().join("built"), &Fake::default());
    fs::create_dir_all(cache.join("update")).unwrap();
    fs::write(&mark, "commit old\nchanges none\n").unwrap();

    // A build that failed leaves the update and its record as they were.
    let failed = keep_update(scratch.path(), &update, &Env::from_process(), || {
        Err(Error::new("the build failed"))
    });
    assert_eq!(failed.unwrap_err().to_string(), "the build failed");
    assert_eq!(
        fs::read_to_string(&mark).unwrap(),
        "commit old\nchanges none\n"
    );

    // A checkout that is none: the update is kept, and the record of the one before is not its own.
    keep_update(scratch.path(), &update, &Env::from_process(), || {
        Ok(built.clone())
    })
    .unwrap();
    assert!(update.join("Contents/Info.plist").is_file());
    assert!(!mark.exists());
}

//! K3's doors judge a place by the file system's identity for it, not by its path's text (N7,
//! T-2222): a folder that has two real spellings (`/Users/Shared` and
//! `/System/Volumes/Data/Users/Shared` on a Mac; an admin share or a `subst` drive on Windows) is
//! one place to the judge, to the seal and to what a run made. Every store here is a stand-in
//! folder of the test's own: never a harness home, never the Keychain.

use std::fs;
use std::path::{Path, PathBuf};

use super::account::TestRoot;
use super::place::Act;
use super::{inside_protected, judge, made};

/// `real`, a real path of a folder or file that is there, spelled the other way the system has
/// for it: through the data volume on a Mac, through the admin share of its drive on Windows.
/// None where this system has no second spelling.
fn other_spelling(real: &Path) -> Option<PathBuf> {
    let real = real.canonicalize().ok()?;
    #[cfg(target_os = "macos")]
    {
        let spelled = Path::new("/System/Volumes/Data").join(real.strip_prefix("/").ok()?);
        Some(spelled)
    }
    #[cfg(windows)]
    {
        let text = real.to_string_lossy().replace(r"\\?\", "");
        let (drive, rest) = text.split_once(':')?;
        let spelled =
            PathBuf::from(format!(r"\\localhost\{drive}$")).join(rest.trim_start_matches('\\'));
        // An admin share is the account's own where it is an administrator's: where it cannot be
        // reached the case has no second spelling to try.
        fs::metadata(&spelled).ok()?;
        Some(spelled)
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        let _ = real;
        None
    }
}

/// The second spelling of `folder`, which must lead to the same file.
fn second(folder: &Path) -> Option<PathBuf> {
    let spelled = other_spelling(folder)?;
    eprintln!("second spelling: {} of {}", spelled.display(), folder.display());
    assert_eq!(
        super::identity::of_path(folder).unwrap(),
        super::identity::of_path(&spelled).unwrap(),
        "{} is not another spelling of {}",
        spelled.display(),
        folder.display()
    );
    assert_ne!(
        spelled.canonicalize().unwrap(),
        folder.canonicalize().unwrap(),
        "the system resolves both spellings to one: the case proves nothing here"
    );
    Some(spelled)
}

#[test]
fn an_addition_inside_a_store_through_its_other_spelling_is_refused() {
    let root = made("k3-identity-").unwrap();
    let store = root.path().join("owner").join(".codex");
    super::make_dir(&store).unwrap();
    let _held = TestRoot::add(&store);
    let Some(spelled) = second(&store) else {
        return;
    };
    // Both spellings are the store: nothing is made new in it by either.
    for way in [&store, &spelled] {
        assert!(
            judge(&way.join("added"), Act::Name).is_err(),
            "{}",
            way.display()
        );
        assert!(judge(&way.join("a").join("b"), Act::Name).is_err());
        assert!(judge(&way.join("tmp"), Act::Allocate).is_err());
        assert!(super::made_in(&way.join("tmp"), "h-").is_err());
        assert!(super::create_new(way.join("file.txt")).is_err());
    }
    assert_eq!(
        fs::read_dir(&store).unwrap().count(),
        0,
        "something was added to the store"
    );
}

#[test]
fn a_store_that_is_not_there_yet_is_the_store_through_its_other_spelling() {
    let root = made("k3-identity-absent-").unwrap();
    let owner = root.path().join("owner");
    super::make_dir(&owner).unwrap();
    let store = owner.join(".gemini");
    let _held = TestRoot::add(&store);
    let Some(spelled) = second(&owner) else {
        return;
    };
    assert!(super::made_in(&spelled.join(".gemini").join("tmp"), "h-").is_err());
    assert!(judge(&spelled.join(".gemini"), Act::Name).is_err());
    assert!(!store.exists(), "the absent store was made");
}

#[test]
fn a_place_that_holds_a_store_is_not_taken_or_moved_through_its_other_spelling() {
    let root = made("k3-identity-tree-").unwrap();
    let tree = root.path().join("tree");
    let inner = tree.join("inner");
    super::make_dir(&inner).unwrap();
    let Some(spelled) = second(&tree) else {
        return;
    };
    // The account's store is spelled the second way; the tree it is in is named the first.
    let _held = TestRoot::add(&spelled.join("inner"));
    assert!(judge(&tree, Act::Remove).is_err());
    assert!(judge(&tree, Act::Rename).is_err());
    assert!(!super::Door::of_run().remove_in_made(&tree).is_gone());
    assert!(inner.is_dir());
}

#[test]
fn the_seal_takes_a_variable_that_names_a_store_through_its_other_spelling() {
    let root = made("k3-identity-seal-").unwrap();
    let store = root.path().join("owner").join("appdata");
    super::make_dir(&store).unwrap();
    let _held = TestRoot::add(&store);
    let Some(spelled) = second(&store) else {
        return;
    };
    assert!(inside_protected(&store.join("x")));
    assert!(inside_protected(&spelled));
    assert!(inside_protected(&spelled.join("x").join("y")));
    // Another folder of the same parent is no store.
    let beside = root.path().join("owner").join("beside");
    super::make_dir(&beside).unwrap();
    assert!(!inside_protected(&second(&beside).unwrap()));
}

#[test]
fn a_folder_this_run_made_is_the_runs_through_its_other_spelling_and_a_persons_is_not() {
    let mine = made("k3-identity-mine-").unwrap();
    let Some(spelled) = second(mine.path()) else {
        return;
    };
    let file = spelled.join("sub").join("note.txt");
    assert!(
        judge(&file, Act::Write).is_ok(),
        "the run's own folder, spelled the other way"
    );
    assert!(judge(&spelled, Act::Remove).is_ok());
    assert!(super::is_made_or_below(&spelled));
    assert!(super::is_made_or_below(&spelled.join("sub")));
    super::write(&file, "mine").unwrap();
    assert_eq!(
        fs::read_to_string(mine.path().join("sub").join("note.txt")).unwrap(),
        "mine"
    );
    // A person's folder is the run's by neither spelling.
    let (theirs, _kept) = (crate::standin_process::unowned_tempdir(), ());
    let their_spelled = second(theirs.path()).unwrap();
    assert!(judge(&their_spelled.join("new.txt"), Act::Write).is_err());
    assert!(!super::is_made_or_below(&their_spelled));
}

/// A store whose identity cannot be read is refused: the door never takes what it cannot judge
/// for what it can.
#[cfg(unix)]
#[test]
fn a_store_whose_identity_cannot_be_read_refuses_every_act() {
    use std::os::unix::fs::PermissionsExt as _;
    let root = made("k3-identity-unreadable-").unwrap();
    let blocked = root.path().join("blocked");
    super::make_dir(blocked.join("store")).unwrap();
    let store = blocked.join("store");
    // The roots are this test's own, never the global list the other tests add to.
    let roots = vec![store];
    fs::set_permissions(&blocked, fs::Permissions::from_mode(0o000)).unwrap();
    let made = vec![super::Entry {
        real: root.path().canonicalize().unwrap(),
        file: None,
    }];
    let outcome = super::place::judge_with(
        &root.path().join("elsewhere.txt"),
        Act::Write,
        &roots,
        &made,
    );
    let named = super::place::judge_with(&root.path().join("new"), Act::Name, &roots, &made);
    let seal = super::Roots::of(roots.clone()).holds(&root.path().join("elsewhere"));
    fs::set_permissions(&blocked, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(
        outcome.is_err(),
        "a root that cannot be read admitted a write"
    );
    assert!(named.is_err(), "a root that cannot be read admitted a name");
    assert!(
        seal,
        "a root that cannot be read let a path through the seal"
    );
}

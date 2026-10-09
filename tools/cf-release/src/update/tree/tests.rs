//! The tests of an app's tree: how a bundle is read, what is refused in it, and
//! how two trees are told apart.

use super::*;

fn plain(mode: u32, size: u64, sha256: &str) -> Entry {
    Entry {
        kind: Kind::File {
            size,
            sha256: sha256.to_string(),
        },
        mode,
    }
}

fn folder(mode: u32) -> Entry {
    Entry {
        kind: Kind::Dir,
        mode,
    }
}

fn tree(entries: &[(&str, Entry)]) -> Tree {
    entries
        .iter()
        .map(|(name, entry)| (name.to_string(), entry.clone()))
        .collect()
}

const HELLO: &str = "5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03";
const EMPTY: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

#[test]
fn the_sha256_of_what_a_reader_holds_comes_with_its_size() {
    let mut empty: &[u8] = b"";
    assert_eq!(digest(&mut empty).unwrap(), (0, EMPTY.to_string()));
    let mut hello: &[u8] = b"hello\n";
    assert_eq!(digest(&mut hello).unwrap(), (6, HELLO.to_string()));
    // More than one read's worth gives what one hash of it all would.
    let long = vec![b'a'; 1 << 20];
    let mut reader: &[u8] = &long;
    let (size, hex) = digest(&mut reader).unwrap();
    assert_eq!(size, 1 << 20);
    let mut whole = Sha256::new();
    whole.update(&long);
    assert_eq!(hex, format!("{:x}", whole.finalize()));
}

#[test]
fn a_reader_that_fails_is_no_digest() {
    struct Broken;
    impl Read for Broken {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Err(io::ErrorKind::TimedOut.into())
        }
    }
    assert!(digest(&mut Broken).is_err());
}

#[test]
fn a_child_of_a_folder_is_named_in_the_folder() {
    assert_eq!(
        named("ConsensFlow.app/Contents", OsStr::new("Info.plist")).unwrap(),
        "ConsensFlow.app/Contents/Info.plist"
    );
}

#[cfg(unix)]
#[test]
fn a_name_that_is_not_text_is_refused_by_its_folder() {
    use std::os::unix::ffi::OsStrExt;
    let bad = OsStr::from_bytes(b"caf\xe9");
    let refused = named("ConsensFlow.app/Contents", bad).unwrap_err();
    assert_eq!(
        refused.to_string(),
        "the app bundle contains a name that is not text: ConsensFlow.app/Contents/caf\u{fffd}"
    );
}

#[test]
fn a_path_that_is_not_there_is_unreadable_and_named() {
    let parent = tempfile::tempdir().unwrap();
    let missing = parent.path().join("Gone.app");
    let refused = of_bundle(&missing).unwrap_err();
    assert!(matches!(refused, TreeError::Unreadable { .. }), "{refused}");
    assert!(
        refused.to_string().starts_with(&format!(
            "could not read the app bundle: {}: ",
            missing.display()
        )),
        "{refused}"
    );
}

#[cfg(not(unix))]
#[test]
fn a_system_with_no_modes_cannot_hold_a_bundle_to_an_archive() {
    let parent = tempfile::tempdir().unwrap();
    let refused = of_bundle(parent.path()).unwrap_err();
    assert!(matches!(refused, TreeError::NoModes), "{refused}");
    assert_eq!(
        refused.to_string(),
        "the app bundle's file modes are read on macOS: this system keeps none"
    );
}

/// What reading a bundle needs a Unix for: its modes, and the files only it has.
#[cfg(unix)]
mod bundles {
    use std::os::unix::fs::{symlink, PermissionsExt};
    use std::os::unix::net::UnixListener;
    use std::path::PathBuf;

    use super::*;

    fn set_mode(path: &Path, mode: u32) {
        fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
    }

    /// A bundle of a folder, a program and a text, with the modes they are given.
    fn bundle(parent: &Path, name: &str) -> PathBuf {
        let app = parent.join(name);
        let contents = app.join("Contents");
        fs::create_dir_all(contents.join("MacOS")).unwrap();
        fs::write(contents.join("Info.plist"), "hello\n").unwrap();
        fs::write(contents.join("MacOS").join("run"), "").unwrap();
        set_mode(&app, 0o755);
        set_mode(&contents, 0o755);
        set_mode(&contents.join("MacOS"), 0o755);
        set_mode(&contents.join("Info.plist"), 0o644);
        set_mode(&contents.join("MacOS").join("run"), 0o755);
        app
    }

    #[test]
    fn a_bundle_is_read_path_by_path_under_the_name_the_archive_gives_it() {
        let parent = tempfile::tempdir().unwrap();
        // Whatever the folder is called, the archive's root is ConsensFlow.app.
        let read = of_bundle(&bundle(parent.path(), "Other.app")).unwrap();
        assert_eq!(
            read,
            tree(&[
                ("ConsensFlow.app", folder(0o755)),
                ("ConsensFlow.app/Contents", folder(0o755)),
                (
                    "ConsensFlow.app/Contents/Info.plist",
                    plain(0o644, 6, HELLO)
                ),
                ("ConsensFlow.app/Contents/MacOS", folder(0o755)),
                ("ConsensFlow.app/Contents/MacOS/run", plain(0o755, 0, EMPTY)),
            ])
        );
    }

    #[test]
    fn the_special_permission_bits_are_part_of_the_mode() {
        let parent = tempfile::tempdir().unwrap();
        let app = bundle(parent.path(), "ConsensFlow.app");
        let run = app.join("Contents").join("MacOS").join("run");
        for mode in [0o4755, 0o2755, 0o1755, 0o7755, 0o600, 0o444, 0o777] {
            set_mode(&run, mode);
            let read = of_bundle(&app).unwrap();
            assert_eq!(
                read["ConsensFlow.app/Contents/MacOS/run"].mode, mode,
                "{mode:o}"
            );
        }
    }

    #[test]
    fn a_symlink_is_refused_wherever_it_is_and_whatever_it_points_at() {
        for target in ["Info.plist", "../nowhere", "/etc", "."] {
            let parent = tempfile::tempdir().unwrap();
            let app = bundle(parent.path(), "ConsensFlow.app");
            symlink(target, app.join("Contents").join("link")).unwrap();
            let refused = of_bundle(&app).unwrap_err();
            assert_eq!(
                refused.to_string(),
                "the app bundle contains a symlink: ConsensFlow.app/Contents/link",
                "{target}"
            );
        }
        // The bundle itself may not be one either.
        let parent = tempfile::tempdir().unwrap();
        let real = bundle(parent.path(), "Real.app");
        let link = parent.path().join("ConsensFlow.app");
        symlink(&real, &link).unwrap();
        assert_eq!(
            of_bundle(&link).unwrap_err().to_string(),
            "the app bundle contains a symlink: ConsensFlow.app"
        );
    }

    #[test]
    fn a_file_that_is_neither_a_file_nor_a_folder_is_refused() {
        let parent = tempfile::tempdir().unwrap();
        let app = bundle(parent.path(), "ConsensFlow.app");
        let _socket = UnixListener::bind(app.join("Contents").join("socket")).unwrap();
        assert_eq!(
            of_bundle(&app).unwrap_err().to_string(),
            "the app bundle contains a special file: ConsensFlow.app/Contents/socket"
        );
    }

    #[test]
    fn the_first_refusal_is_the_one_first_in_the_order_of_the_names() {
        let parent = tempfile::tempdir().unwrap();
        let app = bundle(parent.path(), "ConsensFlow.app");
        for name in ["zeta", "alpha", "mid"] {
            symlink("Info.plist", app.join("Contents").join(name)).unwrap();
        }
        assert_eq!(
            of_bundle(&app).unwrap_err().to_string(),
            "the app bundle contains a symlink: ConsensFlow.app/Contents/alpha"
        );
    }

    #[test]
    fn a_file_that_cannot_be_opened_is_unreadable_and_named() {
        let parent = tempfile::tempdir().unwrap();
        let app = bundle(parent.path(), "ConsensFlow.app");
        let text = app.join("Contents").join("Info.plist");
        set_mode(&text, 0o000);
        // A superuser reads it all the same; nobody else does.
        if File::open(&text).is_ok() {
            return;
        }
        let refused = of_bundle(&app).unwrap_err();
        assert!(
            refused.to_string().starts_with(&format!(
                "could not read the app bundle: {}: ",
                text.display()
            )),
            "{refused}"
        );
    }
}

#[test]
fn two_trees_that_are_one_have_no_difference() {
    let one = tree(&[
        ("ConsensFlow.app", folder(0o755)),
        ("ConsensFlow.app/a", plain(0o644, 6, HELLO)),
    ]);
    assert_eq!(first_difference(&one, &one.clone()), None);
    assert_eq!(first_difference(&Tree::new(), &Tree::new()), None);
}

#[test]
fn a_path_in_one_tree_and_not_the_other_is_told_by_which_has_it() {
    let fewer = tree(&[
        ("ConsensFlow.app", folder(0o755)),
        ("ConsensFlow.app/a", plain(0o644, 6, HELLO)),
    ]);
    let mut more = fewer.clone();
    more.insert("ConsensFlow.app/b".into(), folder(0o755));
    assert_eq!(
        first_difference(&more, &fewer).unwrap(),
        "ConsensFlow.app/b is in the bundle and not in the archive"
    );
    assert_eq!(
        first_difference(&fewer, &more).unwrap(),
        "ConsensFlow.app/b is in the archive and not in the bundle"
    );
}

#[test]
fn a_path_that_is_not_the_same_in_both_says_what_sets_it_apart() {
    let one = |entry: Entry| tree(&[("ConsensFlow.app/a", entry)]);
    let said =
        |bundle: Entry, archive: Entry| first_difference(&one(bundle), &one(archive)).unwrap();
    let at = "ConsensFlow.app/a differs: ";
    assert_eq!(
        said(folder(0o755), plain(0o755, 0, EMPTY)),
        format!("{at}a folder in the bundle, a file in the archive")
    );
    assert_eq!(
        said(plain(0o755, 0, EMPTY), folder(0o755)),
        format!("{at}a file in the bundle, a folder in the archive")
    );
    assert_eq!(
        said(plain(0o755, 6, HELLO), plain(0o644, 6, HELLO)),
        format!("{at}mode 755 in the bundle, 644 in the archive")
    );
    assert_eq!(
        said(folder(0o755), folder(0o700)),
        format!("{at}mode 755 in the bundle, 700 in the archive")
    );
    assert_eq!(
        said(plain(0o644, 6, HELLO), plain(0o4644, 6, HELLO)),
        format!("{at}mode 644 in the bundle, 4644 in the archive")
    );
    assert_eq!(
        said(plain(0o644, 6, HELLO), plain(0o644, 0, EMPTY)),
        format!("{at}6 bytes in the bundle, 0 in the archive")
    );
    assert_eq!(
        said(plain(0o644, 6, HELLO), plain(0o644, 6, EMPTY)),
        format!("{at}the bytes differ")
    );
}

#[test]
fn the_difference_told_is_the_first_in_the_order_of_the_names() {
    let bundle = tree(&[
        ("ConsensFlow.app/b", plain(0o644, 6, HELLO)),
        ("ConsensFlow.app/a", plain(0o644, 6, HELLO)),
        ("ConsensFlow.app/c", folder(0o755)),
    ]);
    let archive = tree(&[
        ("ConsensFlow.app/b", plain(0o600, 6, HELLO)),
        ("ConsensFlow.app/a", plain(0o644, 6, HELLO)),
        ("ConsensFlow.app/c", folder(0o700)),
    ]);
    assert_eq!(
        first_difference(&bundle, &archive).unwrap(),
        "ConsensFlow.app/b differs: mode 644 in the bundle, 600 in the archive"
    );
}

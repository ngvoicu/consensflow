//! The copies' folder, on homes made here: each copy a new folder of its own,
//! the old ones removed at the next opening, and a link left on the way to the
//! folder, or in it, never followed: nothing where it leads is removed or
//! written.

use std::fs;
use std::time::UNIX_EPOCH;

use super::*;

fn read(files: &[(&[&str], &str)]) -> ForBrowser {
    ForBrowser {
        files: files
            .iter()
            .map(|(at, text)| {
                (
                    at.iter().map(|name| (*name).to_owned()).collect(),
                    text.as_bytes().to_vec(),
                )
            })
            .collect(),
    }
}

/// Where the copies of the home `home` are made.
fn root(home: &Path) -> PathBuf {
    home.join("tmp").join("specs-open")
}

/// A file at `path` last written in 2020: older than any copy is kept.
fn old_file(path: &Path) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, "the human's").unwrap();
    fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(UNIX_EPOCH + Duration::from_secs(1_577_836_800))
        .unwrap();
}

#[test]
fn a_copy_is_a_new_folder_of_its_own_with_its_files_where_they_were() {
    let home = tempfile::tempdir().unwrap();
    let given = read(&[
        (&["checkout", "SPEC.html"], "<p>spec</p>"),
        (&["assets", "spec-styles.css"], "p {}"),
    ]);
    let html = place(home.path(), &given, KEPT_FOR).unwrap();
    let copy = html.parent().unwrap().parent().unwrap();
    assert_eq!(copy.parent().unwrap(), root(home.path()));
    assert!(html.ends_with("checkout/SPEC.html"));
    assert_eq!(fs::read_to_string(&html).unwrap(), "<p>spec</p>");
    assert_eq!(
        fs::read_to_string(copy.join("assets/spec-styles.css")).unwrap(),
        "p {}"
    );
    let again = place(home.path(), &given, KEPT_FOR).unwrap();
    assert_ne!(again, html, "each opening its own copy");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(copy), 0o700);
        assert_eq!(mode(&root(home.path())), 0o700);
        assert_eq!(mode(&html), 0o600);
    }
}

/// A copy is kept as long as it may be read, and removed at the first
/// opening after; what cannot be read for the browser makes no copy.
#[test]
fn copies_kept_their_time_are_removed_at_the_next_opening() {
    let home = tempfile::tempdir().unwrap();
    let given = read(&[(&["SPEC.html"], "<p>spec</p>")]);
    let first = place(home.path(), &given, KEPT_FOR).unwrap();
    let second = place(home.path(), &given, KEPT_FOR).unwrap();
    assert!(first.exists(), "kept a day");
    let third = place(home.path(), &given, Duration::ZERO).unwrap();
    assert!(!first.exists() && !second.exists(), "older copies removed");
    assert!(third.exists(), "this opening's copy made after");
    assert_eq!(fs::read_dir(root(home.path())).unwrap().count(), 1);
    assert!(place(home.path(), &read(&[]), KEPT_FOR).is_err());
}

/// What an agent may leave behind, with no race: `tmp/specs-open`, or `tmp`,
/// a link to a folder of the human's. The opening says it cannot copy, and
/// nothing where the link leads is removed or written, an old file included.
#[cfg(unix)]
#[test]
fn a_link_left_on_the_way_to_the_copies_is_refused_and_where_it_leads_is_left_alone() {
    use std::os::unix::fs::symlink;

    let given = read(&[(&["SPEC.html"], "<p>spec</p>")]);
    for (planted, at) in [("tmp/specs-open", "tmp/specs-open"), ("tmp", "tmp")] {
        let home = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let sentinel = outside.path().join("old.txt");
        old_file(&sentinel);
        // Where `tmp` leads, the sweep would read its `specs-open`.
        old_file(&outside.path().join("specs-open/old.txt"));
        let link = home.path().join(planted);
        fs::create_dir_all(link.parent().unwrap()).unwrap();
        symlink(outside.path(), &link).unwrap();
        assert_eq!(
            place(home.path(), &given, KEPT_FOR).unwrap_err(),
            format!(
                "no copy for the browser was made: {} is a link, and no copy is made where a \
                 link leads",
                home.path().join(at).display()
            ),
            "{planted}"
        );
        assert!(sentinel.exists(), "{planted}: the old file where it leads");
        assert!(
            outside.path().join("specs-open/old.txt").exists(),
            "{planted}"
        );
        assert_eq!(
            fs::read_dir(outside.path()).unwrap().count(),
            2,
            "{planted}: nothing written there"
        );
    }
}

/// A link in the copies' folder, or in an old copy, is removed as a link when
/// its time is up: what it leads to is never emptied.
#[cfg(unix)]
#[test]
fn a_link_in_the_copies_folder_is_removed_as_itself_and_never_followed() {
    use std::os::unix::fs::symlink;

    let home = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let sentinel = outside.path().join("old.txt");
    old_file(&sentinel);
    let given = read(&[(&["SPEC.html"], "<p>spec</p>")]);
    let first = place(home.path(), &given, KEPT_FOR).unwrap();
    let copy = first.parent().unwrap();
    symlink(outside.path(), copy.join("inner")).unwrap();
    symlink(outside.path(), root(home.path()).join("left")).unwrap();
    place(home.path(), &given, Duration::ZERO).unwrap();
    assert!(!copy.exists(), "the old copy removed");
    assert!(
        fs::symlink_metadata(root(home.path()).join("left")).is_err(),
        "the link removed"
    );
    assert!(sentinel.exists(), "and nothing where they lead");
}

/// The same on Windows, with a junction, which needs no administrator:
/// `mklink /J` makes `tmp\specs-open` lead to a folder of the human's.
#[cfg(windows)]
#[test]
#[allow(clippy::disallowed_methods)] // The test makes the junction it is about.
fn a_junction_left_on_the_way_to_the_copies_is_refused_and_where_it_leads_is_left_alone() {
    let home = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let sentinel = outside.path().join("old.txt");
    old_file(&sentinel);
    fs::create_dir_all(home.path().join("tmp")).unwrap();
    let junction = root(home.path());
    let made = std::process::Command::new("cmd")
        .args(["/c", "mklink", "/J"])
        .arg(&junction)
        .arg(outside.path())
        .output()
        .unwrap();
    assert!(made.status.success(), "{made:?}");
    let given = read(&[(&["SPEC.html"], "<p>spec</p>")]);
    assert_eq!(
        place(home.path(), &given, KEPT_FOR).unwrap_err(),
        format!(
            "no copy for the browser was made: {} is a link, and no copy is made where a link \
             leads",
            junction.display()
        )
    );
    assert!(sentinel.exists());
    assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 1);
}

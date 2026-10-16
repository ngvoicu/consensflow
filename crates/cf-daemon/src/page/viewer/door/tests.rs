//! The door, held to what it promises on folders made here: a name is opened
//! as itself, through the folder above it, and what was opened is what is
//! checked, whatever a path names a moment later. Links, FIFOs and hard links
//! are Unix's to make in a test; what Windows's sharing keeps out of a held
//! folder is Windows's own test, run there (`cargo test -p cf-daemon --lib
//! viewer` on zeewin).

use std::fs;
use std::io::Read;
use std::path::Path;

use super::*;

fn folder_with(files: &[(&str, &str)]) -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    for (path, text) in files {
        let file = root.path().join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, text).unwrap();
    }
    root
}

/// The folder at `root`, held as a project's is, in a look that may read any
/// number of names.
fn held(root: &Path) -> Held {
    let reading = Reading::new();
    reading.begin(usize::MAX);
    Held::project(root, &reading).unwrap_or_else(|_| panic!("{} is held", root.display()))
}

/// The names `held` lists, and what each is; none for those passed over.
fn names_of(held: &Held) -> Vec<Option<(String, Kind)>> {
    held.entries().unwrap().map(Result::unwrap).collect()
}

fn text_of(opened: Opened) -> String {
    let mut text = String::new();
    { opened.file }.read_to_string(&mut text).unwrap();
    text
}

#[test]
fn a_file_is_opened_through_the_folders_above_it_and_read_as_it_is() {
    let root = folder_with(&[("a/b/SPEC.md", "# B")]);
    let folder = held(root.path()).folder("a").unwrap().folder("b").unwrap();
    let opened = folder.file("SPEC.md", &|| {}).unwrap();
    assert_eq!(opened.size, 3);
    assert_eq!(text_of(opened), "# B");
    assert!(matches!(
        folder.file("gone.md", &|| {}),
        Err(Refused::Missing)
    ));
    assert!(matches!(
        held(root.path()).file("a", &|| {}),
        Err(Refused::Folder)
    ));
    assert!(matches!(
        held(root.path())
            .folder("a")
            .unwrap()
            .folder("b")
            .unwrap()
            .folder("SPEC.md"),
        Err(Refused::NotFolder)
    ));
}

#[test]
fn a_folder_lists_its_names_with_what_each_is_and_never_its_own_two() {
    let root = folder_with(&[("a/x.md", "12345"), ("b.md", "1")]);
    let mut names: Vec<(String, Kind)> =
        names_of(&held(root.path())).into_iter().flatten().collect();
    names.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(
        names,
        [
            ("a".to_owned(), Kind::Folder),
            ("b.md".to_owned(), Kind::File { size: 1 })
        ]
    );
}

/// A name the page could not name is passed over, and is still a name the
/// folder gave: a walk counts it, so a folder of a million of them costs what
/// its budget allows and no more.
#[test]
fn a_name_passed_over_is_still_a_name_read() {
    let root = folder_with(&[("a.md", "1"), ("x1", "1"), ("x2", "1"), ("x3", "1")]);
    UNNAMED.with(|unnamed| unnamed.set(|name| name.starts_with('x')));
    let names = names_of(&held(root.path()));
    UNNAMED.with(|unnamed| unnamed.set(named));
    let passed = names.iter().filter(|named| named.is_none()).count();
    // Unix lists a folder's own two (`.` and `..`) too, passed over the same.
    assert_eq!(passed, if cfg!(unix) { 5 } else { 3 }, "{names:?}");
    assert_eq!(
        names.into_iter().flatten().collect::<Vec<_>>(),
        [("a.md".to_owned(), Kind::File { size: 1 })]
    );
}

/// What a held folder keeps out on Windows, where sharing is what holds it: it
/// cannot be moved aside (and a link put in its place) while it is held, and
/// what the human does in it goes on: a file saved there, and a file the door
/// is reading replaced by an editor's save.
#[cfg(windows)]
#[test]
fn a_held_folder_is_not_moved_aside_and_a_save_in_it_goes_on() {
    let root = folder_with(&[("specs/a/SPEC.md", "# A")]);
    let at = |path: &str| root.path().join(path);
    let project = held(root.path());
    let specs = project.folder("specs").unwrap();
    let a = specs.folder("a").unwrap();
    assert!(
        fs::rename(at("specs/a"), at("specs/moved")).is_err(),
        "the folder held"
    );
    assert!(
        fs::rename(at("specs"), at("moved")).is_err(),
        "a folder above it"
    );
    fs::write(at("specs/a/SPEC.md.tmp"), "# A, saved").unwrap();
    fs::rename(at("specs/a/SPEC.md.tmp"), at("specs/a/SPEC.md")).unwrap();
    let reading = a.file("SPEC.md", &|| {}).unwrap();
    fs::write(at("specs/a/SPEC.md.tmp"), "# A, saved again").unwrap();
    fs::rename(at("specs/a/SPEC.md.tmp"), at("specs/a/SPEC.md")).unwrap();
    assert_eq!(text_of(reading), "# A, saved", "the file that was opened");
    drop((a, specs, project));
    fs::rename(at("specs/a"), at("specs/moved")).unwrap();
}

#[cfg(unix)]
mod unix {
    use std::os::unix::fs::symlink;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    use nix::sys::stat::Mode as Permissions;
    use nix::unistd::mkfifo;

    use super::*;

    #[test]
    fn a_link_is_refused_as_a_file_and_as_a_folder_wherever_it_leads() {
        let root = folder_with(&[("a/SPEC.md", "# A")]);
        let outside = folder_with(&[("secret.env", "TOKEN=1")]);
        symlink(
            root.path().join("a/SPEC.md"),
            root.path().join("a/alias.md"),
        )
        .unwrap();
        symlink(
            outside.path().join("secret.env"),
            root.path().join("a/key.md"),
        )
        .unwrap();
        symlink(outside.path(), root.path().join("away")).unwrap();
        let top = held(root.path());
        let a = top.folder("a").unwrap();
        assert!(
            matches!(a.file("alias.md", &|| {}), Err(Refused::Link)),
            "even one that stays inside"
        );
        assert!(matches!(a.file("key.md", &|| {}), Err(Refused::Link)));
        assert!(matches!(top.folder("away"), Err(Refused::Link)));
        assert!(
            names_of(&a).contains(&Some(("alias.md".to_owned(), Kind::Link))),
            "listed as a link"
        );
        // The project's own folder is the human's: one that is a link is followed.
        let linked = tempfile::tempdir().unwrap();
        symlink(root.path(), linked.path().join("project")).unwrap();
        let project = held(&linked.path().join("project"));
        assert_eq!(
            text_of(
                project
                    .folder("a")
                    .unwrap()
                    .file("SPEC.md", &|| {})
                    .unwrap()
            ),
            "# A"
        );
    }

    /// The race the door closes: a file looked at, then swapped for a link to
    /// a file outside before it is opened. What is opened is the name as it is
    /// then, never followed: the read refuses, and nothing outside is read.
    #[test]
    fn a_file_swapped_for_a_link_between_its_look_and_its_opening_is_refused() {
        let root = folder_with(&[("a/SPEC.md", "safe")]);
        let outside = folder_with(&[("secret.env", "TOKEN=1")]);
        let a = held(root.path()).folder("a").unwrap();
        let swap = || {
            fs::remove_file(root.path().join("a/SPEC.md")).unwrap();
            symlink(
                outside.path().join("secret.env"),
                root.path().join("a/SPEC.md"),
            )
            .unwrap();
        };
        assert!(matches!(a.file("SPEC.md", &swap), Err(Refused::Link)));
    }

    /// The same race one level up: the folder on the way swapped for a link to
    /// a folder outside, after the folder above it was held. The file is opened
    /// through the folder held, which is the one that was looked at.
    #[test]
    fn a_folder_swapped_for_a_link_while_a_file_in_it_is_opened_is_not_followed() {
        let root = folder_with(&[("a/SPEC.md", "safe")]);
        let outside = folder_with(&[("SPEC.md", "TOKEN=1")]);
        let a = held(root.path()).folder("a").unwrap();
        let swap = || {
            fs::rename(root.path().join("a"), root.path().join("moved")).unwrap();
            symlink(outside.path(), root.path().join("a")).unwrap();
        };
        // The folder held is the one moved aside: its file is the safe one.
        assert_eq!(text_of(a.file("SPEC.md", &swap).unwrap()), "safe");
        // Opened again from the top now, the link is refused.
        assert!(matches!(held(root.path()).folder("a"), Err(Refused::Link)));
    }

    /// A FIFO is never waited on: one that is there is refused as what it is, and
    /// a file swapped for one between its look and its opening is refused too,
    /// at once, with no writer ever coming.
    #[test]
    fn a_fifo_is_refused_without_waiting_for_a_writer() {
        let root = folder_with(&[("a/notes.md", "notes")]);
        mkfifo(
            &root.path().join("a/pipe.md"),
            Permissions::S_IRUSR | Permissions::S_IWUSR,
        )
        .unwrap();
        let a = held(root.path()).folder("a").unwrap();
        let (done, finished) = mpsc::channel();
        let path = root.path().to_path_buf();
        thread::spawn(move || {
            let a = held(&path).folder("a").unwrap();
            let there = matches!(a.file("pipe.md", &|| {}), Err(Refused::Other("a FIFO")));
            let swap = || {
                fs::remove_file(path.join("a/notes.md")).unwrap();
                mkfifo(
                    &path.join("a/notes.md"),
                    Permissions::S_IRUSR | Permissions::S_IWUSR,
                )
                .unwrap();
            };
            let swapped = matches!(a.file("notes.md", &swap), Err(Refused::Other("a FIFO")));
            done.send((there, swapped)).unwrap();
        });
        let (there, swapped) = finished
            .recv_timeout(Duration::from_secs(5))
            .expect("no FIFO is waited on");
        assert!(there, "a FIFO that is there");
        assert!(swapped, "a file swapped for a FIFO");
        assert!(
            names_of(&a).contains(&Some(("pipe.md".to_owned(), Kind::Other("a FIFO")))),
            "listed as what it is"
        );
    }

    /// A file of two names may be a file outside under its other name: it is
    /// refused, and says how many names it has.
    #[test]
    fn a_file_of_more_than_one_name_is_refused() {
        let root = folder_with(&[("a/notes.md", "notes")]);
        let outside = folder_with(&[("secret.env", "TOKEN=1")]);
        let leak = root.path().join("a/leak.md");
        if fs::hard_link(outside.path().join("secret.env"), &leak).is_err() {
            // Two temporary folders on two volumes: a link inside one is made instead.
            fs::hard_link(root.path().join("a/notes.md"), &leak).unwrap();
        }
        let a = held(root.path()).folder("a").unwrap();
        assert!(matches!(a.file("leak.md", &|| {}), Err(Refused::Names(2))));
    }
}

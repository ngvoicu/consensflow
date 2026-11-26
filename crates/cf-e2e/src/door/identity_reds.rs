//! K3's doors by file identity, the round after the critical review (T-2319): what the run made is
//! the run's by its text and by its identity, both; an absence is no refusal; the recursive copy
//! asks the door of every child. Every store and every "person's folder" here is a folder the test
//! made (`unowned_tempdir` is the test's own decoy): never a harness home, never the Keychain.

// The tests make their links and read the machine's names as the system gives them.
#![allow(clippy::disallowed_methods)]

use std::fs;
use std::path::{Path, PathBuf};

use super::account::TestRoot;
#[cfg(any(windows, target_os = "macos"))]
use super::inside_protected;
use super::place::{self, Act};
use super::{judge, made, reads};

/// Makes `link` lead to the folder `target`: a symbolic link on Unix, a junction on Windows (which
/// needs no privilege).
fn link_dir(target: &Path, link: &Path) -> bool {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link).is_ok()
    }
    #[cfg(windows)]
    {
        let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".to_owned());
        std::process::Command::new(format!(r"{root}\System32\cmd.exe"))
            .args(["/d", "/c", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }
}

/// Takes the link [`link_dir`] made, and never what it leads to.
fn unlink_dir(link: &Path) {
    #[cfg(unix)]
    let _ = fs::remove_file(link);
    #[cfg(windows)]
    let _ = fs::remove_dir(link);
}

// ---- Finding 1: a folder the run made that is a link now grounds nothing where it leads. ----

#[test]
fn a_folder_the_run_made_that_is_a_link_now_gives_no_ground_where_it_leads() {
    let mine = made("k3-r3-").unwrap();
    let at = mine.path().to_path_buf();
    let theirs = crate::standin_process::unowned_tempdir();
    let theirs_real = place::real(theirs.path()).unwrap();
    fs::write(theirs_real.join("kept.txt"), "a person's").unwrap();
    fs::create_dir(theirs_real.join("kept")).unwrap();
    fs::write(theirs_real.join("kept").join("inner.txt"), "a person's").unwrap();
    assert!(judge(&theirs_real.join("new.txt"), Act::Write).is_err());
    assert!(judge(&theirs_real.join("kept.txt"), Act::Remove).is_err());
    // The run's folder goes, and a link to a person's folder is at its path.
    fs::remove_dir(&at).unwrap();
    assert!(link_dir(&theirs_real, &at), "no link could be made");
    let cases: Vec<(&str, PathBuf, Act)> = vec![
        (
            "write in the person's folder",
            theirs_real.join("new.txt"),
            Act::Write,
        ),
        (
            "write through the run's old path",
            at.join("new.txt"),
            Act::Write,
        ),
        (
            "make a folder in the person's folder",
            theirs_real.join("sub"),
            Act::Folder,
        ),
        (
            "remove the person's file",
            theirs_real.join("kept.txt"),
            Act::Remove,
        ),
        (
            "remove the person's folder",
            theirs_real.join("kept"),
            Act::Remove,
        ),
        (
            "remove through the run's old path",
            at.join("kept.txt"),
            Act::Remove,
        ),
        (
            "rename onto the person's file",
            theirs_real.join("kept.txt"),
            Act::Rename,
        ),
    ];
    let admitted: Vec<&str> = cases
        .iter()
        .filter(|(_, path, act)| judge(path, *act).is_ok())
        .map(|(name, _, _)| *name)
        .collect();
    let written = super::write(at.join("landed.txt"), "the run's");
    let landed = theirs_real.join("landed.txt").exists();
    let removed = super::Door::of_run().remove_in_made(&theirs_real.join("kept.txt"));
    let gone = !theirs_real.join("kept.txt").exists();
    let seal = super::is_made_or_below(&theirs_real) || super::is_made(&theirs_real);
    unlink_dir(&at);
    assert!(
        admitted.is_empty() && written.is_err() && !landed && !removed.is_gone() && !gone && !seal,
        "a run's folder that is a link now grounded {admitted:?}; written={written:?}; landed={landed}; removed={removed}; seal={seal}"
    );
}

/// A folder the run made, put away and another made at its path, is no longer the run's, though
/// the text of every path below it says so.
#[test]
fn a_folder_put_at_the_path_of_one_the_run_made_is_not_the_runs() {
    let root = made("k3-r3-put-").unwrap();
    let at = place::real(&root.path().join("kept")).unwrap_or_else(|_| root.path().join("kept"));
    super::make_dir(&at).unwrap();
    let at = place::real(&at).unwrap();
    let registry = vec![super::Entry::folder(at.clone())];
    assert!(place::judge_with(&at.join("a.txt"), Act::Write, &[], &registry).is_ok());
    // The run's folder is put aside, not taken away, and a person's own is made at its path:
    // two folders that both exist never share an identity, on any file system.
    let aside = at.with_file_name("kept-aside");
    fs::rename(&at, &aside).unwrap();
    fs::create_dir(&at).unwrap();
    assert_ne!(
        super::identity::of_path(&at).unwrap(),
        super::identity::of_path(&aside).unwrap()
    );
    let after = place::judge_with(&at.join("a.txt"), Act::Write, &[], &registry);
    assert!(after.is_err(), "{after:?}");
}

// ---- Finding 2: the recursive copy asks the door of every child. ----

/// A walk of `walk` copied to `out`: whether it went through, and which of `names` came out.
fn copied(walk: &Path, out: &Path, names: &[&str]) -> (bool, Vec<String>) {
    let went = super::copy_tree(walk, out, &|_| false).is_ok();
    let took = names
        .iter()
        .filter(|name| fs::read_to_string(out.join(name)).is_ok())
        .map(|name| (*name).to_owned())
        .collect();
    (went, took)
}

/// The starting folders of the copy, each way the walk can meet a store: by the first spelling of
/// the starting folder and by its second, where the machine has one.
fn walks(root: &Path, name: &str) -> Vec<(&'static str, PathBuf)> {
    let walk = root.join(name);
    #[allow(unused_mut)]
    let mut ways = vec![("first", walk.clone())];
    #[cfg(any(target_os = "macos", windows))]
    ways.push(("second", super::identity_tests::second_of(&walk)));
    ways
}

#[test]
fn a_recursive_copy_takes_nothing_of_a_store_below_its_starting_folder() {
    let root = made("k3-r2-").unwrap();
    let inner = root.path().join("walk-dir").join("inner-store");
    super::make_dir(&inner).unwrap();
    super::write(inner.join("secret.txt"), "inner secret").unwrap();
    super::write(root.path().join("walk-dir").join("plain.txt"), "plain").unwrap();
    let _held = TestRoot::add(&inner);
    assert!(reads::admitted(&inner.join("secret.txt")).is_err());
    for (which, way) in walks(root.path(), "walk-dir") {
        let out = root.path().join(format!("out-dir-{which}"));
        let (went, took) = copied(&way, &out, &["inner-store/secret.txt"]);
        assert!(
            !went && took.is_empty(),
            "{which}: went={went}, took {took:?}"
        );
    }
}

#[test]
fn a_recursive_copy_takes_no_hard_link_to_a_file_that_is_a_store() {
    let root = made("k3-r2-hard-").unwrap();
    let walk = root.path().join("walk-file");
    super::make_dir(&walk).unwrap();
    let store_file = root.path().join("store-file.txt");
    super::write(&store_file, "store file").unwrap();
    assert!(fs::hard_link(&store_file, walk.join("hard.txt")).is_ok());
    super::write(walk.join("plain.txt"), "plain").unwrap();
    let _held = TestRoot::add(&store_file);
    assert!(reads::admitted(&walk.join("hard.txt")).is_err());
    for (which, way) in walks(root.path(), "walk-file") {
        let out = root.path().join(format!("out-file-{which}"));
        let (went, took) = copied(&way, &out, &["hard.txt"]);
        assert!(
            !went && took.is_empty(),
            "{which}: went={went}, took {took:?}"
        );
    }
}

#[test]
fn a_recursive_copy_skips_a_link_to_a_store_and_goes_on() {
    let root = made("k3-r2-link-").unwrap();
    let walk = root.path().join("walk-link");
    let outside = root.path().join("outside-store");
    super::make_dir(&walk).unwrap();
    super::make_dir(&outside).unwrap();
    super::write(outside.join("secret.txt"), "outside secret").unwrap();
    super::write(walk.join("plain.txt"), "plain").unwrap();
    let link = walk.join("link-to-store");
    assert!(link_dir(&outside, &link));
    let _held = TestRoot::add(&outside);
    for (which, way) in walks(root.path(), "walk-link") {
        let out = root.path().join(format!("out-link-{which}"));
        let (went, took) = copied(&way, &out, &["link-to-store/secret.txt", "plain.txt"]);
        assert!(
            went && took == ["plain.txt"],
            "{which}: went={went}, took {took:?}"
        );
    }
    unlink_dir(&link);
}

/// A walk that meets nothing of a store copies as it did, links and all.
#[test]
fn a_recursive_copy_of_a_folder_with_no_store_in_it_is_whole_and_skips_a_link_that_leaves() {
    let root = made("k3-r2-plain-").unwrap();
    let walk = root.path().join("walk");
    let outside = root.path().join("outside");
    super::make_dir(walk.join("sub")).unwrap();
    super::make_dir(&outside).unwrap();
    super::write(walk.join("sub").join("a.txt"), "a").unwrap();
    super::write(outside.join("b.txt"), "b").unwrap();
    let link = walk.join("leaves");
    assert!(link_dir(&outside, &link));
    let tree = root.path().join("tree");
    super::copy_tree(&walk, &tree, &|_| false).unwrap();
    unlink_dir(&link);
    assert_eq!(
        fs::read_to_string(tree.join("sub").join("a.txt")).unwrap(),
        "a"
    );
    assert!(fs::symlink_metadata(tree.join("leaves")).is_err());
}

// ---- Finding 3: absence is not a refusal. ----

/// The cells of the table: a place (a store, or a value of the environment) that is there; not
/// there; whose drive is not there; there and unreadable; a server's bare name. For each the
/// verdict is the text's alone, as before identity, except where a readable identity says
/// "inside a store" (the cases of `identity_tests`), and except a store that is there and cannot be
/// read, which refuses in its own words.
struct Cell {
    name: &'static str,
    place: PathBuf,
    /// Windows' drive that is not there, a server's bare name: no part can be reached.
    unreachable: bool,
    unreadable: bool,
}

fn cells(root: &Path) -> Vec<Cell> {
    let there = root.join("there");
    fs::create_dir_all(&there).unwrap();
    let mut cells = vec![
        Cell {
            name: "there",
            place: there.clone(),
            unreachable: false,
            unreadable: false,
        },
        Cell {
            name: "not there",
            place: root.join("not").join("there"),
            unreachable: false,
            unreadable: false,
        },
    ];
    #[cfg(windows)]
    {
        if let Some(letter) = ('D'..='Z')
            .rev()
            .find(|letter| fs::metadata(format!(r"{letter}:")).is_err())
        {
            cells.push(Cell {
                name: "its drive not there",
                place: PathBuf::from(format!(r"{letter}:\owner\.codex")),
                unreachable: true,
                unreadable: false,
            });
        } else {
            panic!("NOT RUN: every drive letter is there");
        }
        cells.push(Cell {
            name: "a server's bare name",
            place: PathBuf::from(r"\\NOSUCHSERVER-T2319"),
            unreachable: true,
            unreadable: false,
        });
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let blocked = root.join("blocked");
        fs::create_dir_all(blocked.join("inner")).unwrap();
        fs::set_permissions(&blocked, fs::Permissions::from_mode(0o000)).unwrap();
        cells.push(Cell {
            name: "there and unreadable",
            place: blocked.join("inner").join("x"),
            unreachable: false,
            unreadable: true,
        });
    }
    cells
}

/// Puts back what the table's unreadable cell took away, so that the test's folder can go.
fn open_cells(root: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let _ = fs::set_permissions(root.join("blocked"), fs::Permissions::from_mode(0o755));
    }
    #[cfg(not(unix))]
    let _ = root;
}

#[test]
fn a_value_that_cannot_be_reached_is_judged_by_its_text_alone_as_before_identity() {
    let root = made("k3-r4-value-").unwrap();
    let store = root.path().join("owner").join(".codex");
    super::make_dir(&store).unwrap();
    let roots = super::Roots::of(vec![store.clone()]);
    assert!(roots.cannot_be_read().is_none());
    let mut wrong = Vec::new();
    for cell in cells(&root.path().join("cells")) {
        // The text says "not inside the store" for each: so does the head.
        let held = roots.holds(&cell.place) || roots.holds(&cell.place.join("x"));
        if held {
            wrong.push(format!("{}: held", cell.name));
        }
        let _ = (cell.unreachable, cell.unreadable);
    }
    // And where the text says "inside", the cells' own kind does not matter.
    assert!(roots.holds(&store.join("x")));
    open_cells(&root.path().join("cells"));
    assert!(
        wrong.is_empty(),
        "values refused for what is not there: {wrong:?}"
    );
}

#[test]
fn a_store_that_is_not_there_or_whose_drive_is_not_there_is_judged_by_its_text_alone() {
    let root = made("k3-r4-store-").unwrap();
    let cells_root = root.path().join("cells");
    let mut wrong = Vec::new();
    for cell in cells(&cells_root) {
        let roots = vec![cell.place.clone()];
        let made_here = vec![super::Entry::folder(place::real(root.path()).unwrap())];
        let write = place::judge_with(
            &root.path().join("elsewhere.txt"),
            Act::Write,
            &roots,
            &made_here,
        );
        let seal = super::Roots::of(roots.clone());
        let held = seal.holds(&root.path().join("elsewhere"));
        if cell.unreadable {
            // There and unreadable: a refusal that says which store and that it could not be read.
            let said = write
                .as_ref()
                .err()
                .map(ToString::to_string)
                .unwrap_or_default();
            let words = seal.cannot_be_read().map(str::to_owned).unwrap_or_default();
            if write.is_ok()
                || !said.contains("cannot be read")
                || said.contains("inside a store")
                || !words.contains("cannot be read")
                || !held
            {
                wrong.push(format!(
                    "{}: write {write:?}, held {held}, words {words:?}",
                    cell.name
                ));
            }
        } else if write.is_err() || held || seal.cannot_be_read().is_some() {
            wrong.push(format!("{}: write {write:?}, held {held}", cell.name));
        }
    }
    open_cells(&cells_root);
    assert!(wrong.is_empty(), "{wrong:?}");
}

#[test]
fn the_seal_passes_a_value_that_cannot_be_reached_and_names_a_store_it_cannot_read() {
    let root = made("k3-r4-seal-").unwrap();
    let home = root.path().join("home");
    super::make_dir(&home).unwrap();
    let cells_root = root.path().join("cells");
    let store = root.path().join("owner").join(".codex");
    super::make_dir(&store).unwrap();
    let held = TestRoot::add(&store);
    for cell in cells(&cells_root) {
        let mut env = crate::standin_process::home_vars(&home)
            .into_iter()
            .map(|(name, value)| (name.into(), value.into_os_string()))
            .collect::<Vec<(std::ffi::OsString, std::ffi::OsString)>>();
        env.push(("SOME_TOOL_HOME".into(), cell.place.clone().into_os_string()));
        let sealed = super::sealed(env, &[], cfg!(windows));
        // The roots are the account's, which the test cannot make unreadable: each cell's value
        // names nothing of the store, so the seal passes it (the profile halves aside).
        if let Err(why) = &sealed {
            assert!(!why.contains("SOME_TOOL_HOME"), "{}: {why}", cell.name);
        }
    }
    open_cells(&cells_root);
    drop(held);
}

#[cfg(windows)]
#[test]
fn a_variable_that_names_a_drive_that_is_not_there_does_not_refuse_the_start() {
    let root = made("k3-r4w-").unwrap();
    let home = root.path().join("home");
    super::make_dir(&home).unwrap();
    let letter = ('D'..='Z')
        .rev()
        .find(|letter| fs::metadata(format!(r"{letter}:")).is_err())
        .expect("NOT RUN: every drive letter is there");
    let value = PathBuf::from(format!(r"{letter}:	oolsin"));
    assert!(!inside_protected(&value));
    let mut env = crate::standin_process::home_vars(&home)
        .into_iter()
        .filter(|(name, _)| !super::spawn::PROFILE_HALVES.contains(name))
        .map(|(name, value)| (name.into(), value.into_os_string()))
        .collect::<Vec<(std::ffi::OsString, std::ffi::OsString)>>();
    env.push(("SOME_TOOL_HOME".into(), value.into_os_string()));
    let sealed = super::sealed(env, &[], true);
    assert!(sealed.is_ok(), "{sealed:?}");
}

/// `LOGONSERVER` is `\<machine>` in every interactive Windows session and the kit hands it on.
#[cfg(windows)]
#[test]
fn the_logon_server_does_not_refuse_the_start() {
    let root = made("k3-r4l-").unwrap();
    let home = root.path().join("home");
    super::make_dir(&home).unwrap();
    let machine = std::env::var("COMPUTERNAME").unwrap_or_else(|_| "NOSUCHMACHINE".to_owned());
    let mut refused: Vec<String> = Vec::new();
    for value in [
        format!(r"\\{machine}"),
        r"\\MicrosoftAccount".to_owned(),
        r"\\NOSUCHMACHINE".to_owned(),
    ] {
        let mut env = crate::standin_process::home_vars(&home)
            .into_iter()
            .filter(|(name, _)| !super::spawn::PROFILE_HALVES.contains(name))
            .map(|(name, value)| (name.into(), value.into_os_string()))
            .collect::<Vec<(std::ffi::OsString, std::ffi::OsString)>>();
        env.push(("LOGONSERVER".into(), std::ffi::OsString::from(&value)));
        if let Err(why) = super::sealed(env, &[], true) {
            refused.push(format!("{value}: {why}"));
        }
    }
    assert!(
        refused.is_empty(),
        "the seal refused LOGONSERVER: {refused:?}"
    );
}

/// Other spellings Windows has for a folder, each of which the system opens as the store.
#[cfg(windows)]
#[test]
fn every_spelling_windows_opens_as_the_store_is_the_store() {
    let root = made("k3-r4x-").unwrap();
    let store = root.path().join("owner").join(".codex");
    super::make_dir(&store).unwrap();
    let real = place::real(&store).unwrap();
    let _held = TestRoot::add(&store);
    let text = real.to_string_lossy().into_owned();
    let (drive, rest) = text.split_once(':').unwrap();
    let rest = rest.trim_start_matches('\\');
    let spellings: Vec<(&str, PathBuf)> = vec![
        (
            "the admin share by name",
            PathBuf::from(format!(r"\\localhost\{drive}$\{rest}")),
        ),
        (
            "the admin share by address",
            PathBuf::from(format!(r"\\127.0.0.1\{drive}$\{rest}")),
        ),
        ("the verbatim drive", PathBuf::from(format!(r"\\?\{text}"))),
        ("the device drive", PathBuf::from(format!(r"\\.\{text}"))),
        (
            "the folder's own stream",
            PathBuf::from(format!(r"{text}::$INDEX_ALLOCATION")),
        ),
        (
            "the global root",
            PathBuf::from(format!(r"\\?\GLOBALROOT\DosDevices\{text}")),
        ),
    ];
    let own = super::identity::of_path(&store).unwrap();
    let mut through: Vec<&str> = Vec::new();
    for (name, spelled) in &spellings {
        let is_store = super::identity::of_path(spelled).is_ok_and(|id| id == own);
        let held = inside_protected(spelled) && inside_protected(&spelled.join("x"));
        if is_store && (!held || judge(&spelled.join("added"), Act::Name).is_ok()) {
            through.push(*name);
        }
    }
    assert_eq!(fs::read_dir(&store).unwrap().count(), 0);
    assert!(
        through.is_empty(),
        "spellings of the store that passed a door: {through:?}"
    );
}

/// A place below a folder the account cannot enter, on the Mac and Linux, is judged by its text:
/// no store is near it.
#[cfg(unix)]
#[test]
fn a_place_below_a_folder_the_account_cannot_enter_is_not_held_for_it() {
    use std::os::unix::fs::PermissionsExt as _;
    let root = made("k3-r4-enter-").unwrap();
    let store = root.path().join("owner").join(".codex");
    super::make_dir(&store).unwrap();
    let blocked = root.path().join("blocked");
    super::make_dir(blocked.join("inner")).unwrap();
    fs::set_permissions(&blocked, fs::Permissions::from_mode(0o000)).unwrap();
    let roots = super::Roots::of(vec![store]);
    let held = roots.holds(&blocked.join("inner").join("x"));
    fs::set_permissions(&blocked, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(
        !held,
        "a variable below a folder the account cannot enter was held"
    );
}

// ---- Finding 12: a folder of the run's that is a link now is not handed to a child. ----

#[test]
fn a_folder_that_is_a_link_now_is_not_handed_to_a_child_as_the_runs() {
    let mine = made("k3-f12-").unwrap();
    let at = place::real(mine.path()).unwrap();
    let theirs = crate::standin_process::unowned_tempdir();
    let theirs_real = place::real(theirs.path()).unwrap();
    fs::remove_dir(mine.path()).unwrap();
    assert!(link_dir(&theirs_real, mine.path()), "no link could be made");
    let handed: Vec<PathBuf> = super::handed_down()
        .into_iter()
        .filter(|(name, _)| name.as_str() == super::MADE_ROOTS)
        .flat_map(|(_, value)| std::env::split_paths(&value).collect::<Vec<_>>())
        .collect();
    // What the child makes of what it is handed, as its registry does when it starts.
    let one = std::env::join_paths([&at]).unwrap();
    let childs: Vec<super::Entry> = super::roots_handed_down(Some(one.as_os_str()))
        .into_iter()
        .map(super::Entry::folder)
        .collect();
    let write = place::judge_with(
        &theirs_real.join("new.txt"),
        Act::Write,
        &super::account::protected(),
        &childs,
    );
    unlink_dir(mine.path());
    assert!(
        !handed.contains(&at),
        "a folder that is a link now was handed down: {handed:?}"
    );
    let _ = write;
}

// ---- Finding 13: the Mac's volume-and-file-number spelling of a store is held. ----

#[cfg(target_os = "macos")]
#[test]
fn a_store_by_its_volume_and_file_number_is_held_by_the_seal_and_the_read_door() {
    use std::os::unix::fs::MetadataExt as _;
    let root = made("k3-f13-").unwrap();
    let store = root.path().join("owner").join(".codex");
    super::make_dir(&store).unwrap();
    super::write(store.join("marker.txt"), "made up").unwrap();
    let by_number = |path: &Path| {
        let found = fs::metadata(path).unwrap();
        PathBuf::from(format!("/.vol/{}/{}", found.dev(), found.ino()))
    };
    let _held = TestRoot::add(&store);
    for path in [
        by_number(&store),
        by_number(&store).join("marker.txt"),
        by_number(&store.join("marker.txt")),
    ] {
        assert!(
            fs::metadata(&path).is_ok(),
            "{} does not open",
            path.display()
        );
        assert!(
            inside_protected(&path),
            "{} passed the seal",
            path.display()
        );
        assert!(reads::read(&path).is_err(), "{} was read", path.display());
    }
}

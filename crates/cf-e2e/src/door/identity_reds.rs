//! K3's doors by file identity, the round after the critical review (T-2319): what the run made is
//! the run's by its text and by its identity, both; an absence is no refusal; the recursive copy
//! asks the door of every child. Every store and every "person's folder" here is a folder the test
//! made (`unowned_tempdir` is the test's own decoy): never a harness home, never the Keychain.

// The tests make their links and read the machine's names as the system gives them.
#![allow(clippy::disallowed_methods)]

use std::fs;
use std::path::{Path, PathBuf};

use super::account::TestRoot;
#[cfg(windows)]
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

// ---- Finding 2: the recursive copy asks the door of every child. ----

#[test]
fn a_recursive_copy_takes_nothing_of_a_store_below_its_starting_folder() {
    let root = made("k3-r2-").unwrap();
    let walk = root.path().join("walk");
    let inner = walk.join("inner-store");
    let outside = root.path().join("outside-store");
    super::make_dir(&inner).unwrap();
    super::make_dir(&outside).unwrap();
    super::write(inner.join("secret.txt"), "inner secret").unwrap();
    super::write(outside.join("secret.txt"), "outside secret").unwrap();
    super::write(walk.join("plain.txt"), "plain").unwrap();
    let link = walk.join("link-to-store");
    assert!(link_dir(&outside, &link));
    // A hard link to a file that is itself a store.
    let store_file = root.path().join("store-file.txt");
    super::write(&store_file, "store file").unwrap();
    assert!(fs::hard_link(&store_file, walk.join("hard.txt")).is_ok());
    let out = root.path().join("out");
    super::make_dir(&out).unwrap();
    let (_inner, _outside, _file) = (
        TestRoot::add(&inner),
        TestRoot::add(&outside),
        TestRoot::add(&store_file),
    );
    // Asked alone, the door refuses each of them.
    assert!(reads::admitted(&inner.join("secret.txt")).is_err());
    assert!(reads::admitted(&walk.join("hard.txt")).is_err());
    let mut took: Vec<String> = Vec::new();
    let mut ways = vec![("first", walk.clone())];
    #[cfg(any(target_os = "macos", windows))]
    ways.push(("second", super::identity_tests::second_of(&walk)));
    for (which, way) in &ways {
        let tree = out.join(format!("tree-{which}"));
        let copied = super::copy_tree(way, &tree, &|_| false);
        let read = |name: &str| fs::read_to_string(tree.join(name)).ok();
        if copied.is_ok() {
            took.push(format!("{which}: the copy went through"));
        }
        for name in [
            "inner-store/secret.txt",
            "link-to-store/secret.txt",
            "hard.txt",
        ] {
            if read(name).is_some() {
                took.push(format!("{which}: {name}"));
            }
        }
    }
    unlink_dir(&link);
    assert!(took.is_empty(), "the recursive copy took {took:?}");
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

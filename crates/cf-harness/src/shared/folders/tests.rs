//! Whether a folder is inside another by the names of their folders, in any form a system writes
//! a path in. Nothing here looks at the disk: the same on every system.

use super::*;

/// Whether `folder`, as it is written, is `outer` or inside it, the names compared as `fold` says.
fn inside_on(outer: &str, folder: &str, fold: Fold) -> bool {
    inside(Path::new(folder), Path::new(outer), fold)
}

#[test]
fn a_windows_path_is_inside_in_any_form_it_is_written_in() {
    let on_windows = |outer: &str, folder: &str| inside_on(outer, folder, Fold::WINDOWS);
    let outer = r"C:\Users\Dana\.codex\memories";
    for folder in [
        r"C:\Users\Dana\.codex\memories",
        r"C:\Users\Dana\.codex\memories\rollout_summaries",
        // The separators the shell wrote.
        "C:/Users/Dana/.codex/memories",
        r"C:\Users\Dana/.codex\memories/rollout_summaries",
        // A trailing one, a `.`, and a `..` that stays inside.
        r"C:\Users\Dana\.codex\memories\",
        r"C:\Users\Dana\.codex\memories\.\rollout_summaries",
        r"C:\Users\Dana\.codex\memories\a\..\rollout_summaries",
        // Windows' own way to write a path whose links are resolved.
        r"\\?\C:\Users\Dana\.codex\memories",
        r"\\?\C:\Users\Dana\.codex\memories\rollout_summaries",
        // The drive and the folders in any case: NTFS does not tell them apart.
        r"c:\users\dana\.CODEX\Memories",
    ] {
        assert!(on_windows(outer, folder), "{folder} is inside");
    }
    for folder in [
        r"C:\Users\Dana\.codex",
        r"C:\Users\Dana\.codex\sessions",
        r"C:\Users\Dana\.codex\memories-old",
        r"C:\Users\Dana\.codex\memories\..\sessions",
        r"C:\Users\Dana\project",
        // Another drive, and a folder of the same name elsewhere.
        r"D:\Users\Dana\.codex\memories",
        r"C:\Other\Users\Dana\.codex\memories",
        r"\\?\D:\Users\Dana\.codex\memories",
        r"\\server\share\Users\Dana\.codex\memories",
        // Nothing is above a drive's root.
        r"C:\..\..\Users\Dana\.codex\memories\..\..",
    ] {
        assert!(!on_windows(outer, folder), "{folder} is outside");
    }
    // The outer folder written in any of the forms too, and a network one.
    for outer in [
        "C:/Users/Dana/.codex/memories",
        r"\\?\C:\Users\Dana\.codex\memories",
        r"c:\USERS\dana\.codex\memories\",
    ] {
        assert!(
            on_windows(outer, r"C:\Users\Dana\.codex\memories\x"),
            "{outer}"
        );
    }
    let network = r"\\server\share\codex\memories";
    assert!(on_windows(
        network,
        r"\\?\UNC\server\share\codex\memories\x"
    ));
    assert!(on_windows(network, r"\\SERVER\Share\codex\memories"));
    assert!(!on_windows(
        network,
        r"\\?\UNC\server\other\codex\memories\x"
    ));
    assert!(!on_windows(network, r"\\server\share\codex"));
    // A folder with no root is inside nothing, and nothing is inside no folder.
    assert!(!on_windows(outer, "memories"));
    assert!(!on_windows("", r"C:\Users"));
}

#[test]
fn a_mac_compares_the_names_of_folders_in_any_case_and_a_system_that_does_not_ignore_it_does_not() {
    let outer = "/Users/dana/.codex/memories";
    let folder = "/users/Dana/.CODEX/Memories/x";
    assert!(inside_on(outer, folder, Fold::MAC));
    assert!(!inside_on(outer, folder, Fold::LINUX));
    assert!(inside_on(
        outer,
        "/Users/dana/.codex/memories/x",
        Fold::LINUX
    ));
    // A trailing separator, a doubled one and a `.` are no folder.
    assert!(inside_on(
        outer,
        "/Users//dana/./.codex/memories/",
        Fold::LINUX
    ));
    // `..` takes the folder before it and the root has none.
    assert!(!inside_on(
        outer,
        "/Users/dana/.codex/memories/../sessions",
        Fold::MAC
    ));
    assert!(inside_on("/", "/../..", Fold::MAC));
    assert!(!inside_on(outer, "/../..", Fold::MAC));
    // A folder is not inside what only starts with its name.
    assert!(!inside_on(
        outer,
        "/Users/dana/.codex/memories-old",
        Fold::LINUX
    ));
}

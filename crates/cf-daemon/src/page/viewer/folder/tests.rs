//! The folder read as the viewer reads it, on folders made here: the tree in
//! order, without dotfiles; each kind of file; every budget of a walk, in its
//! words; whether the folder holds a file; the HTML file read for the browser
//! with its assets; and every way out of the folder refused, through the door.

use std::fs;
use std::path::Path;

use serde_json::json;

use super::super::door::{named, UNNAMED};
use super::*;

/// A project's folder holding `.specs`, which holds `files` (path and text),
/// each folder made as it is needed.
fn project_with(files: &[(&str, &str)]) -> tempfile::TempDir {
    let project = tempfile::tempdir().unwrap();
    let root = project.path().join(".specs");
    fs::create_dir(&root).unwrap();
    for (path, text) in files {
        let file = root.join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, text).unwrap();
    }
    project
}

fn specs(project: &tempfile::TempDir) -> Folder {
    open(project.path(), ".specs", &|| {}).unwrap().unwrap()
}

/// The folder `folder` of the project at `project`, found as the viewer finds it.
fn found(project: &Path, folder: &str) -> Result<Option<Folder>, String> {
    open(project, folder, &|| {})
}

fn read(folder: &Folder, path: &str) -> Result<Shown, String> {
    folder.read(path, &|| {})
}

/// `count` empty files `{prefix}0000.md` and on, in `folder` of the project's specs folder.
fn files_in(project: &tempfile::TempDir, folder: &str, prefix: &str, count: usize) {
    let at = project.path().join(".specs").join(folder);
    fs::create_dir_all(&at).unwrap();
    for n in 0..count {
        fs::write(at.join(format!("{prefix}{n:05}.md")), "").unwrap();
    }
}

/// How many entries a tree holds, inside its folders too.
fn counted(entries: &[Entry]) -> usize {
    entries
        .iter()
        .map(|entry| match entry {
            Entry::File { .. } => 1,
            Entry::Folder { entries, .. } => 1 + counted(entries),
        })
        .sum()
}

#[test]
fn the_tree_is_each_folder_by_name_with_each_file_s_size_and_no_dotfile() {
    let project = project_with(&[
        ("registry.md", "| ID |"),
        ("user-auth/SPEC.md", "# Auth"),
        ("user-auth/research-01.md", "r"),
        ("user-auth/artifacts/notes.md", "notes"),
        ("user-auth/.DS_Store", "\0\0"),
        ("assets/spec-styles.css", "body{}"),
        (".DS_Store", "\0"),
        (".git/config", "[core]"),
    ]);
    let tree = specs(&project).tree();
    assert_eq!(tree.cut, None);
    assert_eq!(
        serde_json::to_value(&tree.entries).unwrap(),
        json!([
            { "name": "assets", "path": "assets", "entries": [
                { "name": "spec-styles.css", "path": "assets/spec-styles.css", "size": 6 },
            ] },
            { "name": "registry.md", "path": "registry.md", "size": 6 },
            { "name": "user-auth", "path": "user-auth", "entries": [
                { "name": "SPEC.md", "path": "user-auth/SPEC.md", "size": 6 },
                { "name": "artifacts", "path": "user-auth/artifacts", "entries": [
                    { "name": "notes.md", "path": "user-auth/artifacts/notes.md", "size": 5 },
                ] },
                { "name": "research-01.md", "path": "user-auth/research-01.md", "size": 1 },
            ] },
        ])
    );
}

#[test]
fn a_file_is_its_text_an_image_its_bytes_and_anything_else_its_size_and_why() {
    let project = project_with(&[
        ("user-auth/SPEC.md", "# Auth\n\n- [x] done ✓\n"),
        ("user-auth/logo.svg", "<svg><script>alert(1)</script></svg>"),
        ("user-auth/SPEC.md.sha256", "abc  SPEC.md\n"),
    ]);
    let root = project.path().join(".specs");
    fs::write(
        root.join("user-auth/flow.PNG"),
        [0x89, b'P', b'N', b'G', 0, 1],
    )
    .unwrap();
    fs::write(root.join("user-auth/blob.bin"), [b'a', 0, b'b']).unwrap();
    fs::write(root.join("user-auth/latin.txt"), [b'c', 0xE9]).unwrap();
    let folder = specs(&project);
    let shown = |path| read(&folder, path).unwrap();
    assert_eq!(
        serde_json::to_value(shown("user-auth/SPEC.md")).unwrap(),
        json!({ "path": "user-auth/SPEC.md", "size": 23, "text": "# Auth\n\n- [x] done ✓\n" })
    );
    assert_eq!(
        shown("user-auth/SPEC.md.sha256").content,
        Content::Text("abc  SPEC.md\n".to_owned())
    );
    // An image is its bytes, whatever its case, an SVG too: the page draws it as a picture.
    assert_eq!(
        shown("user-auth/flow.PNG").content,
        Content::Image("iVBORwAB".to_owned())
    );
    assert_eq!(
        serde_json::to_value(shown("user-auth/logo.svg")).unwrap()["image"],
        json!("PHN2Zz48c2NyaXB0PmFsZXJ0KDEpPC9zY3JpcHQ+PC9zdmc+")
    );
    for (path, size) in [("user-auth/blob.bin", 3), ("user-auth/latin.txt", 2)] {
        assert_eq!(
            serde_json::to_value(shown(path)).unwrap(),
            json!({
                "path": path,
                "size": size,
                "withheld": format!("{path} is {size} bytes that is neither text nor an image"),
            })
        );
    }
}

#[test]
fn a_file_over_the_limit_or_whose_answer_would_not_fit_a_frame_is_withheld_with_its_size() {
    let project = project_with(&[]);
    let root = project.path().join(".specs");
    let limit = usize::try_from(MAX_FILE).unwrap();
    fs::write(root.join("huge.log"), "x".repeat(limit + 1)).unwrap();
    fs::write(root.join("just.md"), "x".repeat(limit)).unwrap();
    // As many bytes as the limit, but JSON writes each quote as two.
    fs::write(root.join("quotes.md"), "\"".repeat(limit)).unwrap();
    let folder = specs(&project);
    let withheld = |path: &str| {
        Content::Withheld(format!(
            "{path} is 512.0 KB, more than the 512.0 KB the viewer shows"
        ))
    };
    assert_eq!(
        read(&folder, "huge.log").unwrap().content,
        withheld("huge.log")
    );
    assert_eq!(read(&folder, "huge.log").unwrap().size, MAX_FILE + 1);
    assert!(matches!(
        read(&folder, "just.md").unwrap().content,
        Content::Text(_)
    ));
    assert_eq!(
        read(&folder, "quotes.md").unwrap().content,
        withheld("quotes.md")
    );
}

/// A file is measured through the handle it is read by, not by the look before
/// it was opened: one grown past the limit since that look is withheld.
#[test]
fn a_file_is_measured_through_the_handle_it_is_read_by() {
    let project = project_with(&[("grows.md", "small")]);
    let path = project.path().join(".specs/grows.md");
    let limit = usize::try_from(MAX_FILE).unwrap();
    let grow = || fs::write(&path, "y".repeat(limit + 10)).unwrap();
    let folder = specs(&project);
    let shown = folder.read("grows.md", &grow).unwrap();
    // Measured after it was opened: its size is the grown one, and it is withheld.
    assert_eq!(shown.size, MAX_FILE + 10);
    assert!(matches!(shown.content, Content::Withheld(_)), "{shown:?}");
}

#[test]
fn a_tree_of_more_entries_than_the_limit_is_cut_and_says_so() {
    let project = project_with(&[]);
    files_in(&project, "", "note-", MAX_ENTRIES + 1);
    let tree = specs(&project).tree();
    assert_eq!(tree.entries.len(), MAX_ENTRIES);
    assert_eq!(tree.cut.as_deref(), Some(MORE_ENTRIES));
    assert!(MORE_ENTRIES.contains(&MAX_ENTRIES.to_string()));
}

/// A folder's slot is its own before what it holds is walked: a subfolder that
/// spends the whole budget leaves its folder counted, and nothing after it.
#[test]
fn a_subfolder_that_spends_the_whole_budget_is_counted_and_nothing_overflows() {
    let project = project_with(&[("z-after.md", "z")]);
    files_in(&project, "a", "f-", MAX_ENTRIES);
    let tree = specs(&project).tree();
    assert_eq!(counted(&tree.entries), MAX_ENTRIES);
    let [Entry::Folder { name, entries, .. }] = tree.entries.as_slice() else {
        panic!("the one folder, then nothing: {:?}", tree.entries.len());
    };
    assert_eq!(name, "a");
    assert_eq!(entries.len(), MAX_ENTRIES - 1);
    assert_eq!(tree.cut.as_deref(), Some(MORE_ENTRIES));
}

/// A folder of more names than a walk reads is read no further: the walk
/// stops at the budget of names, whatever else the folder holds.
#[test]
fn a_folder_of_more_names_than_a_walk_reads_is_read_no_further() {
    let project = project_with(&[]);
    files_in(&project, "big", ".hidden-", MAX_SCANNED + 5);
    let folder = specs(&project);
    let tree = folder.tree();
    assert_eq!(tree.cut.as_deref(), Some(MORE_NAMES));
    assert!(MORE_NAMES.contains(&MAX_SCANNED.to_string()));
    // Every name read was a dotfile: the folder shows empty, and holds no file to show.
    assert_eq!(
        serde_json::to_value(&tree.entries).unwrap(),
        json!([{ "name": "big", "path": "big", "entries": [] }])
    );
    assert!(!folder.present());
}

/// A name the page could not name (no UTF-8, which a Linux disk may hold by
/// the million) is passed over and still read: it counts against the budget of
/// names like any other, so such a folder is read no further either.
#[test]
fn a_folder_of_names_passed_over_is_read_no_further_than_the_budget() {
    let project = project_with(&[]);
    files_in(&project, "big", "x-", MAX_SCANNED + 5);
    UNNAMED.with(|unnamed| unnamed.set(|name| name.starts_with("x-")));
    let folder = specs(&project);
    let (tree, present) = (folder.tree(), folder.present());
    UNNAMED.with(|unnamed| unnamed.set(named));
    assert_eq!(tree.cut.as_deref(), Some(MORE_NAMES));
    assert_eq!(
        serde_json::to_value(&tree.entries).unwrap(),
        json!([{ "name": "big", "path": "big", "entries": [] }])
    );
    assert!(!present);
}

#[test]
fn a_tree_whose_names_would_not_fit_one_answer_is_cut_and_its_answer_fits() {
    let project = project_with(&[]);
    let at = project.path().join(".specs");
    for n in 0..1999 {
        fs::write(at.join(format!("{n:04}{}.md", "x".repeat(248))), "").unwrap();
    }
    let tree = specs(&project).tree();
    assert_eq!(tree.cut.as_deref(), Some(TOO_LONG));
    let answer = serde_json::to_vec(&json!({ "ok": true, "specs": {
        "folder": ".specs", "found": true, "entries": tree.entries, "cut": tree.cut,
    } }))
    .unwrap();
    assert!(answer.len() < 1024 * 1024, "{} bytes", answer.len());
    assert!(
        tree.entries.len() > 900,
        "as many as fit: {}",
        tree.entries.len()
    );
}

#[test]
fn a_folder_deeper_than_the_limit_is_shown_empty_and_the_tree_says_so() {
    let deep = (0..=MAX_DEPTH)
        .map(|at| format!("d{at}"))
        .collect::<Vec<_>>();
    let path = format!("{}/SPEC.md", deep.join("/"));
    let project = project_with(&[(path.as_str(), "deep")]);
    let tree = specs(&project).tree();
    let mut level = &tree.entries;
    for _ in 0..MAX_DEPTH {
        let [Entry::Folder { entries, .. }] = level.as_slice() else {
            panic!("a folder at each level: {level:?}");
        };
        level = entries;
    }
    assert!(level.is_empty(), "the folder past the limit is shown empty");
    assert_eq!(tree.cut.as_deref(), Some(DEEPER));
    assert!(DEEPER.contains(&MAX_DEPTH.to_string()));
    // It is still read by its path: the limit is the tree's.
    assert!(read(&specs(&project), &path).is_ok());
}

#[test]
fn a_folder_holds_a_file_where_a_tree_would_show_one() {
    let project = project_with(&[(".DS_Store", "\0")]);
    fs::create_dir_all(project.path().join(".specs/a/b")).unwrap();
    let folder = specs(&project);
    assert!(!folder.present(), "folders and a dotfile only");
    fs::write(project.path().join(".specs/a/b/SPEC.md"), "# B").unwrap();
    assert!(folder.present());
}

#[test]
fn a_path_out_of_the_folder_or_to_what_is_no_file_is_refused_in_words() {
    let project = project_with(&[("user-auth/SPEC.md", "# Auth"), ("user-auth/.notes", "x")]);
    fs::write(project.path().join("secret.env"), "TOKEN=1").unwrap();
    let folder = specs(&project);
    let absolute = project.path().join("secret.env");
    for (path, words) in [
        (
            "../secret.env",
            "../secret.env climbs out of the specs folder (..)".to_owned(),
        ),
        (
            "user-auth/../../secret.env",
            "user-auth/../../secret.env climbs out of the specs folder (..)".to_owned(),
        ),
        (
            "/etc/passwd",
            "/etc/passwd is absolute: a path in the specs folder is relative to it".to_owned(),
        ),
        (
            absolute.to_str().unwrap(),
            if cfg!(windows) {
                format!(
                    "{} holds a backslash: a path in the specs folder is written in slashes",
                    absolute.display()
                )
            } else {
                format!(
                    "{} is absolute: a path in the specs folder is relative to it",
                    absolute.display()
                )
            },
        ),
        (
            "user-auth/.notes",
            "user-auth/.notes is a dotfile, which the viewer leaves out".to_owned(),
        ),
        ("user-auth", "user-auth is a folder, not a file".to_owned()),
        (
            "user-auth/PLAN.md",
            "no file user-auth/PLAN.md in the specs folder".to_owned(),
        ),
        (
            "user-auth/SPEC.md/x",
            "user-auth/SPEC.md/x goes through user-auth/SPEC.md, which is no folder".to_owned(),
        ),
        (
            "",
            "a path in the specs folder names a file: this one is empty".to_owned(),
        ),
    ] {
        assert_eq!(read(&folder, path).unwrap_err(), words, "{path:?}");
    }
}

#[test]
fn a_folder_that_is_not_there_is_none_and_one_that_is_a_file_is_refused() {
    let project = tempfile::tempdir().unwrap();
    assert!(found(project.path(), ".specs").unwrap().is_none());
    assert!(found(&project.path().join("gone"), ".specs")
        .unwrap()
        .is_none());
    fs::write(project.path().join(".specs"), "not a folder").unwrap();
    assert_eq!(
        found(project.path(), ".specs").err().unwrap(),
        "the specs folder .specs is a file, not a folder"
    );
    fs::create_dir_all(project.path().join("docs/plans")).unwrap();
    assert!(found(project.path(), "docs/plans").unwrap().is_some());
    assert!(found(project.path(), "docs/gone/plans").unwrap().is_none());
    fs::write(project.path().join("notes"), "a file").unwrap();
    assert_eq!(
        found(project.path(), "notes/plans").err().unwrap(),
        "the specs folder notes/plans goes through notes, which is no folder"
    );
    assert_eq!(
        found(project.path(), "../elsewhere").err().unwrap(),
        "../elsewhere climbs out of the specs folder (..)"
    );
}

/// A dotfile is left out wherever it is in a path, and on Windows a name that
/// may be one's short (8.3) name, as the page writes it (`GIT~1` is `.git`
/// where the volume keeps short names).
#[test]
fn a_path_through_a_dotfile_or_on_windows_a_short_name_is_left_out() {
    let words = |path: &str, windows| {
        let named: Vec<&str> = path.split('/').collect();
        left_out(path, &named, windows).err()
    };
    for windows in [false, true] {
        assert_eq!(
            words(".git/config", windows).as_deref(),
            Some(".git/config is a dotfile, which the viewer leaves out")
        );
        assert_eq!(words("a/SPEC.md", windows), None);
    }
    assert_eq!(
        words("GIT~1/config", true).as_deref(),
        Some(
            "GIT~1/config names GIT~1, which may be the short (8.3) name of a dotfile: the \
             viewer reads no such name on Windows"
        )
    );
    assert_eq!(words("GIT~1/config", false), None);
    // Read through the folder: on Windows refused as the page wrote it, whatever the volume keeps.
    let project = project_with(&[(".git/config", "[core]")]);
    let refused = read(&specs(&project), "git~1/config").unwrap_err();
    if cfg!(windows) {
        assert!(refused.contains("short (8.3) name"), "{refused}");
    } else {
        assert_eq!(refused, "no file git~1/config in the specs folder");
    }
}

#[test]
fn a_size_is_said_in_bytes_kb_or_mb() {
    assert_eq!(measure(0), "0 bytes");
    assert_eq!(measure(1023), "1023 bytes");
    assert_eq!(measure(1024), "1.0 KB");
    assert_eq!(measure(12_800), "12.5 KB");
    assert_eq!(measure(3_400_000), "3.2 MB");
}

mod browser;

/// Links, FIFOs and hard links, which Unix lets a test make.
#[cfg(unix)]
mod links;

/// Names as Windows answers to them: in any case, and by a short name.
#[cfg(windows)]
mod windows;

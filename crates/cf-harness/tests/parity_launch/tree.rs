//! A root's tree: its snapshot, what a plan changed in it, and what the CLIs
//! wrote in the folders they keep their own state in.

use std::collections::BTreeMap;
use std::fs;
use std::io::ErrorKind;
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::Change;

/// Everything under a root a plan would change, and what the CLIs' own
/// folders hold.
pub(super) struct Snapshot {
    entries: BTreeMap<String, Change>,
    counts: BTreeMap<String, i64>,
}

/// Everything under `root` a plan would change (`snapshot`, `tree.mjs`), by
/// its path there with `/` between the names; `owned` folders are counted,
/// not listed.
pub(super) fn snapshot(root: &Path, owned: &[String]) -> Snapshot {
    fn walk(
        folder: &Path,
        relative: &str,
        owned: &[String],
        harness_owns: bool,
        found: &mut Snapshot,
    ) {
        let names = match fs::read_dir(folder) {
            Ok(names) => names,
            Err(error) if harness_owns && error.kind() == ErrorKind::NotFound => return,
            Err(error) => panic!("{}: {error}", folder.display()),
        };
        for entry in names {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) if harness_owns && error.kind() == ErrorKind::NotFound => continue,
                Err(error) => panic!("{}: {error}", folder.display()),
            };
            let name = entry.file_name().to_string_lossy().into_owned();
            let name = if relative.is_empty() {
                name
            } else {
                format!("{relative}/{name}")
            };
            let within = owned
                .iter()
                .find(|own| name.starts_with(&format!("{own}/")));
            if let Some(within) = within {
                *found.counts.get_mut(within).unwrap() += 1;
            } else if !owned.contains(&name) {
                found
                    .entries
                    .insert(name.clone(), describe_file(&entry.path(), &name));
            }
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                let owns = within.is_some() || owned.contains(&name);
                walk(&entry.path(), &name, owned, owns, found);
            }
        }
    }
    let mut found = Snapshot {
        entries: BTreeMap::new(),
        counts: owned.iter().map(|folder| (folder.clone(), 0)).collect(),
    };
    walk(root, "", owned, false, &mut found);
    found
}

/// A file, folder or link as the comparison holds it (`describe`, `tree.mjs`).
fn describe_file(file: &Path, name: &str) -> Change {
    let found = fs::symlink_metadata(file).unwrap();
    if found.is_symlink() {
        let mut link = Change::new(name, "link");
        link.target = Some(fs::read_link(file).unwrap().to_string_lossy().into_owned());
        return link;
    }
    let mode = mode_of(&found);
    if found.is_dir() {
        let mut folder = Change::new(name, "dir");
        folder.mode = mode;
        return folder;
    }
    let mut made = Change::new(name, "file");
    made.mode = mode;
    match String::from_utf8(fs::read(file).unwrap()) {
        Ok(text) => made.text = Some(text),
        Err(invalid) => made.bytes = Some(hex(&Sha256::digest(invalid.as_bytes()))),
    }
    made
}

#[cfg(unix)]
fn mode_of(found: &fs::Metadata) -> Option<u32> {
    Some(std::os::unix::fs::PermissionsExt::mode(&found.permissions()) & 0o777)
}

#[cfg(not(unix))]
fn mode_of(_found: &fs::Metadata) -> Option<u32> {
    None
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// What a plan did to the tree: each path made or changed, as it is now, and
/// each one removed (`changes`, `tree.mjs`).
pub(super) fn changes(before: &Snapshot, after: &Snapshot) -> Vec<Change> {
    let mut made: Vec<Change> = after
        .entries
        .iter()
        .filter(|(path, entry)| before.entries.get(*path) != Some(entry))
        .map(|(_, entry)| entry.clone())
        .collect();
    made.extend(
        before
            .entries
            .keys()
            .filter(|path| !after.entries.contains_key(*path))
            .map(|path| Change::new(path, "removed")),
    );
    made.sort_by(|left, right| left.path.cmp(&right.path));
    made
}

/// How many entries each owned folder gained, those that gained any.
pub(super) fn gained(before: &Snapshot, after: &Snapshot) -> BTreeMap<String, i64> {
    after
        .counts
        .iter()
        .map(|(folder, count)| (folder.clone(), count - before.counts[folder]))
        .filter(|(_, made)| *made != 0)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A file with its folders made.
    fn write(file: &Path, text: impl AsRef<[u8]>) {
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, text).unwrap();
    }

    #[test]
    fn a_plan_s_changes_are_what_it_made_changed_and_removed_but_for_the_folders_a_cli_owns() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let owned = vec!["cli".to_owned()];
        write(&root.join("a/kept.txt"), "kept");
        write(&root.join("a/changed.txt"), "was");
        write(&root.join("gone.txt"), "x");
        write(&root.join("cli/state/db"), "x");
        let before = snapshot(root, &owned);
        write(&root.join("a/changed.txt"), "is");
        write(&root.join("a/made.txt"), "new");
        fs::remove_file(root.join("gone.txt")).unwrap();
        write(&root.join("cli/state/more"), "y");
        write(&root.join("cli/other"), "z");
        let after = snapshot(root, &owned);
        let made: Vec<(String, String, Option<String>)> = changes(&before, &after)
            .into_iter()
            .map(|change| (change.path, change.kind, change.text))
            .collect();
        assert_eq!(
            made,
            [
                ("a/changed.txt", "file", Some("is")),
                ("a/made.txt", "file", Some("new")),
                ("gone.txt", "removed", None),
            ]
            .map(|(path, kind, text)| (
                path.to_owned(),
                kind.to_owned(),
                text.map(str::to_owned)
            ))
        );
        assert_eq!(
            gained(&before, &after),
            BTreeMap::from([("cli".to_owned(), 2)])
        );
        assert!(
            !after.entries.contains_key("cli"),
            "an owned folder is not listed"
        );
    }

    #[test]
    fn a_file_is_its_text_whole_or_the_hash_of_its_bytes_and_a_link_is_its_target() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(&root.join("bom"), "\u{feff}text");
        write(&root.join("binary"), [0xff, 0xfe, 0x00]);
        #[cfg(unix)]
        std::os::unix::fs::symlink("elsewhere", root.join("link")).unwrap();
        let found = snapshot(root, &[]).entries;
        assert_eq!(found["bom"].text.as_deref(), Some("\u{feff}text"));
        assert_eq!(found["binary"].text, None);
        assert_eq!(
            found["binary"].bytes.as_deref(),
            Some(hex(&Sha256::digest([0xff, 0xfe, 0x00])).as_str())
        );
        #[cfg(unix)]
        {
            assert_eq!(found["link"].kind, "link");
            assert_eq!(found["link"].target.as_deref(), Some("elsewhere"));
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_mode_is_what_the_system_gave_it_and_it_is_told() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(&root.join("sealed/file"), "x");
        fs::set_permissions(root.join("sealed/file"), fs::Permissions::from_mode(0o600)).unwrap();
        fs::set_permissions(root.join("sealed"), fs::Permissions::from_mode(0o700)).unwrap();
        let found = snapshot(root, &[]).entries;
        assert_eq!(found["sealed"].mode, Some(0o700));
        assert_eq!(found["sealed/file"].mode, Some(0o600));
    }
}

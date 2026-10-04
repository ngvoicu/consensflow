//! A session's file found under a harness's folder (`findFile`,
//! `hosts/lib/completion/shared.js`): depth first, the first match winning,
//! every store read-only.

use std::fs;
use std::path::{Path, PathBuf};

use cf_base::path;

/// How deep under its root a lookup goes.
pub(crate) const DEPTH: u32 = 6;

/// The first file under `root`, `depth` folders down at most, whose name
/// `matches`. A folder that cannot be read holds nothing; a link is neither
/// a file nor a folder, as `readdir`'s entries say.
///
/// Node reads an entry's name as UTF-8, a byte that is none as U+FFFD, and
/// joins that text to the folder (`path.join`): so does this. A name that is
/// no UTF-8 then names a path that is not there, as it did for Node.
pub(crate) fn find_file(
    root: &Path,
    matches: &dyn Fn(&str) -> bool,
    depth: u32,
) -> Option<PathBuf> {
    let folder = root.to_string_lossy();
    for entry in entries(root) {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let full = PathBuf::from(path::join(&[&folder, &name]));
        if kind.is_file() && matches(&name) {
            return Some(full);
        }
        if kind.is_dir() && depth > 0 {
            if let Some(found) = find_file(&full, matches, depth - 1) {
                return Some(found);
            }
        }
    }
    None
}

/// A folder's entries in the order Node's `readdir` lists them: libuv sorts
/// them by name on Unix, and takes the system's order on Windows.
fn entries(folder: &Path) -> Vec<fs::DirEntry> {
    let Ok(listed) = fs::read_dir(folder) else {
        return Vec::new();
    };
    #[allow(unused_mut)] // Sorted on Unix alone.
    let mut entries: Vec<fs::DirEntry> = listed.filter_map(Result::ok).collect();
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        entries.sort_by(|left, right| {
            left.file_name()
                .as_bytes()
                .cmp(right.file_name().as_bytes())
        });
    }
    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_match_depth_first_in_node_s_order_is_found() {
        let root = tempfile::tempdir().unwrap();
        let deep = root.path().join("2026").join("09").join("06");
        fs::create_dir_all(&deep).unwrap();
        fs::write(deep.join("rollout-x-session.jsonl"), "").unwrap();
        fs::create_dir_all(root.path().join("b")).unwrap();
        fs::write(root.path().join("b").join("other-session.jsonl"), "").unwrap();
        let found = find_file(root.path(), &|name| name.contains("session"), DEPTH).unwrap();
        // "2026" sorts before "b": depth first, it is found first.
        assert_eq!(found, deep.join("rollout-x-session.jsonl"));
        assert_eq!(
            find_file(root.path(), &|name| name.contains("none"), DEPTH),
            None
        );
        assert_eq!(
            find_file(&root.path().join("missing"), &|_| true, DEPTH),
            None
        );
    }

    #[test]
    fn nothing_deeper_than_the_depth_is_found() {
        let root = tempfile::tempdir().unwrap();
        let mut deep = root.path().to_path_buf();
        for level in 0..8 {
            deep = deep.join(level.to_string());
        }
        fs::create_dir_all(&deep).unwrap();
        fs::write(deep.join("found.jsonl"), "").unwrap();
        assert_eq!(
            find_file(root.path(), &|name| name == "found.jsonl", DEPTH),
            None
        );
        assert!(find_file(root.path(), &|name| name == "found.jsonl", 8).is_some());
    }
}

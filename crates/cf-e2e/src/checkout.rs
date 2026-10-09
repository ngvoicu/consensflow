//! The checkout the suites run in: where it is, the paths under it written the
//! way the repository writes them (`/` between folders), and the files below a
//! folder. A suite that reads the repository's own words (the usage `cf` is
//! held to, the sources it must not have grown back) finds them here.

use std::fs;
use std::path::{Path, PathBuf};

use crate::{Error, Result};

/// The checkout's root: this crate is `crates/cf-e2e` in it.
pub fn root() -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest
        .ancestors()
        .nth(2)
        .unwrap_or(manifest)
        .to_path_buf()
}

/// The path `relative` names under the root, which the repository writes with
/// `/` between folders and the platform joins with its own separator.
pub fn path(relative: &str) -> PathBuf {
    relative
        .split('/')
        .filter(|part| !part.is_empty())
        .fold(root(), |path, part| path.join(part))
}

/// `path` as the repository writes it, from the root and with `/` between
/// folders; a path that is not under the root, as it is.
pub fn relative(path: &Path) -> String {
    match path.strip_prefix(root()) {
        Ok(under) => {
            let parts: Vec<_> = under
                .components()
                .map(|part| part.as_os_str().to_string_lossy())
                .collect();
            parts.join("/")
        }
        Err(_) => path.to_string_lossy().into_owned(),
    }
}

/// Every file below the folder `dir`, in the order of their paths. A folder
/// named in `skipping` is not entered, wherever it is; a link to a folder is a
/// file here, not entered either.
pub fn files_below(dir: &Path, skipping: &[&str]) -> Result<Vec<PathBuf>> {
    let mut found = Vec::new();
    collect(dir, skipping, &mut found)?;
    found.sort();
    Ok(found)
}

fn collect(dir: &Path, skipping: &[&str], found: &mut Vec<PathBuf>) -> Result {
    for entry in fs::read_dir(dir).map_err(Error::file("list", dir))? {
        let entry = entry.map_err(Error::file("list", dir))?;
        let path = entry.path();
        let kind = entry.file_type().map_err(Error::file("look at", &path))?;
        if !kind.is_dir() {
            found.push(path);
        } else if !skipping.iter().any(|name| entry.file_name() == *name) {
            collect(&path, skipping, found)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_root_is_the_checkout_this_crate_is_in() {
        let root = root();
        assert!(root.join("Cargo.toml").is_file());
        assert!(root
            .join("crates")
            .join("cf-e2e")
            .join("Cargo.toml")
            .is_file());
    }

    #[test]
    fn a_path_is_joined_with_the_platforms_separator_and_empty_parts_are_nothing() {
        let expected = root().join("crates").join("cf").join("src");
        assert_eq!(path("crates/cf/src"), expected);
        assert_eq!(path("crates//cf/src/"), expected);
        assert_eq!(path(""), root());
    }

    #[test]
    fn a_path_under_the_root_is_written_from_it_with_slashes() {
        assert_eq!(
            relative(&root().join("crates").join("cf").join("Cargo.toml")),
            "crates/cf/Cargo.toml"
        );
        assert_eq!(
            relative(&path("crates/cf/Cargo.toml")),
            "crates/cf/Cargo.toml"
        );
        assert_eq!(relative(&root()), "");
        // Not under the root: as it is.
        assert_eq!(relative(Path::new("elsewhere")), "elsewhere");
    }

    fn touch(path: &Path) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "").unwrap();
    }

    #[test]
    fn the_files_below_a_folder_are_listed_in_order_without_the_folders_skipped() {
        let root = tempfile::tempdir().unwrap();
        for file in [
            "b.md",
            "a/z.rs",
            "a/inner/y.rs",
            "node_modules/dep/index.js",
            "a/node_modules/dep/index.js",
            "c/node_modules.md",
        ] {
            touch(&root.path().join(file));
        }
        let listed = files_below(root.path(), &["node_modules"]).unwrap();
        let names: Vec<_> = listed
            .iter()
            .map(|file| file.strip_prefix(root.path()).unwrap().to_path_buf())
            .collect();
        let expected: Vec<PathBuf> = ["a/inner/y.rs", "a/z.rs", "b.md", "c/node_modules.md"]
            .iter()
            .map(|file| file.split('/').collect())
            .collect();
        assert_eq!(names, expected);
        assert_eq!(files_below(root.path(), &[]).unwrap().len(), 6);
    }

    #[cfg(unix)]
    #[test]
    fn a_link_to_a_folder_is_a_file_here_and_is_not_entered() {
        let root = tempfile::tempdir().unwrap();
        touch(&root.path().join("real").join("inside.txt"));
        std::os::unix::fs::symlink(root.path().join("real"), root.path().join("link")).unwrap();
        let listed = files_below(root.path(), &[]).unwrap();
        assert_eq!(
            listed,
            [
                root.path().join("link"),
                root.path().join("real").join("inside.txt")
            ]
        );
    }

    #[test]
    fn a_folder_that_is_not_there_is_an_error_that_names_it() {
        let root = tempfile::tempdir().unwrap();
        let missing = root.path().join("missing");
        let failed = files_below(&missing, &[]).unwrap_err();
        assert!(
            matches!(&failed, Error::File { action: "list", path, .. } if *path == missing),
            "{failed}"
        );
    }
}

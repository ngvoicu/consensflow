//! The proof that the live app and its roster were not touched by a build of the
//! candidate, rather than an assumption: the bundle and the roster are
//! fingerprinted before the build and compared after.

use std::fs::{self, File};
use std::io;
use std::path::Path;

use sha2::{Digest, Sha256};

use super::{file, Error};

/// What the live app and the live roster were, as one hash each.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Canaries {
    /// Every path, link and byte of the live app's bundle, and the mode of each
    /// file: any change changes it. None where no app is installed.
    pub app: Option<String>,
    /// The roster's bytes. None where there is no roster.
    pub roster: Option<String>,
}

/// The canaries of the live app at `live_app` and of the roster at `live_roster`.
pub fn take(live_app: &Path, live_roster: &Path) -> Result<Canaries, Error> {
    let app = if live_app.exists() {
        Some(fingerprint(live_app)?)
    } else {
        None
    };
    let roster = if live_roster.exists() {
        let bytes = fs::read(live_roster).map_err(file("read", live_roster))?;
        Some(format!("{:x}", Sha256::digest(bytes)))
    } else {
        None
    };
    Ok(Canaries { app, roster })
}

/// One hash over every path and byte in the tree at `root`: any change changes it.
pub fn fingerprint(root: &Path) -> Result<String, Error> {
    let mut hash = Sha256::new();
    walk(root, root, &mut hash)?;
    Ok(format!("{:x}", hash.finalize()))
}

/// Feeds `hash` with what is in `directory`, entry by entry in the order of
/// their names, and what is in each folder among them.
fn walk(root: &Path, directory: &Path, hash: &mut Sha256) -> Result<(), Error> {
    let mut names = fs::read_dir(directory)
        .map_err(file("list", directory))?
        .map(|entry| entry.map(|entry| entry.file_name()))
        .collect::<io::Result<Vec<_>>>()
        .map_err(file("list", directory))?;
    names.sort();
    for name in names {
        let path = directory.join(name);
        let relative = path.strip_prefix(root).unwrap_or(&path);
        hash.update(relative.to_string_lossy().as_bytes());
        let kind = fs::symlink_metadata(&path).map_err(file("look at", &path))?;
        if kind.is_dir() {
            hash.update(b"\0dir\0");
            walk(root, &path, hash)?;
        } else if kind.is_symlink() {
            let target = fs::read_link(&path).map_err(file("read the link", &path))?;
            hash.update(b"\0link\0");
            hash.update(target.to_string_lossy().as_bytes());
        } else {
            hash.update(b"\0file\0");
            hash.update(mode_of(&kind).to_string().as_bytes());
            hash.update(b"\0");
            let mut bytes = File::open(&path).map_err(file("open", &path))?;
            io::copy(&mut bytes, hash).map_err(file("read", &path))?;
        }
        hash.update(b"\0");
    }
    Ok(())
}

/// A file's permission bits, which a system with none says as 0.
fn mode_of(metadata: &fs::Metadata) -> u32 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o777
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::path::PathBuf;

    /// A tree with a folder, a file in it, an empty folder, and a link.
    fn tree() -> (tempfile::TempDir, PathBuf) {
        let root = tempfile::tempdir().unwrap();
        let app = root.path().join("ConsensFlow.app");
        fs::create_dir_all(app.join("Contents").join("MacOS")).unwrap();
        fs::create_dir_all(app.join("Contents").join("Empty")).unwrap();
        fs::write(app.join("Contents").join("MacOS").join("app"), "program").unwrap();
        fs::write(app.join("Contents").join("Info.plist"), "plist").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink("MacOS/app", app.join("Contents").join("link")).unwrap();
        (root, app)
    }

    #[test]
    fn a_tree_has_the_same_fingerprint_however_often_it_is_read_and_a_copy_of_it_the_same() {
        let (_root, app) = tree();
        let first = fingerprint(&app).unwrap();
        assert_eq!(fingerprint(&app).unwrap(), first);
        assert_eq!(first.len(), 64);
        let (_other, copy) = tree();
        assert_eq!(fingerprint(&copy).unwrap(), first);
    }

    #[test]
    fn any_change_to_a_tree_changes_its_fingerprint() {
        let (_root, app) = tree();
        let contents = app.join("Contents");
        let mut seen = vec![fingerprint(&app).unwrap()];
        // Each change is made on the tree the one before left, and so is one more of its own.
        for what in [
            "a byte",
            "a file more",
            "an empty folder more",
            "a file renamed",
            "a file less",
            "an empty folder less",
        ] {
            match what {
                "a byte" => fs::write(contents.join("MacOS").join("app"), "programs").unwrap(),
                "a file more" => fs::write(contents.join("added"), "").unwrap(),
                "an empty folder more" => {
                    fs::create_dir(contents.join("Empty").join("inside")).unwrap()
                }
                "a file renamed" => {
                    fs::rename(contents.join("Info.plist"), contents.join("Other.plist")).unwrap();
                }
                "a file less" => fs::remove_file(contents.join("Other.plist")).unwrap(),
                _ => fs::remove_dir_all(contents.join("Empty")).unwrap(),
            }
            let after = fingerprint(&app).unwrap();
            assert!(
                !seen.contains(&after),
                "{what} left the fingerprint as it was"
            );
            seen.push(after);
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_link_made_to_name_another_file_and_a_file_made_executable_are_changes_too() {
        use std::os::unix::fs::PermissionsExt;

        let (_root, app) = tree();
        let before = fingerprint(&app).unwrap();
        let link = app.join("Contents").join("link");
        fs::remove_file(&link).unwrap();
        std::os::unix::fs::symlink("Info.plist", &link).unwrap();
        let relinked = fingerprint(&app).unwrap();
        assert_ne!(relinked, before);
        let program = app.join("Contents").join("MacOS").join("app");
        fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
        let executable = fingerprint(&app).unwrap();
        assert_ne!(executable, relinked);
        // The link is not followed: what it names changing is no change of the link.
        fs::write(app.join("Contents").join("Info.plist"), "plist").unwrap();
        assert_eq!(fingerprint(&app).unwrap(), executable);
    }

    #[test]
    fn the_canaries_are_a_hash_of_the_app_and_a_hash_of_the_roster_and_none_for_what_is_not_there()
    {
        let (root, app) = tree();
        let roster = root.path().join("agents.json");
        let missing = take(&app.join("not-installed"), &roster).unwrap();
        assert_eq!(
            missing,
            Canaries {
                app: None,
                roster: None
            }
        );

        fs::write(&roster, "{\"agents\":[]}").unwrap();
        let taken = take(&app, &roster).unwrap();
        assert_eq!(taken.app, Some(fingerprint(&app).unwrap()));
        // The SHA-256 of the roster's bytes, as `shasum -a 256` says it.
        let expected = format!("{:x}", Sha256::digest(b"{\"agents\":[]}"));
        assert_eq!(taken.roster, Some(expected));

        fs::write(&roster, "{\"agents\":[1]}").unwrap();
        let changed = take(&app, &roster).unwrap();
        assert_eq!(changed.app, taken.app);
        assert_ne!(changed.roster, taken.roster);
    }

    #[test]
    fn a_tree_that_cannot_be_read_is_an_error_that_names_the_path() {
        let root = tempfile::tempdir().unwrap();
        let gone = root.path().join("gone");
        // `take` looks for the app first, so a folder that is a file is what cannot be listed.
        fs::write(&gone, "not a folder").unwrap();
        let said = fingerprint(&gone).unwrap_err().to_string();
        assert!(
            said.starts_with(&format!("could not list {}", gone.display())),
            "{said}"
        );
    }
}

//! The synchronous calls of Node's `fs` that `write` and `promises` do not
//! make, and a bundle's installation does: a file read whole
//! (`readFileSync`), a folder of a name of its own made (`mkdtempSync`), and
//! a folder with all it holds removed (`rmSync` with `recursive` and
//! `force`). Each failure is said as Node's error says it, probed on Node
//! v26.8.1.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::errno::is_missing;
use super::error::FileError;
use super::promises::read_whole;

/// The characters a made name is spelled with: the 62 `mkdtemp` draws from.
const NAME_CHARACTERS: &[u8; 62] =
    b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";

/// How many names are tried, each taken by a folder already, before the one
/// that is there is the failure.
const TRIES: usize = 128;

/// `readFileSync(path)`: the file's bytes, or the failure of its `open` with
/// the path, or of its `read` (a folder, which opens) with none: the
/// synchronous call, unlike the promised one, names no path there.
pub fn read_file_sync(path: &Path) -> Result<Vec<u8>, FileError> {
    read_whole(path, None)
}

/// `mkdtempSync(prefix)`: a folder named `prefix` and six characters made of
/// `random`'s bytes, private to this user (mode `0o700`; Windows has none),
/// the first name not taken. A failure names the template Node gives its
/// call, the prefix and `XXXXXX`; so does a `random` that fails, as the
/// system's own failure to draw is the call's.
pub fn make_temporary_folder(
    prefix: &str,
    mut random: impl FnMut() -> io::Result<[u8; 6]>,
) -> Result<PathBuf, FileError> {
    let template = PathBuf::from(format!("{prefix}XXXXXX"));
    let failed = |failed| FileError::call(failed, "mkdtemp", Some(&template));
    #[cfg(unix)]
    let builder = {
        let mut builder = fs::DirBuilder::new();
        std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
        builder
    };
    #[cfg(not(unix))]
    let builder = fs::DirBuilder::new();
    let mut tries = 1;
    loop {
        let name: String = random()
            .map_err(failed)?
            .iter()
            .map(|byte| char::from(NAME_CHARACTERS[usize::from(*byte) % NAME_CHARACTERS.len()]))
            .collect();
        let folder = PathBuf::from(format!("{prefix}{name}"));
        match builder.create(&folder) {
            Ok(()) => return Ok(folder),
            Err(taken) if taken.kind() == io::ErrorKind::AlreadyExists && tries < TRIES => {
                tries += 1;
            }
            Err(other) => return Err(failed(other)),
        }
    }
}

/// `rmSync(path, { recursive: true, force: true })`: a folder with all it
/// holds, a file, or a link and not what it names, or nothing there, which is
/// no failure. A failure is said as the C++ behind `rmSync` says it.
///
/// Kept from Node on purpose: a tree some of which cannot be removed says
/// the first failure, where Node's C++ library says its own, which differs
/// between systems (macOS's: that the folder is not empty).
pub fn remove_all(path: &Path) -> Result<(), FileError> {
    let removed = match fs::symlink_metadata(path) {
        Ok(found) if found.is_dir() => fs::remove_dir_all(path),
        Ok(_) => fs::remove_file(path),
        Err(failed) => Err(failed),
    };
    match removed {
        Err(failed) if !is_missing(&failed) => Err(FileError::removal(failed, path)),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A name source that says each of `names` in turn, a byte of each
    /// character's place in the alphabet.
    fn names<'a>(names: &'a [&'a str]) -> impl FnMut() -> io::Result<[u8; 6]> + 'a {
        let mut next = names.iter();
        move || {
            let name = next.next().expect("a name left");
            let mut bytes = [0; 6];
            for (byte, character) in bytes.iter_mut().zip(name.bytes()) {
                let at = NAME_CHARACTERS.iter().position(|held| *held == character);
                *byte = u8::try_from(at.expect("a character of the alphabet")).unwrap();
            }
            Ok(bytes)
        }
    }

    #[test]
    fn a_read_says_the_open_with_its_path_and_a_folder_s_read_with_none() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing");
        assert_eq!(
            read_file_sync(&missing).unwrap_err().to_string(),
            format!(
                "ENOENT: no such file or directory, open '{}'",
                missing.display()
            )
        );
        fs::write(dir.path().join("file"), "text").unwrap();
        assert_eq!(read_file_sync(&dir.path().join("file")).unwrap(), b"text");
        // Probed on Node v26.8.1: `fs.readFileSync` of a folder, where the
        // promised `readFile` names the folder.
        assert_eq!(
            read_file_sync(dir.path()).unwrap_err().to_string(),
            "EISDIR: illegal operation on a directory, read"
        );
    }

    #[test]
    fn a_folder_is_made_under_the_prefix_and_six_characters_and_is_private() {
        let dir = tempfile::tempdir().unwrap();
        let prefix = format!("{}/.install-", dir.path().display());
        let made = make_temporary_folder(&prefix, names(&["aB3zZ9"])).unwrap();
        assert_eq!(made, PathBuf::from(format!("{prefix}aB3zZ9")));
        assert!(made.is_dir());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&made).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o700);
        }
    }

    #[test]
    fn a_name_taken_is_left_and_the_next_one_made() {
        let dir = tempfile::tempdir().unwrap();
        let prefix = format!("{}/.install-", dir.path().display());
        fs::create_dir(format!("{prefix}aaaaaa")).unwrap();
        let made = make_temporary_folder(&prefix, names(&["aaaaaa", "bbbbbb"])).unwrap();
        assert_eq!(made, PathBuf::from(format!("{prefix}bbbbbb")));
    }

    #[test]
    fn names_that_are_all_taken_end_in_the_one_that_exists() {
        let dir = tempfile::tempdir().unwrap();
        let prefix = format!("{}/.install-", dir.path().display());
        fs::create_dir(format!("{prefix}aaaaaa")).unwrap();
        let failed = make_temporary_folder(&prefix, || Ok([0; 6])).unwrap_err();
        assert_eq!(
            failed.to_string(),
            format!("EEXIST: file already exists, mkdtemp '{prefix}XXXXXX'")
        );
    }

    #[test]
    fn a_failure_to_make_a_folder_says_the_template() {
        let dir = tempfile::tempdir().unwrap();
        let missing = format!("{}/missing/.install-", dir.path().display());
        // Probed on Node v26.8.1: `fs.mkdtempSync` names the template.
        assert_eq!(
            make_temporary_folder(&missing, || Ok([0; 6]))
                .unwrap_err()
                .to_string(),
            format!("ENOENT: no such file or directory, mkdtemp '{missing}XXXXXX'")
        );
        // Under a file Windows says there is no such folder, as it says of any
        // path that cannot be.
        fs::write(dir.path().join("file"), "x").unwrap();
        let under = format!("{}/file/.install-", dir.path().display());
        let failed = make_temporary_folder(&under, || Ok([0; 6])).unwrap_err();
        #[cfg(unix)]
        assert_eq!(
            failed.to_string(),
            format!("ENOTDIR: not a directory, mkdtemp '{under}XXXXXX'")
        );
        #[cfg(windows)]
        assert!(failed
            .to_string()
            .ends_with(&format!("mkdtemp '{under}XXXXXX'")));
    }

    #[cfg(unix)]
    #[test]
    fn a_source_of_names_that_fails_fails_the_call() {
        let dir = tempfile::tempdir().unwrap();
        let prefix = format!("{}/.install-", dir.path().display());
        // 5 is EIO on every Unix.
        let failed =
            make_temporary_folder(&prefix, || Err(io::Error::from_raw_os_error(5))).unwrap_err();
        assert_eq!(
            failed.to_string(),
            format!("EIO: i/o error, mkdtemp '{prefix}XXXXXX'")
        );
    }

    #[test]
    fn a_tree_a_file_a_link_and_nothing_are_removed_and_never_what_a_link_names() {
        let dir = tempfile::tempdir().unwrap();
        remove_all(&dir.path().join("missing")).unwrap();
        let tree = dir.path().join("tree");
        fs::create_dir_all(tree.join("inner")).unwrap();
        fs::write(tree.join("inner").join("file"), "x").unwrap();
        remove_all(&tree).unwrap();
        assert!(!tree.exists());
        let file = dir.path().join("file");
        fs::write(&file, "x").unwrap();
        remove_all(&file).unwrap();
        assert!(!file.exists());
        #[cfg(unix)]
        {
            let target = dir.path().join("target");
            fs::create_dir(&target).unwrap();
            fs::write(target.join("kept"), "x").unwrap();
            let link = dir.path().join("link");
            std::os::unix::fs::symlink(&target, &link).unwrap();
            remove_all(&link).unwrap();
            assert!(fs::symlink_metadata(&link).is_err());
            assert!(target.join("kept").exists());
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_removal_the_system_refuses_says_the_c_plus_plus_words() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let locked = dir.path().join("ro");
        fs::create_dir(&locked).unwrap();
        let kept = locked.join("kept");
        fs::write(&kept, "x").unwrap();
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o555)).unwrap();
        let said = remove_all(&kept).map_err(|failed| failed.to_string());
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
        if kept.exists() {
            // Probed on Node v26.8.1: `fs.rmSync` with `recursive` and `force`.
            assert_eq!(
                said.unwrap_err(),
                format!(
                    "EACCES, Permission denied: {} '{}'",
                    kept.display(),
                    kept.display()
                )
            );
        }
    }
}

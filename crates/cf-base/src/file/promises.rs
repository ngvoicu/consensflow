//! The `fs/promises` calls a channel makes, each failure said as Node's
//! error says it: a file read whole, renamed, and removed. Read against
//! Node v26.8.1 (`lib/internal/fs/promises.js`, `lib/internal/fs/rimraf.js`)
//! and probed there.

use std::fs::{self, File};
use std::io::Read;
use std::path::Path;

use super::errno::{errno_name, is_missing};
use super::error::FileError;

/// `readFile(path)`: the file's bytes, or the failure of its `open`, or of
/// its `read` (a folder opened on Unix), each with the path.
pub fn read_file(path: &Path) -> Result<Vec<u8>, FileError> {
    let mut file =
        File::open(path).map_err(|failed| FileError::call(failed, "open", Some(path)))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|failed| FileError::call(failed, "read", Some(path)))?;
    Ok(bytes)
}

/// `rename(from, to)`: the failure with both paths.
pub fn rename(from: &Path, to: &Path) -> Result<(), FileError> {
    fs::rename(from, to).map_err(|failed| FileError::call_to(failed, "rename", from, to))
}

/// `rm(path, { force: true })`, asked of a file: nothing there is no
/// failure; a folder is `ERR_FS_EISDIR`, no recursion having been asked
/// for; a failure to look is its `lstat`'s, and to remove its `unlink`'s.
/// An `EPERM` from `unlink` is Unix's word for a folder, which this was
/// not, and is said as it came; on Windows the file is made writable and
/// removed once more, as rimraf's `fixWinEPERM` does.
pub fn rm_force(path: &Path) -> Result<(), FileError> {
    match fs::symlink_metadata(path) {
        Err(failed) if is_missing(&failed) => return Ok(()),
        Err(failed) => return Err(FileError::call(failed, "lstat", Some(path))),
        Ok(found) if found.is_dir() => return Err(FileError::directory(path)),
        Ok(_) => {}
    }
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(failed) if is_missing(&failed) => Ok(()),
        Err(failed) if cfg!(windows) && errno_name(&failed) == Some("EPERM") => {
            writable_and_removed(path, failed)
        }
        Err(failed) => Err(FileError::call(failed, "unlink", Some(path))),
    }
}

/// rimraf's `fixWinEPERM` for a file: made writable, looked at again, and
/// removed; gone meanwhile is no failure, and any other failure on the way
/// is the first one.
fn writable_and_removed(path: &Path, first: std::io::Error) -> Result<(), FileError> {
    let unlink = |failed: std::io::Error| FileError::call(failed, "unlink", Some(path));
    let permissions = match fs::metadata(path) {
        Ok(found) => found.permissions(),
        Err(failed) if is_missing(&failed) => return Ok(()),
        Err(_) => return Err(unlink(first)),
    };
    let mut writable = permissions;
    #[allow(clippy::permissions_set_readonly_false)] // chmod 0o666: Windows' writable file.
    writable.set_readonly(false);
    match fs::set_permissions(path, writable) {
        Err(failed) if is_missing(&failed) => return Ok(()),
        Err(_) => return Err(unlink(first)),
        Ok(()) => {}
    }
    match fs::remove_file(path) {
        Err(failed) if is_missing(&failed) => Ok(()),
        removed => removed.map_err(unlink),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_read_says_the_open_that_failed_and_a_file_there_reads_whole() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing");
        assert_eq!(
            read_file(&missing).unwrap_err().to_string(),
            format!(
                "ENOENT: no such file or directory, open '{}'",
                missing.display()
            )
        );
        fs::write(dir.path().join("file"), "text").unwrap();
        assert_eq!(read_file(&dir.path().join("file")).unwrap(), b"text");
    }

    #[cfg(unix)]
    #[test]
    fn a_folder_opens_on_unix_and_its_read_fails_with_its_path() {
        let dir = tempfile::tempdir().unwrap();
        // Probed on Node v26.8.1: `fs.promises.readFile` of a folder.
        assert_eq!(
            read_file(dir.path()).unwrap_err().to_string(),
            format!(
                "EISDIR: illegal operation on a directory, read '{}'",
                dir.path().display()
            )
        );
    }

    #[test]
    fn a_rename_names_both_paths() {
        let dir = tempfile::tempdir().unwrap();
        let (from, to) = (dir.path().join("missing"), dir.path().join("b"));
        assert_eq!(
            rename(&from, &to).unwrap_err().to_string(),
            format!(
                "ENOENT: no such file or directory, rename '{}' -> '{}'",
                from.display(),
                to.display()
            )
        );
    }

    #[test]
    fn a_removal_forced_takes_nothing_for_gone_and_refuses_a_folder() {
        let dir = tempfile::tempdir().unwrap();
        rm_force(&dir.path().join("missing")).unwrap();
        let file = dir.path().join("file");
        fs::write(&file, "x").unwrap();
        rm_force(&file).unwrap();
        assert!(!file.exists());
        let failed = rm_force(dir.path()).unwrap_err();
        assert_eq!(failed.code(), "ERR_FS_EISDIR");
        assert_eq!(
            failed.to_string(),
            format!(
                "Path is a directory: rm returned EISDIR (is a directory) {}",
                dir.path().display()
            )
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_removal_the_system_refuses_says_its_unlink() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let locked = dir.path().join("ro");
        fs::create_dir(&locked).unwrap();
        let kept = locked.join("kept");
        fs::write(&kept, "x").unwrap();
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o555)).unwrap();
        let said = rm_force(&kept).map_err(|failed| failed.to_string());
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
        if kept.exists() {
            // Probed on Node v26.8.1: `fs.promises.rm` with `force`.
            assert_eq!(
                said.unwrap_err(),
                format!("EACCES: permission denied, unlink '{}'", kept.display())
            );
        }
    }
}

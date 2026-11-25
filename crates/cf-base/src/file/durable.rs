//! A file replaced so that the replacement is on the disk: the new file's
//! bytes flushed, then its rename flushed through the folder, so that a cut at
//! any moment leaves the old file or the new one, never a name with nothing
//! behind it.

use std::fs::File;
use std::io;
use std::path::Path;

/// The new file's bytes, flushed to the disk (`fsync`).
pub fn flush(file: &File) -> io::Result<()> {
    file.sync_all()
}

/// `from` renamed over `to`. On Windows the move is Microsoft's documented
/// durable one (`MoveFileExW` with `MOVEFILE_REPLACE_EXISTING` and
/// `MOVEFILE_WRITE_THROUGH`), which returns once the move is on the disk,
/// whatever the volume's filesystem; on Unix it is `rename`, which
/// [`flush_folder`] makes durable.
pub fn rename_durably(from: &Path, to: &Path) -> io::Result<()> {
    rename_through(from, to)
}

/// The file at `path` flushed to the disk, through a handle opened for writing: on Windows
/// a handle that only reads is refused (`Access is denied`) a flush.
pub fn flush_path(path: &Path) -> io::Result<()> {
    std::fs::OpenOptions::new().write(true).open(path)?.sync_all()
}

/// `from` put at `to` only if nothing is at `to`, written through: the name is taken or
/// `AlreadyExists` is answered, and nothing that was there is replaced (a file, a folder, a
/// link, dangling or not). A `from` that is not there is `NotFound`. The folder is flushed.
///
/// - Windows: `MoveFileExW` with `MOVEFILE_WRITE_THROUGH` and no leave to replace. `from`
///   must be closed: a file another handle holds is refused (a sharing violation).
/// - Unix: `hard_link`, then `from` removed. Where the file system makes no link (exFAT, FAT)
///   the name is claimed with an empty file made new, and `from` renamed over it: a cut
///   between those two steps leaves the empty claim, which the start that finds it refuses.
pub fn put_new(from: &Path, to: &Path) -> io::Result<()> {
    put_new_through(from, to)?;
    flush_folder(to.parent().unwrap_or_else(|| Path::new(".")))
}

/// The entries of `folder` flushed to the disk: a rename made in it is there
/// after a power cut. On Unix the folder is synced; on Windows it is opened for
/// writing with backup semantics and flushed, which NTFS honours and another
/// filesystem (ReFS) may answer without doing: the rename itself was made
/// write-through by [`rename_durably`].
pub fn flush_folder(folder: &Path) -> io::Result<()> {
    let folder = if folder.as_os_str().is_empty() {
        Path::new(".")
    } else {
        folder
    };
    flush_folder_of(folder)
}

#[cfg(unix)]
fn rename_through(from: &Path, to: &Path) -> io::Result<()> {
    std::fs::rename(from, to)
}

#[cfg(unix)]
fn put_new_through(from: &Path, to: &Path) -> io::Result<()> {
    match std::fs::hard_link(from, to) {
        Ok(()) => std::fs::remove_file(from),
        Err(failed)
            if matches!(
                failed.kind(),
                io::ErrorKind::AlreadyExists | io::ErrorKind::NotFound
            ) =>
        {
            Err(failed)
        }
        Err(_) => {
            // No links here: the name is claimed first, which only a free name allows.
            if !from.exists() {
                return Err(io::ErrorKind::NotFound.into());
            }
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(to)?;
            std::fs::rename(from, to).inspect_err(|_| {
                let _ = std::fs::remove_file(to);
            })
        }
    }
}

#[cfg(unix)]
fn flush_folder_of(folder: &Path) -> io::Result<()> {
    File::open(folder)?.sync_all()
}

#[cfg(windows)]
fn flush_folder_of(folder: &Path) -> io::Result<()> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Foundation::GENERIC_WRITE;
    use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_BACKUP_SEMANTICS;
    std::fs::OpenOptions::new()
        .access_mode(GENERIC_WRITE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(folder)?
        .sync_all()
}

#[cfg(windows)]
#[allow(unsafe_code)]
fn put_new_through(from: &Path, to: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{MoveFileExW, MOVEFILE_WRITE_THROUGH};
    let wide = |path: &Path| -> Vec<u16> { path.as_os_str().encode_wide().chain([0]).collect() };
    let (from, to) = (wide(from), wide(to));
    // SAFETY: both names are NUL-terminated and outlive the call, which keeps no pointer.
    let moved = unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), MOVEFILE_WRITE_THROUGH) };
    if moved == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// The one `unsafe` call of this crate: Windows has no safe wrapper of a move
/// that is written through to the disk, and `std::fs::rename` is not one.
#[cfg(windows)]
#[allow(unsafe_code)]
fn rename_through(from: &Path, to: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };
    let wide = |path: &Path| -> Vec<u16> { path.as_os_str().encode_wide().chain([0]).collect() };
    let (from, to) = (wide(from), wide(to));
    // SAFETY: both names are NUL-terminated and outlive the call, which keeps no pointer.
    let moved = unsafe {
        MoveFileExW(
            from.as_ptr(),
            to.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if moved == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;

    #[test]
    fn a_file_is_renamed_over_another_and_the_old_one_is_gone() {
        let dir = tempfile::tempdir().unwrap();
        let (new, old) = (dir.path().join("new"), dir.path().join("old"));
        fs::write(&old, "old").unwrap();
        let mut file = File::create(&new).unwrap();
        file.write_all(b"new").unwrap();
        flush(&file).unwrap();
        rename_durably(&new, &old).unwrap();
        flush_folder(dir.path()).unwrap();
        assert_eq!(fs::read_to_string(&old).unwrap(), "new");
        assert!(!new.exists());
    }

    #[test]
    fn a_name_is_taken_only_where_it_is_free() {
        let dir = tempfile::tempdir().unwrap();
        let at = |name: &str| dir.path().join(name);
        fs::write(at("staged"), "new").unwrap();
        put_new(&at("staged"), &at("free")).unwrap();
        assert_eq!(fs::read_to_string(at("free")).unwrap(), "new");
        assert!(!at("staged").exists(), "the staging name is gone");

        fs::write(at("staged"), "newer").unwrap();
        let taken = put_new(&at("staged"), &at("free")).unwrap_err();
        assert_eq!(taken.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read_to_string(at("free")).unwrap(), "new", "not replaced");
        assert!(at("staged").exists(), "a refused put leaves its source");

        fs::create_dir(at("folder")).unwrap();
        let taken = put_new(&at("staged"), &at("folder")).unwrap_err();
        assert_eq!(taken.kind(), io::ErrorKind::AlreadyExists);
        assert!(at("folder").is_dir());
    }

    #[cfg(unix)]
    #[test]
    fn a_dangling_link_is_a_name_taken() {
        let dir = tempfile::tempdir().unwrap();
        let at = |name: &str| dir.path().join(name);
        fs::write(at("staged"), "new").unwrap();
        std::os::unix::fs::symlink(at("nowhere"), at("dangling")).unwrap();
        let taken = put_new(&at("staged"), &at("dangling")).unwrap_err();
        assert_eq!(taken.kind(), io::ErrorKind::AlreadyExists);
        assert!(!at("nowhere").exists(), "nothing was made where the link leads");
        assert!(fs::symlink_metadata(at("dangling")).unwrap().file_type().is_symlink());
    }

    #[test]
    fn a_source_that_is_not_there_is_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let gone = put_new(&dir.path().join("none"), &dir.path().join("to")).unwrap_err();
        assert_eq!(gone.kind(), io::ErrorKind::NotFound);
        assert!(!dir.path().join("to").exists(), "no name was claimed");
    }

    #[test]
    fn a_closed_file_is_flushed_by_its_path() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a"), "x").unwrap();
        flush_path(&dir.path().join("a")).unwrap();
        assert_eq!(
            flush_path(&dir.path().join("none")).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
    }

    #[test]
    fn a_rename_that_cannot_be_made_is_a_failure() {
        let dir = tempfile::tempdir().unwrap();
        let failed = rename_durably(&dir.path().join("none"), &dir.path().join("to"));
        assert_eq!(failed.unwrap_err().kind(), io::ErrorKind::NotFound);
    }
}

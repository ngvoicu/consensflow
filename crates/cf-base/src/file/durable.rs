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

/// `from` renamed over `to`, and the rename flushed through the folder: on
/// Unix the folder is synced after the rename, on Windows the move is made
/// with write-through (`MOVEFILE_WRITE_THROUGH`), which returns once it is on
/// the disk.
pub fn rename_durably(from: &Path, to: &Path) -> io::Result<()> {
    rename_through(from, to)
}

#[cfg(unix)]
fn rename_through(from: &Path, to: &Path) -> io::Result<()> {
    std::fs::rename(from, to)?;
    let folder = match to.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    File::open(folder)?.sync_all()
}

#[cfg(windows)]
fn rename_through(from: &Path, to: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };
    let wide = |path: &Path| -> Vec<u16> { path.as_os_str().encode_wide().chain([0]).collect() };
    let (from, to) = (wide(from), wide(to));
    // SAFETY: both are NUL-terminated and live across the call.
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
        assert_eq!(fs::read_to_string(&old).unwrap(), "new");
        assert!(!new.exists());
    }

    #[test]
    fn a_rename_that_cannot_be_made_is_a_failure() {
        let dir = tempfile::tempdir().unwrap();
        let failed = rename_durably(&dir.path().join("none"), &dir.path().join("to"));
        assert_eq!(failed.unwrap_err().kind(), io::ErrorKind::NotFound);
    }
}

//! A file written whole or not at all, step by step as `saveDocument`
//! (`src/roster.js`) writes it, each step's failure said as the call of
//! Node's that failed.

use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use super::errno::{errno_name, is_missing};
use super::error::FileError;

/// Writes `bytes` to `path` whole or not at all, as `saveDocument` does: the
/// folder made, the bytes written beside the file to `<file>.<pid>.tmp`, then
/// renamed over it, so a write cut short (a crash, a full disk) leaves the
/// previous file. The file is made as `writeFileSync` makes one, with the
/// system's default permissions.
///
/// A failure is said as Node's call that failed says it: `mkdir` and the
/// folder, `open` and the temporary, `write` with no path, `rename` and the
/// two paths. On a failure of the write or the rename the temporary is
/// removed as `rmSync(temporary, { force: true })` removes it, and a failure
/// of that removal is said instead of the first one, as Node throws it.
pub fn write_whole(path: &Path, bytes: &[u8]) -> Result<(), FileError> {
    if let Some(folder) = path
        .parent()
        .filter(|folder| !folder.as_os_str().is_empty())
    {
        make_folder(folder)?;
    }
    let mut temporary = OsString::from(path.as_os_str());
    temporary.push(format!(".{}.tmp", std::process::id()));
    let temporary = PathBuf::from(temporary);
    write_new(&temporary, bytes)
        .and_then(|()| rename(&temporary, path))
        .map_err(|failure| remove(&temporary).err().unwrap_or(failure))
}

/// `mkdirSync(folder, { recursive: true })`, as Node's `MKDirpSync` walks it:
/// a folder that is not there is made after the one above it, and the path
/// in a failure is the whole one asked for, not the level that failed. A
/// level the system refuses for permission, space or because it is not a
/// directory ends the walk; one that is there already is a folder, or is
/// not, which is `EEXIST` for the path itself and `ENOTDIR` for a file in
/// the way of a level above. Reconstructed from Node's behaviour, not read
/// from its C++ (not at hand): the walk's Unix answers are probed, its
/// Windows ones are not.
fn make_folder(folder: &Path) -> Result<(), FileError> {
    let mut pending = vec![folder.to_path_buf()];
    while let Some(next) = pending.pop() {
        let Err(error) = fs::create_dir(&next) else {
            continue;
        };
        let name = mkdir_error_name(&error);
        let above = next
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty());
        match (name, above) {
            (Some("EACCES" | "ENOSPC" | "ENOTDIR" | "EPERM"), _) => {
                return Err(FileError::call(error, "mkdir", Some(folder)));
            }
            (Some("ENOENT"), Some(above)) => {
                let above = above.to_path_buf();
                pending.push(next);
                pending.push(above);
            }
            _ => match fs::metadata(&next) {
                Ok(there) if !there.is_dir() => {
                    let code = not_a_folder(name, !pending.is_empty());
                    return Err(FileError::named(code, error, "mkdir", Some(folder), None));
                }
                Ok(_) => {}
                Err(error) => return Err(FileError::call(error, "mkdir", Some(folder))),
            },
        }
    }
    Ok(())
}

/// What libuv's `mkdir` names a failure. On Windows a name the system will
/// not make (`ERROR_INVALID_NAME`, `ERROR_DIRECTORY`) is `EINVAL` there
/// (`fs__mkdir`, `src/win/fs.c`), where every other call says `ENOENT`: so
/// the walk asks what is there instead of climbing above it forever.
fn mkdir_error_name(error: &io::Error) -> Option<&'static str> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::{ERROR_DIRECTORY, ERROR_INVALID_NAME};
        let code = error
            .raw_os_error()
            .and_then(|code| u32::try_from(code).ok());
        if matches!(code, Some(ERROR_INVALID_NAME | ERROR_DIRECTORY)) {
            return Some("EINVAL");
        }
    }
    errno_name(error)
}

/// What Node says of a level that is there and is no folder, after the system
/// said `name` when the walk tried to make it. For the path asked for it is
/// that the file exists. When levels below it were still to be made, a file is
/// in the way of the path, and Node says that is not a directory. Unix says
/// so itself, as it looks a path up through the file, so only Windows gets
/// here with levels below.
fn not_a_folder(name: Option<&str>, levels_below: bool) -> &'static str {
    if name == Some("EEXIST") && levels_below {
        "ENOTDIR"
    } else {
        "EEXIST"
    }
}

/// `writeFileSync(temporary, text)`: the file made as the flag `w` makes it
/// (`open`, with the temporary's path), the bytes written to it, and the
/// file closed. Node checks the close, where a write the system deferred
/// (a full disk, a network share) fails at last, and throws there. Rust's
/// close says nothing of it, so the bytes are synced before the file is
/// let go, and a failure there is said as Node says a failed close: the
/// rename is never made over a file written short.
fn write_new(temporary: &Path, bytes: &[u8]) -> Result<(), FileError> {
    let mut file =
        File::create(temporary).map_err(|error| FileError::call(error, "open", Some(temporary)))?;
    write_all(&mut file, bytes)?;
    file.sync_all()
        .map_err(|error| FileError::call(error, "close", None))
}

/// The write of `writeFileSync`'s fast path for a string, which Node does in
/// C++ (`WriteFileUtf8`): libuv's `write`, called until all is written, and
/// said with no path, as its request is made with none. Node's own words
/// for a write that fails, probed on macOS with a file size limit: `EFBIG:
/// file too large, write`.
fn write_all(file: &mut impl Write, bytes: &[u8]) -> Result<(), FileError> {
    file.write_all(bytes)
        .map_err(|error| FileError::call(error, "write", None))
}

/// `renameSync(temporary, path)`.
fn rename(temporary: &Path, path: &Path) -> Result<(), FileError> {
    fs::rename(temporary, path)
        .map_err(|error| FileError::call_to(error, "rename", temporary, path))
}

/// `rmSync(path, { force: true })`, which asks for no recursion: nothing
/// there is no failure; a directory there is `ERR_FS_EISDIR`; and what
/// cannot be looked at (`lstat`, as Node's check calls it) or cannot be
/// removed is a failure of its own.
fn remove(path: &Path) -> Result<(), FileError> {
    let looked_at = match fs::symlink_metadata(path) {
        Ok(looked_at) => looked_at,
        Err(error) if is_missing(&error) => return Ok(()),
        Err(error) => return Err(FileError::call(error, "lstat", Some(path))),
    };
    if looked_at.is_dir() {
        return Err(FileError::directory(path));
    }
    match fs::remove_file(path) {
        Err(error) if !is_missing(&error) => Err(FileError::removal(error, path)),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests;

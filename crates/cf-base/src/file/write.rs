//! Files and folders made as Node makes them, each failure said as the call
//! of Node's that failed: a folder with every level above it, a file written
//! in place, and a file written whole or not at all, step by step as
//! `saveDocument` (`src/roster.js`) writes it.

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
        make_folder(folder, 0o777, Mkdir::Sync)?;
    }
    let mut temporary = OsString::from(path.as_os_str());
    temporary.push(format!(".{}.tmp", std::process::id()));
    let temporary = PathBuf::from(temporary);
    write_file(&temporary, bytes, 0o666)
        .and_then(|()| rename(&temporary, path))
        .map_err(|failure| remove(&temporary).err().unwrap_or(failure))
}

/// Which of Node's recursive `mkdir` calls a folder is made as. Both walk
/// the same levels and fail with the same codes (`MKDirpSync` and
/// `MKDirpAsync`, `src/node_file.cc` of Node v26.8.1); a failure names the
/// folder asked for after `mkdirSync`, and the level that failed after
/// `fs.promises.mkdir`, whose error is the last request's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mkdir {
    Sync,
    Promise,
}

/// `mkdir(folder, { recursive: true, mode })`, as Node walks it: a folder
/// that is not there is made after the one above it, with `mode` (less the
/// umask; Windows has none). A level the system refuses for permission,
/// space or because it is not a directory ends the walk; one that is there
/// already is a folder, or is not, which is `EEXIST` for the path itself and
/// `ENOTDIR` for a file in the way of a level above. `call` says which path
/// a failure names.
pub fn make_folder(folder: &Path, mode: u32, call: Mkdir) -> Result<(), FileError> {
    #[cfg(unix)]
    let builder = {
        let mut builder = fs::DirBuilder::new();
        std::os::unix::fs::DirBuilderExt::mode(&mut builder, mode);
        builder
    };
    #[cfg(not(unix))]
    let builder = {
        let _ = mode;
        fs::DirBuilder::new()
    };
    let mut pending = vec![folder.to_path_buf()];
    while let Some(next) = pending.pop() {
        let Err(error) = builder.create(&next) else {
            continue;
        };
        let name = mkdir_error_name(&error);
        let named = match call {
            Mkdir::Sync => folder,
            Mkdir::Promise => next.as_path(),
        };
        let above = next
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty());
        match (name, above) {
            (Some("EACCES" | "ENOSPC" | "ENOTDIR" | "EPERM"), _) => {
                return Err(FileError::call(error, "mkdir", Some(named)));
            }
            (Some("ENOENT"), Some(above)) => {
                let above = above.to_path_buf();
                pending.push(next);
                pending.push(above);
            }
            _ => match fs::metadata(&next) {
                Ok(there) if there.is_dir() => {}
                // The promised walk takes a level there already, with levels
                // still to make below it, for no folder whatever its look
                // says, a link to nothing among them (`MKDirpAsync`).
                _ if call == Mkdir::Promise && name == Some("EEXIST") && !pending.is_empty() => {
                    return Err(FileError::named(
                        "ENOTDIR",
                        error,
                        "mkdir",
                        Some(named),
                        None,
                    ));
                }
                Ok(_) => {
                    let code = not_a_folder(name, !pending.is_empty());
                    return Err(FileError::named(code, error, "mkdir", Some(named), None));
                }
                Err(error) => return Err(FileError::call(error, "mkdir", Some(named))),
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

/// `writeFile(path, bytes, { mode })`: the file opened as the flag `w` opens
/// it (`open`, with its path), made with `mode` (less the umask) when it is
/// not there and keeping its own when it is, the bytes written to it, and
/// the file closed, its close checked on Unix as Node checks it: a rename is
/// never made over a file written short. Windows takes no mode here, where
/// libuv made a file read-only for a mode without the owner's write bit:
/// every caller asks for one its owner may write (`0o600`, `0o666`).
pub fn write_file(path: &Path, bytes: &[u8], mode: u32) -> Result<(), FileError> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, mode);
    #[cfg(not(unix))]
    let _ = mode;
    let mut file = options
        .open(path)
        .map_err(|error| open_failed(error, path))?;
    write_all(&mut file, bytes)?;
    close(file)
}

/// `open`'s failure for a file made anew, in Node's words. On Windows a
/// folder in its way is `EISDIR`, as libuv reads the answer its own open of
/// it gets (`fs__open`) and as Unix says it, where the system answers this
/// open with access denied.
fn open_failed(error: io::Error, path: &Path) -> FileError {
    if cfg!(windows) && fs::metadata(path).is_ok_and(|found| found.is_dir()) {
        return FileError::named("EISDIR", error, "open", Some(path), None);
    }
    FileError::call(error, "open", Some(path))
}

/// The close Node checks, where a write the system deferred (to a network
/// share) fails at last: `EIO: i/o error, close`. Rust's own close says
/// nothing of it. As libuv's close, an interrupted one is no failure: the
/// file is closed all the same.
#[cfg(unix)]
fn close(file: File) -> Result<(), FileError> {
    use std::os::fd::IntoRawFd;
    match nix::unistd::close(file.into_raw_fd()) {
        Ok(()) | Err(nix::errno::Errno::EINTR | nix::errno::Errno::EINPROGRESS) => Ok(()),
        Err(errno) => Err(FileError::call(
            io::Error::from_raw_os_error(errno as i32),
            "close",
            None,
        )),
    }
}

/// The close on Windows: `CloseHandle` gives no deferred write's failure,
/// which its cache makes later and says in no call, so the file is let go
/// as Rust lets it go.
#[cfg(windows)]
fn close(file: File) -> Result<(), FileError> {
    drop(file);
    Ok(())
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

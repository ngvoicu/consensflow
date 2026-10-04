//! Files as Node read and wrote them: which file a path names now (another
//! file renamed over it is another file), when it was last written, in
//! milliseconds as `mtimeMs` has it, what a failure is called (`error.code`,
//! `ENOENT`), and a file written whole or not at all.

use std::ffi::OsString;
use std::fs::{self, File, Metadata};
use std::io;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

/// Which file an open file is, whatever its path: its device and inode on
/// Unix, its volume and file index on Windows (Node's `stat` calls the
/// index `ino` there).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Identity {
    volume: u64,
    index: u64,
}

/// The identity of `file`, read from its open handle.
#[cfg(unix)]
pub fn identity(file: &File) -> io::Result<Identity> {
    use std::os::unix::fs::MetadataExt;
    let metadata = file.metadata()?;
    Ok(Identity {
        volume: metadata.dev(),
        index: metadata.ino(),
    })
}

/// The identity of `file`, read from its open handle.
#[cfg(windows)]
pub fn identity(file: &File) -> io::Result<Identity> {
    let information = winapi_util::file::information(file)?;
    Ok(Identity {
        volume: information.volume_serial_number(),
        index: information.file_index(),
    })
}

/// When the file was last written, as Node's `mtimeMs` computes it:
/// seconds times a thousand plus nanoseconds over a million, so the two
/// read the same double.
pub fn mtime_ms(metadata: &Metadata) -> io::Result<f64> {
    let modified = metadata.modified()?;
    let (sign, since) = match modified.duration_since(UNIX_EPOCH) {
        Ok(since) => (1.0, since),
        Err(before) => (-1.0, before.duration()),
    };
    // Lossless: seconds since the epoch fit a double's 53 bits for millions of years.
    #[allow(clippy::cast_precision_loss)]
    let seconds = since.as_secs() as f64;
    Ok(sign * (seconds * 1e3 + f64::from(since.subsec_nanos()) / 1e6))
}

/// What Node calls the failure behind `error` (`error.code`): the errno's
/// name, as libuv names a system's failure. On Unix the name of the errno;
/// on Windows the name libuv translates the system's code to, so access
/// denied is `EPERM` and a name no file can have is `ENOENT`, as Node says.
/// None for a failure the readers and writers here never meet.
pub fn errno_name(error: &io::Error) -> Option<&'static str> {
    system_name(error.raw_os_error()?)
}

/// Whether `error` says there is no such file, as Node's `ENOENT` does.
pub fn is_missing(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::NotFound || errno_name(error) == Some("ENOENT")
}

#[cfg(unix)]
fn system_name(code: i32) -> Option<&'static str> {
    Some(match code {
        libc::ENOENT => "ENOENT",
        libc::EACCES => "EACCES",
        libc::EPERM => "EPERM",
        libc::EISDIR => "EISDIR",
        libc::ENOTDIR => "ENOTDIR",
        libc::ELOOP => "ELOOP",
        libc::ENAMETOOLONG => "ENAMETOOLONG",
        libc::EIO => "EIO",
        libc::EMFILE => "EMFILE",
        libc::ENFILE => "ENFILE",
        libc::ENOMEM => "ENOMEM",
        libc::EBUSY => "EBUSY",
        libc::ENXIO => "ENXIO",
        libc::ENODEV => "ENODEV",
        libc::EINVAL => "EINVAL",
        libc::EOVERFLOW => "EOVERFLOW",
        libc::ETIMEDOUT => "ETIMEDOUT",
        libc::ESTALE => "ESTALE",
        libc::EAGAIN => "EAGAIN",
        libc::ENOSPC => "ENOSPC",
        libc::EROFS => "EROFS",
        libc::EEXIST => "EEXIST",
        libc::EXDEV => "EXDEV",
        _ => return None,
    })
}

/// libuv's `uv_translate_sys_error`, for the system codes a file meets.
#[cfg(windows)]
fn system_name(code: i32) -> Option<&'static str> {
    use windows_sys::Win32::Foundation::{
        ERROR_ACCESS_DENIED, ERROR_ALREADY_EXISTS, ERROR_BAD_PATHNAME, ERROR_CANT_ACCESS_FILE,
        ERROR_CANT_RESOLVE_FILENAME, ERROR_DIRECTORY, ERROR_DIRECTORY_NOT_SUPPORTED,
        ERROR_DISK_FULL, ERROR_FILENAME_EXCED_RANGE, ERROR_FILE_EXISTS, ERROR_FILE_NOT_FOUND,
        ERROR_INVALID_DRIVE, ERROR_INVALID_FUNCTION, ERROR_INVALID_NAME, ERROR_LOCK_VIOLATION,
        ERROR_NOT_ENOUGH_MEMORY, ERROR_NOT_SAME_DEVICE, ERROR_OUTOFMEMORY, ERROR_PATH_NOT_FOUND,
        ERROR_SHARING_VIOLATION, ERROR_TOO_MANY_OPEN_FILES, ERROR_WRITE_PROTECT,
    };
    let code = u32::try_from(code).ok()?;
    Some(match code {
        ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND | ERROR_INVALID_DRIVE | ERROR_INVALID_NAME
        | ERROR_BAD_PATHNAME | ERROR_DIRECTORY => "ENOENT",
        ERROR_ACCESS_DENIED => "EPERM",
        ERROR_CANT_ACCESS_FILE => "EACCES",
        ERROR_SHARING_VIOLATION | ERROR_LOCK_VIOLATION => "EBUSY",
        ERROR_TOO_MANY_OPEN_FILES => "EMFILE",
        ERROR_NOT_ENOUGH_MEMORY | ERROR_OUTOFMEMORY => "ENOMEM",
        ERROR_FILENAME_EXCED_RANGE => "ENAMETOOLONG",
        ERROR_CANT_RESOLVE_FILENAME => "ELOOP",
        ERROR_INVALID_FUNCTION | ERROR_DIRECTORY_NOT_SUPPORTED => "EISDIR",
        ERROR_DISK_FULL => "ENOSPC",
        ERROR_WRITE_PROTECT => "EROFS",
        ERROR_FILE_EXISTS | ERROR_ALREADY_EXISTS => "EEXIST",
        ERROR_NOT_SAME_DEVICE => "EXDEV",
        _ => return None,
    })
}

/// Writes `bytes` to `path` whole or not at all, as `saveDocument` does: the
/// folder made, the bytes written beside the file to `<file>.<pid>.tmp`,
/// then renamed over it, so a write cut short (a crash, a full disk) leaves
/// the previous file. On a failure the temporary goes and the first error
/// is said. The file is made as `writeFileSync` makes one, with the
/// system's default permissions.
pub fn write_whole(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(folder) = path
        .parent()
        .filter(|folder| !folder.as_os_str().is_empty())
    {
        fs::create_dir_all(folder)?;
    }
    let mut temporary = OsString::from(path.as_os_str());
    temporary.push(format!(".{}.tmp", std::process::id()));
    let temporary = PathBuf::from(temporary);
    fs::write(&temporary, bytes)
        .and_then(|()| fs::rename(&temporary, path))
        .inspect_err(|_| {
            // `rmSync(temporary, { force: true })`: one that is not there is no failure.
            let _ = fs::remove_file(&temporary);
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime};

    #[test]
    fn a_file_written_whole_replaces_the_one_there_and_leaves_nothing_beside_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("home").join("agents.json");
        write_whole(&path, b"{}\n").unwrap();
        assert_eq!(
            fs::read(&path).unwrap(),
            b"{}\n",
            "the folder made, the file written"
        );
        let before = identity(&File::open(&path).unwrap()).unwrap();
        write_whole(&path, b"{\"agents\":[]}\n").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"{\"agents\":[]}\n");
        assert_ne!(
            identity(&File::open(&path).unwrap()).unwrap(),
            before,
            "another file renamed over it, never the old one written in place"
        );
        let left: Vec<_> = fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(left, ["agents.json"]);
    }

    #[test]
    fn a_write_that_fails_leaves_the_file_as_it_was_and_no_temporary() {
        let dir = tempfile::tempdir().unwrap();
        // A folder where the file should be: the rename over it fails.
        let path = dir.path().join("agents.json");
        fs::create_dir(&path).unwrap();
        fs::write(path.join("inside"), "kept").unwrap();
        assert!(write_whole(&path, b"{}\n").is_err());
        assert_eq!(fs::read_to_string(path.join("inside")).unwrap(), "kept");
        let left: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(left, ["agents.json"], "no temporary left beside it");
    }

    #[test]
    fn a_failure_is_called_what_node_calls_it() {
        let dir = tempfile::tempdir().unwrap();
        let missing = fs::read(dir.path().join("none")).unwrap_err();
        assert_eq!(errno_name(&missing), Some("ENOENT"));
        assert!(is_missing(&missing));
        let directory = fs::read(dir.path()).unwrap_err();
        // Unix opens a folder and refuses to read it; Windows refuses to open
        // it, as access denied: Node's word for that is EPERM.
        let word = errno_name(&directory);
        assert!(word == Some("EISDIR") || word == Some("EPERM"), "{word:?}");
        assert!(!is_missing(&directory));
        assert_eq!(errno_name(&io::Error::other("no system code")), None);
    }

    #[cfg(unix)]
    #[test]
    fn each_errno_a_file_meets_is_called_by_its_name_and_any_other_by_none() {
        let table = [
            (libc::ENOENT, "ENOENT"),
            (libc::EACCES, "EACCES"),
            (libc::EPERM, "EPERM"),
            (libc::EISDIR, "EISDIR"),
            (libc::ENOTDIR, "ENOTDIR"),
            (libc::ELOOP, "ELOOP"),
            (libc::ENAMETOOLONG, "ENAMETOOLONG"),
            (libc::EIO, "EIO"),
            (libc::EMFILE, "EMFILE"),
            (libc::ENFILE, "ENFILE"),
            (libc::ENOMEM, "ENOMEM"),
            (libc::EBUSY, "EBUSY"),
            (libc::ENXIO, "ENXIO"),
            (libc::ENODEV, "ENODEV"),
            (libc::EINVAL, "EINVAL"),
            (libc::EOVERFLOW, "EOVERFLOW"),
            (libc::ETIMEDOUT, "ETIMEDOUT"),
            (libc::ESTALE, "ESTALE"),
            (libc::EAGAIN, "EAGAIN"),
            (libc::ENOSPC, "ENOSPC"),
            (libc::EROFS, "EROFS"),
            (libc::EEXIST, "EEXIST"),
            (libc::EXDEV, "EXDEV"),
        ];
        for (errno, name) in table {
            assert_eq!(errno_name(&io::Error::from_raw_os_error(errno)), Some(name));
        }
        assert_eq!(errno_name(&io::Error::from_raw_os_error(libc::EHOSTUNREACH)), None);
    }

    #[cfg(windows)]
    #[test]
    fn on_windows_a_name_no_file_can_have_is_missing_as_node_says() {
        let dir = tempfile::tempdir().unwrap();
        let invalid = fs::read(dir.path().join("a<b").join("agents.json")).unwrap_err();
        assert_eq!(errno_name(&invalid), Some("ENOENT"));
        assert!(is_missing(&invalid));
    }

    #[test]
    fn a_file_is_itself_and_another_renamed_over_it_is_not() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("record.jsonl");
        std::fs::write(&path, "one\n").unwrap();
        let first = identity(&File::open(&path).unwrap()).unwrap();
        std::fs::write(&path, "one\ntwo\n").unwrap();
        assert_eq!(
            identity(&File::open(&path).unwrap()).unwrap(),
            first,
            "written in place, the same file"
        );
        let beside = dir.path().join("record.jsonl.new");
        std::fs::write(&beside, "other\n").unwrap();
        std::fs::rename(&beside, &path).unwrap();
        assert_ne!(identity(&File::open(&path).unwrap()).unwrap(), first);
    }

    #[test]
    fn a_write_time_reads_as_node_reads_it_in_milliseconds() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("record.jsonl");
        std::fs::write(&path, "x").unwrap();
        let file = File::options().write(true).open(&path).unwrap();
        let at = SystemTime::UNIX_EPOCH + Duration::from_millis(1_790_000_000_123);
        file.set_modified(at).unwrap();
        assert_eq!(
            mtime_ms(&file.metadata().unwrap()).unwrap(),
            1_790_000_000_123.0
        );
        let fraction = SystemTime::UNIX_EPOCH + Duration::new(1_790_000_000, 500_000);
        file.set_modified(fraction).unwrap();
        assert_eq!(
            mtime_ms(&file.metadata().unwrap()).unwrap(),
            1_790_000_000_000.5
        );
    }
}

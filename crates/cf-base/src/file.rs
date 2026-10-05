//! Files as Node read and wrote them: which file a path names now (another
//! file renamed over it is another file), when it was last written, in
//! milliseconds as `mtimeMs` has it, what a failure is called (`error.code`,
//! `ENOENT`) and how Node words it, and a file written whole or not at all.
//!
//! - `errno`: libuv's names and words for a failure;
//! - `error`: a failed file operation, said as Node's error says it;
//! - `write`: a file written whole, each step's failure said.

use std::fs::{self, File, Metadata};
use std::io;
use std::path::Path;
use std::time::UNIX_EPOCH;

mod errno;
mod error;
mod write;

pub use errno::{errno_name, error_code, is_missing, uv_words};
pub use error::FileError;
pub use write::write_whole;

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

/// What Node's `fs.stat` says of a path: which file it names, how long it
/// is, and when it was last written. It reads the file's attributes, never
/// its bytes, so a file the user may not read is stated all the same.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stat {
    pub identity: Identity,
    pub size: u64,
    pub mtime_ms: f64,
}

/// `fs.stat(path)`, following a link as it does.
#[cfg(unix)]
pub fn stat(path: &Path) -> io::Result<Stat> {
    use std::os::unix::fs::MetadataExt;
    let metadata = fs::metadata(path)?;
    Ok(Stat {
        identity: Identity {
            volume: metadata.dev(),
            index: metadata.ino(),
        },
        size: metadata.len(),
        mtime_ms: mtime_ms(&metadata)?,
    })
}

/// `fs.stat(path)`, following a link as it does: the path opened as
/// libuv's stat opens it, for its attributes alone, a folder too.
#[cfg(windows)]
pub fn stat(path: &Path) -> io::Result<Stat> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FILE_READ_ATTRIBUTES,
    };
    let file = fs::OpenOptions::new()
        .access_mode(FILE_READ_ATTRIBUTES)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)?;
    let metadata = file.metadata()?;
    Ok(Stat {
        identity: identity(&file)?,
        size: metadata.len(),
        mtime_ms: mtime_ms(&metadata)?,
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime};

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
    fn a_stat_says_what_an_open_file_says_without_opening_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("record.jsonl");
        std::fs::write(&path, "one\n").unwrap();
        let file = File::open(&path).unwrap();
        let stated = stat(&path).unwrap();
        assert_eq!(stated.identity, identity(&file).unwrap());
        assert_eq!(stated.size, 4);
        assert_eq!(
            stated.mtime_ms,
            mtime_ms(&file.metadata().unwrap()).unwrap()
        );
        assert!(stat(&dir.path().join("none")).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn a_file_the_user_may_not_read_is_stated_all_the_same() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("record.jsonl");
        std::fs::write(&path, "one\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
        assert_eq!(stat(&path).unwrap().size, 4);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
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

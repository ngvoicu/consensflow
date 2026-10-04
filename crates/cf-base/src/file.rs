//! Files as Node read and wrote them: which file a path names now (another
//! file renamed over it is another file), when it was last written, in
//! milliseconds as `mtimeMs` has it, what a failure is called (`error.code`,
//! `ENOENT`) and how Node words it, and a file written whole or not at all.
//!
//! - `errno`: libuv's names and words for a failure;
//! - `error`: a failed file operation, said as Node's error says it;
//! - `write`: a file written whole, each step's failure said.

use std::fs::{File, Metadata};
use std::io;
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

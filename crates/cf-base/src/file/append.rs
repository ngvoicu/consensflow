//! A file that is only ever appended to, kept to a size: what the daemon's
//! log and its trace both do (`src/core/log.js`, `src/core/trace.js`).

use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// Appends `text` to `file`, made if it is not there. A file already past
/// `limit` bytes is moved aside first, to `<file>.1`, which an earlier one
/// moved aside there is replaced by: one previous file is kept, and the file
/// that is written is never more than `limit` bytes and one append. The
/// size is read before each append, as Node read it, so the file grows past
/// the limit by at most one append.
///
/// A failure of either step is the caller's to ignore or to say: nothing is
/// appended after a failed move.
pub fn append_rotating(file: &Path, text: &str, limit: u64) -> io::Result<()> {
    // A file that cannot be looked at has no size: it is made, or fails below.
    let size = fs::metadata(file).map_or(0, |found| found.len());
    if size > limit {
        fs::rename(file, aside(file))?;
    }
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(file)?
        .write_all(text.as_bytes())
}

/// Where a file moved aside goes: its name and `.1`.
pub fn aside(file: &Path) -> PathBuf {
    let mut name = OsString::from(file.as_os_str());
    name.push(".1");
    PathBuf::from(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(file: &Path) -> String {
        fs::read_to_string(file).unwrap()
    }

    #[test]
    fn appends_in_order_and_makes_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("daemon.log");
        append_rotating(&file, "one\n", 100).unwrap();
        append_rotating(&file, "two\n", 100).unwrap();
        assert_eq!(read(&file), "one\ntwo\n");
        assert!(!aside(&file).exists());
    }

    #[test]
    fn a_file_past_the_limit_is_moved_aside_before_the_append_and_not_at_it() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("events.jsonl");
        // Exactly the limit is not past it.
        append_rotating(&file, "12345", 5).unwrap();
        append_rotating(&file, "6", 5).unwrap();
        assert_eq!(read(&file), "123456");
        assert!(!aside(&file).exists());
        // Now it is past it: it is the previous file, and a new one starts.
        append_rotating(&file, "7", 5).unwrap();
        assert_eq!(read(&aside(&file)), "123456");
        assert_eq!(read(&file), "7");
    }

    #[test]
    fn only_one_previous_file_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("daemon.log");
        for text in ["aaaa", "bbbb", "cccc"] {
            append_rotating(&file, text, 3).unwrap();
        }
        // The first went when the second was moved aside.
        assert_eq!(read(&aside(&file)), "bbbb");
        assert_eq!(read(&file), "cccc");
    }

    #[test]
    fn a_folder_that_is_not_there_is_a_failure_to_say() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("gone").join("daemon.log");
        assert_eq!(
            append_rotating(&file, "x\n", 100).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
    }

    #[test]
    fn a_move_that_fails_appends_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("daemon.log");
        append_rotating(&file, "long enough", 3).unwrap();
        // A folder where the previous file goes cannot be replaced by a file.
        fs::create_dir(aside(&file)).unwrap();
        assert!(append_rotating(&file, "more", 3).is_err());
        assert_eq!(read(&file), "long enough");
    }
}

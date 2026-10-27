//! A file that is only ever appended to, kept to a size: what the daemon's log
//! and its trace both do.

use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Held across an append's look at the size, its move and its write: two
/// threads that append to one file (the daemon's and the records' own, to the
/// daemon's log) would both move it aside at its limit, and the second would
/// move the first's new file over the previous one.
static APPENDING: Mutex<()> = Mutex::new(());

/// Appends `text` to `file`, made if it is not there. A file already past
/// `limit` bytes is moved aside first, to `<file>.1`, which an earlier one
/// moved aside there is replaced by: one previous file is kept, and the file
/// that is written is never more than `limit` bytes and one append. The
/// size is read before each append, as Node read it, so the file grows past
/// the limit by at most one append.
///
/// A failure of either step is the caller's to ignore or to say: nothing is
/// appended after a failed move. The appends of a process take turns, and so
/// do those of processes, through an advisory lock on a file beside it
/// ([`lock_beside`]): a second daemon starting on a home writes its first line
/// to the log before its ledger refuses it.
pub fn append_rotating(file: &Path, text: &str, limit: u64) -> io::Result<()> {
    // A thread that panicked holding it left nothing half done that matters here.
    let _turn = APPENDING
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _held = held(file);
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

/// The lock beside `file`, taken: released when it is dropped. None where it
/// cannot be made or taken (a home that cannot be written fails the append
/// itself), which leaves the append to take turns within its process only.
fn held(file: &Path) -> Option<File> {
    let beside = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(lock_beside(file))
        .ok()?;
    beside.lock().ok()?;
    Some(beside)
}

/// The file whose advisory lock the appends to `file` take: its name and
/// `.lock`, which is never moved aside.
pub fn lock_beside(file: &Path) -> PathBuf {
    let mut name = OsString::from(file.as_os_str());
    name.push(".lock");
    PathBuf::from(name)
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
    fn two_threads_that_append_at_the_limit_move_the_file_aside_once_and_lose_no_line() {
        // The daemon's log is written by the engine's thread and by the records'.
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("daemon.log");
        for round in 0..200 {
            fs::write(&file, "x".repeat(11)).unwrap();
            let _ = fs::remove_file(aside(&file));
            let start = std::sync::Barrier::new(2);
            std::thread::scope(|scope| {
                for line in ["a\n", "b\n"] {
                    let (file, start) = (&file, &start);
                    scope.spawn(move || {
                        start.wait();
                        append_rotating(file, line, 10).unwrap();
                    });
                }
            });
            assert_eq!(read(&aside(&file)), "x".repeat(11), "round {round}");
            let mut lines: Vec<String> = read(&file).lines().map(str::to_owned).collect();
            lines.sort();
            assert_eq!(lines, ["a", "b"], "round {round}");
        }
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

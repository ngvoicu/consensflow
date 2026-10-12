//! A look at a database that is in use. The daemon's ledger holds its file
//! exclusively (`PRAGMA locking_mode = EXCLUSIVE`), so while a run goes another
//! connection finds it locked; a copy of the file and of its write-ahead log is
//! read instead, and the one in use is never opened.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use rusqlite::Connection;

use crate::{Error, Result};

/// A folder of the run's own in the machine's temporary folder, removed with it.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Result<Self> {
        static MADE: AtomicU64 = AtomicU64::new(0);
        let folder = std::env::temp_dir().join(format!(
            "cf-eval-db-{}-{}",
            std::process::id(),
            MADE.fetch_add(1, Ordering::Relaxed)
        ));
        // One of an earlier run of the same process id is in the way.
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).map_err(Error::file("make", &folder))?;
        Ok(Self(folder))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// `read(db)` on a copy of the SQLite database `file` and its write-ahead log.
pub fn on_copy<T>(file: &Path, read: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
    let scratch = Scratch::new()?;
    let copy = scratch.0.join("copy.db");
    fs::copy(file, &copy).map_err(Error::file("copy", file))?;
    let wal = wal_of(file);
    if wal.exists() {
        fs::copy(&wal, wal_of(&copy)).map_err(Error::file("copy", &wal))?;
    }
    let db = Connection::open(&copy)?;
    read(&db)
}

/// Where the write-ahead log of the database `file` is: its name with `-wal`.
fn wal_of(file: &Path) -> PathBuf {
    let mut name = file.as_os_str().to_owned();
    name.push("-wal");
    PathBuf::from(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_copy_is_read_with_what_the_write_ahead_log_holds_and_the_original_is_left_alone() {
        let folder = tempfile::tempdir().unwrap();
        let file = folder.path().join("ledger.db");
        // A database in write-ahead mode that is still open, as the daemon's is.
        let live = Connection::open(&file).unwrap();
        live.query_row("PRAGMA journal_mode = WAL", [], |_| Ok(()))
            .unwrap();
        live.execute_batch("CREATE TABLE note (text TEXT); INSERT INTO note VALUES ('first');")
            .unwrap();
        // It is in the log and not yet in the file.
        let seen = on_copy(&file, |db| {
            Ok(db.query_row("SELECT text FROM note", [], |row| row.get::<_, String>(0))?)
        })
        .unwrap();
        assert_eq!(seen, "first");
        // The original takes more, and is still there to take it.
        live.execute_batch("INSERT INTO note VALUES ('second')")
            .unwrap();
        let count: i64 = live
            .query_row("SELECT COUNT(*) FROM note", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn the_folder_of_the_copy_goes_with_the_look_and_a_file_that_is_not_there_is_named() {
        let folder = tempfile::tempdir().unwrap();
        let mut at = None;
        let missing = folder.path().join("missing.db");
        let failed = on_copy(&missing, |_| Ok(())).unwrap_err();
        assert!(failed.to_string().contains("missing.db"), "{failed}");
        let file = folder.path().join("ledger.db");
        Connection::open(&file)
            .unwrap()
            .execute_batch("CREATE TABLE t (x INTEGER)")
            .unwrap();
        on_copy(&file, |db| {
            at = db.path().map(PathBuf::from);
            Ok(())
        })
        .unwrap();
        assert!(at.is_some_and(|copy| !copy.exists()));
    }
}

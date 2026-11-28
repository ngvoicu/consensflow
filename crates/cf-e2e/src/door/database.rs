//! The database door (K3, `live-kit-isolation-doors.md` section 3): a SQLite database is opened
//! only here, and only below a folder this run made. A read of one can write its sidecar files
//! (the journal, the WAL and the shared memory beside it), so a read is judged as a write of each.
//! The matrix's export of the ledger writes a second path, `VACUUM INTO`'s destination, which is
//! its own judged act ([`vacuum_into`], T-2002 correction 7).

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};

use super::place::Act;
use crate::{Error, Result};

/// The files SQLite may write beside `file`, and `file` itself.
fn with_sidecars(file: &Path) -> Vec<PathBuf> {
    ["", "-journal", "-wal", "-shm"]
        .into_iter()
        .map(|suffix| {
            let mut name: OsString = file.as_os_str().to_os_string();
            name.push(suffix);
            PathBuf::from(name)
        })
        .collect()
}

/// Judges `file` and its sidecars as writes below a folder this run made.
fn judged(file: &Path) -> Result {
    for path in with_sidecars(file) {
        super::judge(&path, Act::Write).map_err(Error::file("open the database", &path))?;
    }
    Ok(())
}

fn opened(file: &Path) -> impl Fn(rusqlite::Error) -> Error + '_ {
    move |failed| Error::Daemon(format!("the database {}: {failed}", file.display()))
}

/// The database `file`, opened to read: only below a folder this run made, with its sidecars.
pub fn read(file: impl AsRef<Path>) -> Result<Connection> {
    let file = file.as_ref();
    judged(file)?;
    Connection::open_with_flags(
        file,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(opened(file))
}

/// The database `file`, opened to write (made where it is not there): only below a folder this
/// run made, with its sidecars.
pub fn write(file: impl AsRef<Path>) -> Result<Connection> {
    let file = file.as_ref();
    judged(file)?;
    Connection::open(file).map_err(opened(file))
}

/// `VACUUM INTO` of `source` to `to`, the exact file a caller named for the copy: made new
/// ([`Act::Name`]), its sidecars judged as writes, and registered as a file this run made once
/// SQLite has written it.
pub fn vacuum_into(source: &Connection, to: &Path) -> Result {
    if std::fs::symlink_metadata(to).is_ok() {
        return Err(Error::file("make", to)(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "it is there already: a run makes the copy a caller names new and never writes over one",
        )));
    }
    super::judge(to, Act::Name).map_err(Error::file("make", to))?;
    for sidecar in with_sidecars(to).into_iter().skip(1) {
        super::judge(&sidecar, Act::Write).map_err(Error::file("make", &sidecar))?;
    }
    source
        .execute("VACUUM INTO ?1", [to.to_string_lossy().as_ref()])
        .map_err(opened(to))?;
    let made = std::fs::File::open(to).map_err(Error::file("open", to))?;
    super::register_file(to, &made).map_err(Error::file("make", to))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_database_is_opened_only_below_a_folder_of_the_runs_and_a_copy_is_made_new() {
        let run = super::super::made("db-door-").unwrap();
        let file = run.path().join("store.db");
        let db = write(&file).unwrap();
        db.execute("CREATE TABLE t (x)", []).unwrap();
        drop(db);
        assert!(read(&file).is_ok());
        let elsewhere = crate::standin_process::unowned_tempdir();
        let foreign = elsewhere.path().join("foreign.db");
        assert!(write(&foreign).is_err());
        assert!(read(&foreign).is_err());
        assert!(!foreign.exists());
        // The copy is its own judged act: new, and then the run's.
        let source = read(&file).unwrap();
        let copy = run.path().join("copy.db");
        vacuum_into(&source, &copy).unwrap();
        assert!(vacuum_into(&source, &copy).is_err(), "written over");
        assert!(read(&copy).is_ok());
        // Not into a folder of no run's, where SQLite's sidecars would be written beside it.
        let outside = elsewhere.path().join("copy.db");
        assert!(vacuum_into(&source, &outside).is_err());
        assert!(!outside.exists());
    }
}

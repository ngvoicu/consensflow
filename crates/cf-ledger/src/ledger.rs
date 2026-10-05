//! The ledger as its callers hold it (`index.js`): opened on a file, it
//! answers one operation at a time, each its concern's, and runs it on its
//! store, which no caller reaches. Its operations are in a file per concern:
//! projects, staff, conversations, tasks, messages, and what the page reads.

mod conversations;
mod messages;
mod page_reads;
mod projects;
mod staff;
mod tasks;

use std::path::Path;
use std::time::Duration;

use cf_base::time::{Clock, SystemClock};
use rusqlite::{Connection, ErrorCode};
use serde_json::Value;

use crate::model::LedgerError;
use crate::names;
use crate::schema::{migrate, MIGRATIONS};
use crate::store::Store;

/// An event as it is logged: when, of which project, what kind, and its data.
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    pub at: String,
    pub project: i64,
    pub kind: String,
    pub data: Value,
}

/// What a ledger is given: the time, a fresh name for each session, and
/// someone told every event as it is logged.
pub struct Options {
    pub clock: Box<dyn Clock>,
    pub names: Box<dyn FnMut() -> String>,
    pub trace: Box<dyn FnMut(&Event)>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            clock: Box::new(SystemClock),
            names: Box::new(|| names::session_name(names::random_unit)),
            trace: Box::new(|_| {}),
        }
    }
}

/// What a test does with the calls it watches ([`Ledger::watch`]): told the
/// call's name and the id it was given, it may fail it.
#[cfg(feature = "test-support")]
pub type Watcher = Box<dyn FnMut(&str, Option<i64>) -> Result<(), LedgerError>>;

/// The ledger of a home, open.
pub struct Ledger {
    store: Store,
    #[cfg(feature = "test-support")]
    watcher: std::cell::RefCell<Option<Watcher>>,
}

/// Opens the ledger at `file`, made or brought to this build's schema: the
/// one instance that holds it. Another holder refuses it (`ledger-locked`),
/// as does a file that is no ledger (`ledger-unreadable`) and one a newer
/// build wrote (`ledger-newer`).
pub fn open_ledger(file: &Path, options: Options) -> Result<Ledger, LedgerError> {
    let db = Connection::open(file)?;
    // SQLite's own default waits five seconds for a lock: a second instance
    // is refused at once instead. Node set both of these when it opened.
    db.busy_timeout(Duration::ZERO)?;
    db.execute_batch("PRAGMA foreign_keys = ON")?;
    let ready = (|| {
        db.execute_batch("PRAGMA locking_mode = EXCLUSIVE")?;
        // In exclusive mode the first write lock is kept until the connection
        // closes; taking it here is what makes this connection the only one.
        db.execute_batch("BEGIN EXCLUSIVE; COMMIT")?;
        db.execute_batch("PRAGMA journal_mode = WAL")?;
        db.execute_batch("PRAGMA synchronous = FULL")?;
        migrate(&db, &MIGRATIONS)
    })();
    if let Err(cause) = ready {
        drop(db);
        return Err(opening_refusal(file, cause));
    }
    Ok(Ledger {
        store: Store::new(db, options.clock, options.names, options.trace),
        #[cfg(feature = "test-support")]
        watcher: std::cell::RefCell::new(None),
    })
}

/// What an opening that failed tells: a held file, or one that is no ledger,
/// said as the ledger says it; anything else as it came.
fn opening_refusal(file: &Path, cause: LedgerError) -> LedgerError {
    let LedgerError::Sqlite(rusqlite::Error::SqliteFailure(failure, message)) = &cause else {
        return cause;
    };
    match failure.code {
        ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked => LedgerError::refused_with(
            "ledger-locked",
            format!("another ConsensFlow has {} open", file.display()),
            409,
        ),
        ErrorCode::DatabaseCorrupt | ErrorCode::NotADatabase => LedgerError::refused_with(
            "ledger-unreadable",
            format!(
                "{} is not a readable ConsensFlow ledger: {}",
                file.display(),
                message.clone().unwrap_or_else(|| failure.to_string())
            ),
            409,
        ),
        _ => cause,
    }
}

#[cfg(feature = "test-support")]
impl Ledger {
    /// Tells `watcher` of each read of `projects` and `next_delivery`, and
    /// each write of `pause_task` (by its task's number), before it is made:
    /// the engine's tests count them, or fail one.
    pub fn watch(&mut self, watcher: Watcher) {
        *self.watcher.get_mut() = Some(watcher);
    }

    /// Tells the watcher, if there is one, of the call about to be made.
    pub(crate) fn watched(&self, call: &str, id: Option<i64>) -> Result<(), LedgerError> {
        match self.watcher.borrow_mut().as_mut() {
            Some(watcher) => watcher(call, id),
            None => Ok(()),
        }
    }
}

impl Ledger {
    /// SQLite's own consistency check of the whole file: `ok` when sound.
    pub fn integrity(&mut self) -> Result<String, LedgerError> {
        Ok(self
            .store
            .db
            .query_row("PRAGMA integrity_check", [], |row| row.get(0))?)
    }

    /// Closes the file; the lock goes with it.
    pub fn close(self) -> Result<(), LedgerError> {
        self.store.db.close().map_err(|(_, cause)| cause.into())
    }
}

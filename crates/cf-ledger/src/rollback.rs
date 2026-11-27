//! The one place a transaction is rolled back. A rollback is not a formality:
//! one SQLite refuses leaves the transaction open, and what is done after it
//! joins that transaction and is never committed.

use rusqlite::Connection;

#[cfg(feature = "test-support")]
thread_local! {
    static REFUSED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Makes this thread's next rollback fail the way SQLite's own refusal does,
/// the transaction left open (T-2028 made one with an authorizer). A test's
/// stand-in for a disk that refuses.
#[cfg(feature = "test-support")]
pub fn refuse_next_rollback() {
    REFUSED.with(|refused| refused.set(true));
}

/// `ROLLBACK`, on a connection that is in a transaction; none is nothing to
/// undo, and says nothing.
pub(crate) fn roll_back(db: &Connection) -> Result<(), rusqlite::Error> {
    if db.is_autocommit() {
        return Ok(());
    }
    #[cfg(feature = "test-support")]
    if REFUSED.with(|refused| refused.replace(false)) {
        return Err(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_IOERR),
            Some("disk I/O error".to_owned()),
        ));
    }
    db.execute_batch("ROLLBACK")
}

/// What `first` is once the rollback after it is known: itself, or itself
/// with the rollback's failure added.
pub(crate) fn said(
    first: crate::model::LedgerError,
    rollback: Result<(), rusqlite::Error>,
) -> crate::model::LedgerError {
    match rollback {
        Ok(()) => first,
        Err(rollback) => crate::model::LedgerError::Unrolled {
            first: Box::new(first),
            rollback,
        },
    }
}

/// The words a panic carried, if it carried words.
pub(crate) fn words_of(payload: &Box<dyn std::any::Any + Send>) -> Option<String> {
    payload
        .downcast_ref::<&str>()
        .map(|words| (*words).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
}

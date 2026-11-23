//! The fact of a hold made for a retry (D3's N35). A turn that failed holds its task and main's
//! own Resume tries it again; nothing of the retry is pending anywhere else. The only thing kept
//! is this row, written with the hold, and its message once, when main's Resume of that hold is
//! queued. The tries a window has used are never counted: they are the try that the message last
//! pasted into the window carried ([`tries_carried`]).

use rusqlite::{params, OptionalExtension};

use crate::model::LedgerError;
use crate::store::Store;
use crate::views::TaskRow;

/// What a hold made for a retry keeps of the turn that failed.
#[derive(Debug, Clone, Copy)]
pub struct RetryFact<'a> {
    /// The window (a session) whose turn it was.
    pub session: i64,
    /// What turn it was for: `failed`.
    pub kind: &'a str,
    /// Which try the hold is for: 1 or 2.
    pub which: i64,
    /// What the turn said, and why it failed, as the engine read it.
    pub said: &'a str,
    pub cause: &'a str,
}

/// The fact of the hold the task has just been put in (its `paused_at` names that hold).
pub(crate) fn write(store: &Store, task: i64, fact: &RetryFact<'_>) -> Result<(), LedgerError> {
    store.db.execute(
        "INSERT INTO turn_retry (task_id, session_id, kind, try, said, cause, paused_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, (SELECT paused_at FROM task WHERE id = ?1))",
        params![
            task,
            fact.session,
            fact.kind,
            fact.which,
            fact.said,
            fact.cause
        ],
    )?;
    Ok(())
}

/// main's Resume of the hold `task` is in, queued as `message`: the fact of that hold, if it
/// is one made for a retry, names the message that carries it. A Resume of a person's, or of
/// any other hold, links nothing: the pause the fact names is the one the task is in.
pub(crate) fn link(store: &Store, task: &TaskRow, message: i64) -> Result<(), LedgerError> {
    let Some(paused_at) = &task.paused_at else {
        return Ok(());
    };
    store.db.execute(
        "UPDATE turn_retry SET message_id = ?1
          WHERE id = (SELECT id FROM turn_retry
                       WHERE task_id = ?2 AND paused_at = ?3 AND message_id IS NULL
                       ORDER BY id DESC LIMIT 1)",
        params![message, task.id, paused_at],
    )?;
    Ok(())
}

/// The try the message `message` carried: 1 or 2 where it is a retry's Resume, 0 for any other
/// message. The window's tries are read from this, of the message last pasted into it.
pub(crate) fn tries_carried(store: &Store, message: i64) -> Result<i64, LedgerError> {
    Ok(store
        .db
        .query_row(
            "SELECT try FROM turn_retry WHERE message_id = ?",
            [message],
            |row| row.get(0),
        )
        .optional()?
        .unwrap_or(0))
}

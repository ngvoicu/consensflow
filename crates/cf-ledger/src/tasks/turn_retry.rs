//! The fact of a hold made for a retry (D3's N35). A turn that failed holds its task and main's
//! own Resume tries it again; nothing of the retry is pending anywhere else. The only thing kept
//! is this row, written with the hold, and its message once, when main's Resume of that hold is
//! queued. The tries a task has used in a row are a number on the task, `turn_retries`: it goes
//! up by one in the write that queues a numbered Resume ([`link`]) and back to zero in the
//! write of a person's or the chief's action on the task, of its result, or of the first
//! failure 30 minutes after the last numbered Resume was delivered ([`standing`]).

use rusqlite::{params, OptionalExtension};

use crate::model::LedgerError;
use crate::store::Store;

/// What a hold made for a retry keeps of the turn that failed.
#[derive(Debug, Clone, Copy)]
pub struct RetryFact<'a> {
    /// The window (a session) whose turn it was.
    pub session: i64,
    /// What turn it was for: `failed`.
    pub kind: &'a str,
    /// What the turn said, and why it failed, as the engine read it.
    pub said: &'a str,
    pub cause: &'a str,
}

/// The fact of the hold the task has just been put in, and the task named as in it.
pub(crate) fn write(
    store: &Store,
    task: i64,
    which: i64,
    fact: &RetryFact<'_>,
) -> Result<(), LedgerError> {
    store.db.execute(
        "INSERT INTO turn_retry (task_id, session_id, kind, try, said, cause)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![task, fact.session, fact.kind, which, fact.said, fact.cause],
    )?;
    store.db.execute(
        "UPDATE task SET retry_hold = ?1 WHERE id = ?2",
        params![store.db.last_insert_rowid(), task],
    )?;
    Ok(())
}

/// The retry hold the task is in, by the ledger's own identity: none for any other hold, and
/// none once that hold has ended. Read before the move that ends it.
pub(crate) fn active(store: &Store, task: i64) -> Result<Option<i64>, LedgerError> {
    Ok(store
        .db
        .query_row("SELECT retry_hold FROM task WHERE id = ?", [task], |row| {
            row.get(0)
        })?)
}

/// main's Resume of the hold `task` was in (`hold`, [`active`] before the move), queued as
/// `message`: where that is a retry's hold, its fact names the message that carries it, and the
/// try is used in this write. Any other hold, a quota's or a busy model's, links nothing.
pub(crate) fn link(
    store: &Store,
    task: i64,
    hold: Option<i64>,
    message: i64,
) -> Result<(), LedgerError> {
    let Some(hold) = hold else {
        return Ok(());
    };
    let linked = store.db.execute(
        "UPDATE turn_retry SET message_id = ?1 WHERE id = ?2 AND message_id IS NULL",
        params![message, hold],
    )?;
    if linked > 0 {
        // The numbered Resume is queued: the try is used, in this same write. The column
        // refuses a third.
        store.db.execute(
            "UPDATE task SET turn_retries = turn_retries + 1 WHERE id = ?",
            [task],
        )?;
    }
    Ok(())
}

/// What the failed turn of a working task comes to, as the ledger holds it.
pub enum RetryHold {
    /// The task is held until the time its retry goes in; this is the try the hold is for.
    Held { which: i64 },
    /// The two tries in a row are spent: nothing was written, and the task is to fail.
    Spent,
}

/// How a failed turn's tries are paced: the unit (30 seconds as shipped), how many units the
/// second try waits, and how long after the last numbered Resume was delivered a failure
/// begins its tries again.
#[derive(Debug, Clone, Copy)]
pub struct RetryPlan {
    pub unit_ms: i64,
    pub second_units: i64,
    pub renew_after_ms: i64,
}

/// The tries a task has used in a row at this moment. Where the task's last numbered Resume that
/// was delivered (a later one queued and never delivered hides none) was `renew_after_ms` or more before `now`, they are over, and the count goes back to
/// zero in this write. The comparison is of two times and orders nothing by the clock: a clock
/// put back makes the time since smaller, so it can only delay the end. A Resume not yet
/// delivered is no time to renew from.
pub(crate) fn standing(
    store: &Store,
    task: i64,
    now: i64,
    renew_after_ms: i64,
) -> Result<i64, LedgerError> {
    let used: i64 = store.db.query_row(
        "SELECT turn_retries FROM task WHERE id = ?",
        [task],
        |row| row.get(0),
    )?;
    if used == 0 {
        return Ok(0);
    }
    let delivered: Option<String> = store
        .db
        .query_row(
            "SELECT m.delivered_at FROM turn_retry r JOIN message m ON m.id = r.message_id
              WHERE r.task_id = ? AND m.delivered_at IS NOT NULL ORDER BY r.id DESC LIMIT 1",
            [task],
            |row| row.get(0),
        )
        .optional()?
        .flatten();
    let over = delivered
        .as_deref()
        .and_then(cf_base::time::parse)
        .is_some_and(|at| now - at >= renew_after_ms);
    if over {
        reset(store, task)?;
        return Ok(0);
    }
    Ok(used)
}

/// The task's tries in a row start again: a person's or the chief's action on it, its result.
pub(crate) fn reset(store: &Store, task: i64) -> Result<(), LedgerError> {
    store.db.execute(
        "UPDATE task SET turn_retries = 0 WHERE id = ? AND turn_retries <> 0",
        [task],
    )?;
    Ok(())
}

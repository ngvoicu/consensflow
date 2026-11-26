//! The fact of a hold made for a retry (D3's N35). A turn that failed holds its task and main's
//! own Resume tries it again; nothing of the retry is pending anywhere else. The only thing kept
//! is this row, written with the hold, and its message once, when main's Resume of that hold is
//! queued. The tries a task has used in a row are a number on the task, `turn_retries`: it goes
//! up by one in the write that queues a numbered Resume ([`link`]) and goes back to zero on
//! exactly three things: a person's or the chief's action on the task, its result, and a failure
//! 30 minutes or more after `turn_retry_at` ([`standing`]), the last moment anything happened
//! to a numbered retry of the task. That time is written by the charge ([`link`]) and by the
//! delivery or withdrawal of the message that carries a numbered Resume (a trigger on the
//! message, whichever path moves it), in the same write, and by nothing else.

use rusqlite::params;

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
    store: &mut Store,
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
        let at = cf_base::time::iso(store.recent_ms());
        store.db.execute(
            "UPDATE task SET turn_retries = turn_retries + 1, turn_retry_at = ?1 WHERE id = ?2",
            params![at, task],
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
/// second try waits, and how long after `turn_retry_at` a failure
/// begins its tries again.
#[derive(Debug, Clone, Copy)]
pub struct RetryPlan {
    pub unit_ms: i64,
    pub second_units: i64,
    pub renew_after_ms: i64,
}

/// The tries a task has used in a row at this moment. Where `turn_retry_at` (the last moment
/// anything happened to a numbered retry of the task) is `renew_after_ms` or more before `now`,
/// they are over, and the count goes back to zero in this write. The comparison is of two times:
/// a clock put back makes the time since smaller, so it can only delay the end. A time that
/// cannot be read (a mark of this very write) renews nothing.
pub(crate) fn standing(
    store: &Store,
    task: i64,
    now: i64,
    renew_after_ms: i64,
) -> Result<i64, LedgerError> {
    let (used, at): (i64, Option<String>) = store.db.query_row(
        "SELECT turn_retries, turn_retry_at FROM task WHERE id = ?",
        [task],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    let over = at
        .as_deref()
        .and_then(cf_base::time::parse)
        .is_some_and(|at| now - at >= renew_after_ms);
    if used > 0 && over {
        start_again(store, task)?;
        return Ok(0);
    }
    Ok(used)
}

/// The task's tries in a row start again: a person's or the chief's action on it, its result,
/// or a failure 30 minutes after `turn_retry_at`. The time itself is not written here.
pub(crate) fn start_again(store: &Store, task: i64) -> Result<(), LedgerError> {
    store.db.execute(
        "UPDATE task SET turn_retries = 0 WHERE id = ? AND turn_retries != 0",
        [task],
    )?;
    Ok(())
}

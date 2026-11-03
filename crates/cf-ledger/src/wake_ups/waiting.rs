//! A chief that waits on the human is not asked, again and again, whether it still
//! has work (3.0.7-alpha.2, T-1113; the field board of 2026-11-02, where four nudges
//! in nine hours reached a chief whose only work was decisions it had put to the
//! human, worded the same each time, and the human turned the calls off).
//!
//! ConsensFlow cannot read from a chief's words that it waits: the last text before
//! one of the four nudges ended with a question, and before the other three it did
//! not. It reads what the board and the chief's copy say instead:
//!
//! - **The chief's newest note to the human is unread.** The human has not looked at
//!   what the chief said, so the chief's decisions wait, and a nudge asks it nothing it
//!   has not answered. The nudge is skipped and the skip is logged
//!   (`project.wake_up_skipped`, with `waitingOnTheHuman` naming the notes), at most
//!   [`MOST_SILENT_SKIPS`] intervals in a row: past that the nudge goes, so a board
//!   nobody reads is not silenced for good. The first wake-up after the calls are turned
//!   on or the project resumed always goes (the human is there).
//! - **The chief answered the last nudges only by writing to the human.** Nothing it
//!   did reached the staff or the board: it said it waited. The interval doubles for
//!   each such nudge in a row, at most [`MOST_DOUBLINGS`] times. A nudge the chief answered
//!   by acting (a word to a member, a task given, a decision on a task), or after which
//!   the human typed to the chief, ends the run.
//! - **The human typing to the chief restarts the interval** (`due::typed_origin`): a
//!   nudge never follows the human's words at once, as the fourth did, sixteen seconds
//!   after the human typed.

use rusqlite::params;

use super::due::begun_after;
use crate::conversations::typed_at;
use crate::model::LedgerError;
use crate::store::Store;

/// The most intervals in a row a wake-up is skipped for a chief that waits on the human.
pub(super) const MOST_SILENT_SKIPS: i64 = 3;

/// The most times the interval doubles after nudges the chief answered only by writing
/// to the human: at the most eight intervals apart.
pub(super) const MOST_DOUBLINGS: usize = 3;

/// The most notes of the chief's to the human a skip names.
const NAMED: i64 = 20;

/// The notes the chief wrote to the human that the human has not read, oldest first,
/// when the newest of the chief's notes to the human is one of them; none where it was
/// read, or the chief wrote none.
pub(super) fn unread_by_the_human(store: &Store, project_id: i64) -> Result<Vec<i64>, LedgerError> {
    let newest: Vec<(i64, String)> = store
        .db
        .prepare(
            "SELECT m.id, m.state FROM message m
               JOIN participant sender ON sender.id = m.sender_id
               JOIN participant reader ON reader.id = m.recipient_id
             WHERE m.project_id = ?1 AND sender.role = 'chief' AND reader.role = 'human'
               AND m.kind = 'note'
             ORDER BY m.id DESC LIMIT ?2",
        )?
        .query_map(params![project_id, NAMED], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<_>>()?;
    if newest.first().is_none_or(|(_, state)| state != "queued") {
        return Ok(Vec::new());
    }
    let mut unread: Vec<i64> = newest
        .into_iter()
        .filter(|(_, state)| state == "queued")
        .map(|(id, _)| id)
        .collect();
    unread.reverse();
    Ok(unread)
}

/// How many wake-ups the log says were skipped for a chief that waits on the human
/// since the event `event` (the last wake-up's): the intervals in a row.
pub(super) fn silent_skips_since(
    store: &Store,
    project_id: i64,
    event: i64,
) -> Result<i64, LedgerError> {
    Ok(store.db.query_row(
        "SELECT COUNT(*) FROM event WHERE project_id = ?1 AND id > ?2
           AND kind = 'project.wake_up_skipped'
           AND json_extract(data, '$.waitingOnTheHuman') IS NOT NULL",
        params![project_id, event],
        |row| row.get(0),
    )?)
}

/// How many times the interval doubles now: one for each of the newest wake-ups, in a
/// row, that the chief answered only by writing to the human.
pub(super) fn doublings(store: &Store, project_id: i64) -> Result<usize, LedgerError> {
    let nudges: Vec<(i64, String, i64)> = store
        .db
        .prepare(
            "SELECT id, at, json_extract(data, '$.message') FROM event
             WHERE project_id = ?1 AND kind = 'project.wake_up' ORDER BY id DESC LIMIT ?2",
        )?
        .query_map(params![project_id, MOST_DOUBLINGS as i64], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?
        .collect::<rusqlite::Result<_>>()?;
    if nudges.is_empty() {
        return Ok(0);
    }
    let chief = store.participant_by_handle(project_id, "chief")?;
    let mut run = 0;
    // The answer to a wake-up is what the log and the copy hold between it and the next.
    let mut next: Option<(i64, String)> = None;
    for (event, at, message) in nudges {
        // The calls turned on again, or a Resume: the human is back, and the run ends.
        if begun_after(store, project_id, event)? {
            break;
        }
        let until = next.as_ref().map(|(event, at)| (*event, at.as_str()));
        if !answered_only_to_the_human(store, project_id, chief.id, (event, &at), message, until)? {
            break;
        }
        run += 1;
        next = Some((event, at));
    }
    Ok(run)
}

/// Whether the chief's answer to the wake-up `nudge` (its event and its time), of the
/// message `message`, up to the next one `until`, was a word to the human and nothing
/// else: it wrote to the human; it wrote to no one else, gave no task and decided none;
/// and the human did not type to it.
fn answered_only_to_the_human(
    store: &Store,
    project_id: i64,
    chief: i64,
    nudge: (i64, &str),
    message: i64,
    until: Option<(i64, &str)>,
) -> Result<bool, LedgerError> {
    let (from, at) = nudge;
    let upper = until.map_or(i64::MAX, |(event, _)| event);
    let wrote = store.db.query_row(
        "SELECT EXISTS (SELECT 1 FROM event WHERE project_id = ?1 AND id > ?2 AND id < ?3
           AND kind = 'message.sent' AND json_extract(data, '$.from') = 'chief'
           AND json_extract(data, '$.to') = 'human')",
        params![project_id, from, upper],
        |row| row.get::<_, bool>(0),
    )?;
    if !wrote {
        return Ok(false);
    }
    let acted = store.db.query_row(
        "SELECT EXISTS (SELECT 1 FROM event WHERE project_id = ?1 AND id > ?2 AND id < ?3
           AND ((kind = 'message.sent' AND json_extract(data, '$.from') = 'chief'
                 AND json_extract(data, '$.to') IS NOT 'human')
                OR (kind IN ('task.created', 'task.opened')
                    AND json_extract(data, '$.from') = 'chief')
                OR (kind = 'task.state' AND json_extract(data, '$.by') = 'chief')))",
        params![project_id, from, upper],
        |row| row.get::<_, bool>(0),
    )?;
    if acted {
        return Ok(false);
    }
    let typed = typed_at(store, chief, at, until.map(|(_, at)| at), message)?;
    Ok(typed.is_none())
}

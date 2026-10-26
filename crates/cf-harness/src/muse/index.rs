//! Muse's index of its sessions, `session-index.db` (SQLite, WAL), as a word on whether
//! a session can be resumed where its log cannot be read: read-only, beside a Muse that
//! writes it, never holding it (each read is one connection with no wait on a writer,
//! its one query in a transaction that ends with it: `shared::record::sqlite`).
//!
//! The index is Muse's cache of its logs, not their record, and it lags them (Muse
//! 1.4.13 on echo, T-703, `tests/fixtures/muse/index-states.json`): a session has no
//! row while it runs, nor after it quit cleanly, until a later Muse process starts; the
//! row of a session resumed and quit is gone until the next one starts; and a session
//! killed at rest, its turn answered, reads as a segment that did not end
//! (`latest_segment_terminated = 0`), as one killed at work does. So the index says
//! yes, never no, and never whether a turn was cut: the log says those ([`resumable`];
//! a cut turn is the log's turn in flight, `Records::turn_cut`). A window quit before
//! its first message leaves neither a row nor a log.

use cf_base::env::Env;
use rusqlite::params;

use super::paths;
use crate::records::{Reading, Role};
use crate::shared::record::sqlite::Store;

/// What the index says of a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Row {
    /// How many prompts it holds (`prompt_count`), as of the index's last scan of it.
    pub(crate) prompts: i64,
}

/// The index's row of `session`, none where the index or the row is not there, or why
/// the index could not be read.
pub(crate) fn row(session: &str, env: &Env) -> Result<Option<Row>, String> {
    let index = paths::index(env)?;
    if !index.is_file() {
        return Ok(None);
    }
    let store = Store::open(&index)?;
    let found = store.read(|reads| {
        reads.get(
            "SELECT prompt_count FROM sessions WHERE session_id = ?1",
            params![session],
        )
    })?;
    Ok(found.map(|row| Row {
        prompts: row.get("prompt_count").map_or(0.0, |count| count.number()) as i64,
    }))
}

/// Whether a window can be opened again on the session whose log `reading` read, its
/// index row `row`: the log decides where it was read (yes where it holds a message
/// of the user's, as Devin's store does), and no where there is none, which a session
/// pruned by Muse's retention (`session_retention_days`) is, whatever a row left behind
/// says. Only a log that is there and unreadable asks the index: a row that counts a
/// prompt is a yes, and anything else (no row, none counted, the index unreadable)
/// tells nothing.
pub(crate) fn resumable(reading: &Reading, row: Result<Option<Row>, String>) -> Option<bool> {
    match reading {
        Reading::Known(record) => Some(record.items.iter().any(|item| item.role == Role::User)),
        Reading::Missing(_) => Some(false),
        Reading::Unknown(_) => match row {
            Ok(Some(Row { prompts })) if prompts > 0 => Some(true),
            Ok(_) | Err(_) => None,
        },
    }
}

#[cfg(test)]
mod tests;

//! The replacement of a role: the note ConsensFlow writes to a conversation
//! whose harness keeps its first instructions (Codex) when the role changed since
//! the conversation began, and what makes a message one. The ledger holds the role
//! text it says (`message.role_text`, a NULL in every other message), and a
//! replacement is known by that and by nothing in its words: a note of ConsensFlow
//! with no task is not enough, since the engine writes others to a chief (its
//! hand-off, the note that its turn was cut) that are no role, and a message that
//! quotes the words of one is none.

use cf_proto::ledger::MessageView;

use crate::model::LedgerError;
use crate::queue::{send, Sent};
use crate::store::Store;

/// A note from ConsensFlow to `to`, about no task, with the words `body`, that
/// says `role` to the conversation it goes to: it is a replacement of a role, and
/// the ledger knows it for one from here on.
pub(crate) fn note_role(
    store: &mut Store,
    project_id: i64,
    to: &str,
    body: &str,
    role: &str,
) -> Result<MessageView, LedgerError> {
    send(
        store,
        project_id,
        &Sent {
            to,
            body,
            kind: "note",
            role: Some(role),
            ..Sent::default()
        },
    )
}

/// The ids of the replacements of a role written to a participant, whatever
/// became of them, oldest first.
pub(crate) fn role_notes_to(store: &Store, participant_id: i64) -> Result<Vec<i64>, LedgerError> {
    Ok(store
        .db
        .prepare(
            "SELECT id FROM message WHERE recipient_id = ? AND role_text IS NOT NULL ORDER BY id",
        )?
        .query_map([participant_id], |row| row.get::<_, i64>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?)
}

//! Conversations: each participant's native conversations, one current at
//! a time and followed when the human switches its window to another
//! (`src/ledger/conversations.js`); the copy ConsensFlow keeps of each, item
//! by item (`transcripts`); and the chief's across a Switch chief, with what
//! a chief that takes over reads (`chief`).

mod chief;
mod transcripts;

use cf_proto::ledger::ConversationView;
use rusqlite::{params, OptionalExtension};
use serde_json::json;

use crate::model::{self, LedgerError};
use crate::store::Store;
use crate::views::conversation_view;

pub use chief::ChiefSwitch;
pub(crate) use chief::{chief_history, chief_open_work, history_read, last_switch, switch_chief};
pub use transcripts::TRANSCRIPT_ITEM_MAX;
pub(crate) use transcripts::{copied_item_with, copy_transcript, transcript};

/// A participant's new native conversation; the one before it ends.
pub(crate) fn start_conversation(
    store: &mut Store,
    participant_id: i64,
    harness: &str,
) -> Result<ConversationView, LedgerError> {
    model::require_harness(harness)?;
    store.write(|store| {
        let participant = store.participant_row(participant_id)?;
        let at = store.at();
        store.db.execute(
            "UPDATE conversation SET ended_at = ? WHERE participant_id = ? AND ended_at IS NULL",
            params![at, participant_id],
        )?;
        store.db.execute(
            "INSERT INTO conversation (participant_id, harness, started_at) VALUES (?, ?, ?)",
            params![participant_id, harness, at],
        )?;
        let id = store.db.last_insert_rowid();
        store.log(
            participant.project_id,
            "conversation.started",
            json!({ "participant": participant.handle, "conversation": id }),
        )?;
        known_conversation(store, id)
    })
}

/// The harness's own session a conversation runs in, once the harness says it.
pub(crate) fn bind_conversation(
    store: &mut Store,
    conversation_id: i64,
    native_session: &str,
) -> Result<ConversationView, LedgerError> {
    model::require_text(native_session, "native session", 512)?;
    store.write(|store| {
        let conversation = known_conversation(store, conversation_id)?;
        let taken = store
            .db
            .query_row(
                "SELECT id FROM conversation WHERE harness = ? AND native_session = ? AND id != ?",
                params![conversation.harness, native_session, conversation_id],
                |row| row.get::<_, i64>("id"),
            )
            .optional()?;
        if let Some(taken) = taken {
            return Err(LedgerError::refused_with(
                "native-session-taken",
                format!("native session {native_session} already belongs to conversation {taken}"),
                409,
            ));
        }
        store.db.execute(
            "UPDATE conversation SET native_session = ? WHERE id = ?",
            params![native_session, conversation_id],
        )?;
        let participant = store.participant_row(conversation.participant_id)?;
        store.log(
            participant.project_id,
            "conversation.bound",
            json!({ "conversation": conversation_id, "nativeSession": native_session }),
        )?;
        known_conversation(store, conversation_id)
    })
}

/// A conversation ends; one that has ended already, or none, stays as it is.
pub(crate) fn end_conversation(
    store: &mut Store,
    conversation_id: i64,
) -> Result<Option<ConversationView>, LedgerError> {
    store.write(|store| {
        let conversation = match conversation_by_id(store, conversation_id)? {
            Some(conversation) if conversation.ended_at.is_none() => conversation,
            other => return Ok(other),
        };
        let at = store.at();
        store.db.execute(
            "UPDATE conversation SET ended_at = ? WHERE id = ?",
            params![at, conversation_id],
        )?;
        let participant = store.participant_row(conversation.participant_id)?;
        store.log(
            participant.project_id,
            "conversation.ended",
            json!({ "conversation": conversation_id }),
        )?;
        conversation_by_id(store, conversation_id)
    })
}

/// A participant's conversation now, if one has not ended.
pub(crate) fn current_conversation(
    store: &Store,
    participant_id: i64,
) -> Result<Option<ConversationView>, LedgerError> {
    Ok(store
        .db
        .query_row(
            "SELECT * FROM conversation WHERE participant_id = ? AND ended_at IS NULL",
            [participant_id],
            conversation_view,
        )
        .optional()?)
}

/// A window the human switched to another conversation (/clear, /new,
/// /resume): the participant's conversation is the one on `native_session`
/// from now on, its own earlier one when it had it, else a new one bound to
/// it, and the one in progress ends. A session another participant's
/// conversation holds stays with it: the new conversation is left unbound.
pub(crate) fn follow_conversation(
    store: &mut Store,
    participant_id: i64,
    harness: &str,
    native_session: &str,
) -> Result<ConversationView, LedgerError> {
    model::require_harness(harness)?;
    model::require_text(native_session, "native session", 512)?;
    store.write(|store| {
        let participant = store.participant_row(participant_id)?;
        let held = store
            .db
            .query_row(
                "SELECT * FROM conversation WHERE harness = ? AND native_session = ?",
                params![harness, native_session],
                conversation_view,
            )
            .optional()?;
        let own = held
            .as_ref()
            .filter(|held| held.participant_id == participant_id);
        if let Some(own) = own.filter(|own| own.ended_at.is_none()) {
            return Ok(own.clone());
        }
        let own = own.map(|own| own.id);
        let at = store.at();
        store.db.execute(
            "UPDATE conversation SET ended_at = ? WHERE participant_id = ? AND ended_at IS NULL",
            params![at, participant_id],
        )?;
        let id = match own {
            Some(id) => {
                store
                    .db
                    .execute("UPDATE conversation SET ended_at = NULL WHERE id = ?", [id])?;
                id
            }
            None => {
                store.db.execute(
                    "INSERT INTO conversation (participant_id, harness, native_session, started_at)
           VALUES (?, ?, ?, ?)",
                    params![
                        participant_id,
                        harness,
                        held.is_none().then_some(native_session),
                        at
                    ],
                )?;
                store.db.last_insert_rowid()
            }
        };
        store.log(
            participant.project_id,
            "conversation.followed",
            json!({ "participant": participant.handle, "conversation": id, "nativeSession": native_session }),
        )?;
        known_conversation(store, id)
    })
}

fn conversation_by_id(store: &Store, id: i64) -> Result<Option<ConversationView>, LedgerError> {
    Ok(store
        .db
        .query_row(
            "SELECT * FROM conversation WHERE id = ?",
            [id],
            conversation_view,
        )
        .optional()?)
}

/// A conversation by its id, or a refusal naming it.
fn known_conversation(store: &Store, id: i64) -> Result<ConversationView, LedgerError> {
    conversation_by_id(store, id)?.ok_or_else(|| {
        LedgerError::refused_with("unknown-conversation", format!("no conversation {id}"), 404)
    })
}

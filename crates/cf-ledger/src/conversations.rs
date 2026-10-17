//! Conversations: each participant's native conversations, one current at a
//! time and followed when the human switches its window to another; the copy
//! ConsensFlow keeps of each, item by item (`transcripts`); the chief's
//! across a Switch chief, with what a chief that takes over reads (`chief`);
//! and a participant's whole history, for the human's viewer (`history`).

mod chief;
mod history;
mod transcripts;

use cf_base::text::utf16_len;
use cf_base::time;
use cf_proto::ledger::ConversationView;
use rusqlite::types::ValueRef;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::json;

use crate::model::{self, LedgerError};
use crate::store::Store;
use crate::views::conversation_view;

pub use chief::ChiefSwitch;
pub(crate) use chief::{chief_history, chief_open_work, history_read, last_switch, switch_chief};
pub(crate) use history::{message_opening, read_history, read_history_part};
pub use history::{
    HistoryAsk, HISTORY_FIND_MAX, HISTORY_PART, HISTORY_SCAN_BYTES, MESSAGE_OPENING, OPENING_COST,
};
pub use transcripts::TRANSCRIPT_ITEM_MAX;
pub(crate) use transcripts::{
    copied_item_with, copy_transcript, head_copied, split_copied, transcript, typed_since,
};

/// The most bytes a name the history reads holds: a conversation's harness, a
/// participant's agent (a harness's kind is eleven, an agent's id 64 at most at
/// every write), and a conversation's time that is no time. Migration 0019 cuts
/// what an older build wrote longer to it, at a character, with "…". A time the
/// history reads is kept as a store keeps one wherever it is written
/// ([`time::canonical`], a few dozen bytes), never cut.
pub const FIELD_MAX: usize = 256;

/// The most characters of an item's id a page shows (`id_shown`, in migration
/// 0019's `transcript_head`): a longer one shows those and "…". An id is an
/// item's identity, kept whole where it is stored and matched: a harness's own
/// are tens of characters, and none is bounded.
pub const ID_SHOWN: usize = 256;

/// The most characters (as JavaScript counts them) a conversation's native
/// session holds: a harness's session ids are tens. A longer one is refused
/// where it is bound; migration 0019 leaves a conversation an older build
/// bound to a longer one bound to none, as one the ledger never learned the
/// session of.
pub const NATIVE_SESSION_MAX: usize = 512;

/// The most bytes of data an event the history's headings read holds: where
/// a conversation began, was gone to or ended, and a Switch chief. What this
/// build logs of them is a handle, a conversation's id, a native session, and
/// harnesses and agents, and stays below it whatever they are (a native session
/// of characters JSON escapes, six bytes each, is 3,072 bytes at most);
/// migration 0019 writes what an older build logged longer as what a heading
/// reads of it, and the size it was (`cutFromBytes`).
pub const HEADING_EVENT_MAX: usize = 4_096;

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
    model::require_text(native_session, "native session", NATIVE_SESSION_MAX)?;
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

/// Leaves each conversation an older build bound to a native session longer
/// than [`NATIVE_SESSION_MAX`] characters bound to none: what migration 0019
/// does past its SQL. No harness gives such a session and none is bound to one
/// now, so none is lost: the conversation is one the ledger never learned the
/// session of, as one a window followed onto another's session is.
pub(crate) fn cut_sessions(db: &Connection) -> rusqlite::Result<()> {
    let long = db
        .prepare("SELECT id FROM conversation WHERE octet_length(native_session) > ?")?
        .query_map(
            [i64::try_from(NATIVE_SESSION_MAX).unwrap_or(i64::MAX)],
            |row| row.get::<_, i64>(0),
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut read = db.prepare("SELECT native_session FROM conversation WHERE id = ?")?;
    let mut unbind = db.prepare("UPDATE conversation SET native_session = NULL WHERE id = ?")?;
    for id in long {
        let session = read.query_row([id], |row| {
            Ok(crate::openings::text_of(row.get_ref(0)?.as_bytes()?))
        })?;
        if utf16_len(&session) > NATIVE_SESSION_MAX {
            unbind.execute([id])?;
        }
    }
    Ok(())
}

/// Brings within their ceilings, once, what an older build wrote of each
/// conversation and participant past them: what migration 0019 does past its
/// SQL, measuring each in bytes as it reads. A conversation's harness longer
/// than [`FIELD_MAX`] is cut to it ([`cut_to`]), and its native session is none,
/// so that two are never made one; its times are kept as a store keeps one
/// ([`time::canonical`]), and one that is no time is cut as a harness is, as
/// the conversation began, and ended, all the same. A participant's agent is
/// cut as a harness is; the time it is out until is kept as a store keeps one,
/// none where it is no time (which says it is not out, as before). The time of
/// each event a heading reads is kept as a conversation's is.
pub(crate) fn fields_within(db: &Connection) -> rusqlite::Result<()> {
    let text = |value: ValueRef<'_>| match value {
        ValueRef::Text(bytes) => Some(crate::openings::text_of(bytes)),
        _ => None,
    };
    let time_kept =
        |given: &str| time::canonical(given).unwrap_or_else(|| cut_to(given, FIELD_MAX));
    let mut conversations = Vec::new();
    let mut rows = db.prepare("SELECT id, harness, started_at, ended_at FROM conversation")?;
    let mut found = rows.query([])?;
    while let Some(row) = found.next()? {
        let (harness, started, ended) = (
            text(row.get_ref(1)?).unwrap_or_default(),
            text(row.get_ref(2)?).unwrap_or_default(),
            text(row.get_ref(3)?),
        );
        let kept = (
            cut_to(&harness, FIELD_MAX),
            time_kept(&started),
            ended.as_deref().map(time_kept),
        );
        if (&kept.0, &kept.1, &kept.2) != (&harness, &started, &ended) {
            conversations.push((row.get::<_, i64>(0)?, harness.len() > FIELD_MAX, kept));
        }
    }
    let mut keep = db.prepare(
        "UPDATE conversation SET harness = ?2, started_at = ?3, ended_at = ?4,
           native_session = CASE WHEN ?5 THEN NULL ELSE native_session END
         WHERE id = ?1",
    )?;
    for (id, cut, (harness, started, ended)) in conversations {
        keep.execute(params![id, harness, started, ended, cut])?;
    }
    let mut participants = Vec::new();
    let mut rows = db.prepare("SELECT id, agent, out_until FROM participant")?;
    let mut found = rows.query([])?;
    while let Some(row) = found.next()? {
        let (agent, until) = (text(row.get_ref(1)?), text(row.get_ref(2)?));
        let kept = (
            agent.as_deref().map(|agent| cut_to(agent, FIELD_MAX)),
            until.as_deref().and_then(time::canonical),
        );
        if (&kept.0, &kept.1) != (&agent, &until) {
            participants.push((row.get::<_, i64>(0)?, kept));
        }
    }
    let mut keep = db.prepare("UPDATE participant SET agent = ?2, out_until = ?3 WHERE id = ?1")?;
    for (id, (agent, until)) in participants {
        keep.execute(params![id, agent, until])?;
    }
    // The time of each event a heading reads, which its row holds before its
    // data: one longer than a time a store keeps can be (a few dozen bytes).
    let mut events = Vec::new();
    let mut rows = db.prepare(
        "SELECT id, at FROM event
       WHERE kind IN ('conversation.started', 'conversation.followed', 'conversation.ended', 'chief.switched')
         AND octet_length(at) > 64",
    )?;
    let mut found = rows.query([])?;
    while let Some(row) = found.next()? {
        let at = text(row.get_ref(1)?).unwrap_or_default();
        events.push((row.get::<_, i64>(0)?, time_kept(&at)));
    }
    let mut keep = db.prepare("UPDATE event SET at = ?2 WHERE id = ?1")?;
    for (id, at) in events {
        keep.execute(params![id, at])?;
    }
    Ok(())
}

/// `text` within `most` bytes: as it is, where it is no longer; else as much of
/// it as `most` holds with "…" after it, cut where a character ends.
pub(crate) fn cut_to(text: &str, most: usize) -> String {
    if text.len() <= most {
        return text.to_owned();
    }
    let mut end = most.saturating_sub('…'.len_utf8());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &text[..end])
}

/// The role text a conversation was last given (migration 0016): none where it
/// is not known, as for a conversation from before the column or one the
/// human's window was switched to. A conversation that is not there is refused.
pub(crate) fn conversation_role_text(
    store: &Store,
    conversation_id: i64,
) -> Result<Option<String>, LedgerError> {
    store
        .db
        .query_row(
            "SELECT role_text FROM conversation WHERE id = ?",
            [conversation_id],
            |row| row.get(0),
        )
        .optional()?
        .ok_or_else(|| unknown_conversation(conversation_id))
}

/// A conversation is given `text` as its role. It logs no event: it is written
/// at the start of every conversation of a harness that keeps its first
/// instructions, and no trace of Node's has one.
pub(crate) fn set_conversation_role_text(
    store: &mut Store,
    conversation_id: i64,
    text: &str,
) -> Result<(), LedgerError> {
    store.write(|store| {
        let written = store.db.execute(
            "UPDATE conversation SET role_text = ? WHERE id = ?",
            params![text, conversation_id],
        )?;
        if written == 0 {
            return Err(unknown_conversation(conversation_id));
        }
        Ok(())
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
    model::require_text(native_session, "native session", NATIVE_SESSION_MAX)?;
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
    conversation_by_id(store, id)?.ok_or_else(|| unknown_conversation(id))
}

/// The refusal for a conversation that is not there.
fn unknown_conversation(id: i64) -> LedgerError {
    LedgerError::refused_with("unknown-conversation", format!("no conversation {id}"), 404)
}

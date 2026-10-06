//! A member's sessions, each a named window from its first task until the
//! human deletes it, and after that for a follow-up, and whether one has
//! work on its hands.

use cf_proto::ledger::ProjectView;
use rusqlite::{params, OptionalExtension};
use serde_json::json;

use crate::conversations::current_conversation;
use crate::model::{sql_list, LedgerError, HELD_TASK_STATES};
use crate::projects::known_project;
use crate::store::Store;
use crate::views::ParticipantRow;

/// A member's new session: its own participant, named after the member,
/// with the member's agent, harness, designer flag and tier and the role
/// its task needs. It starts from nothing and ends with its work.
pub(crate) fn start_session(
    store: &mut Store,
    project_id: i64,
    member: &ParticipantRow,
    role: &str,
) -> Result<ParticipantRow, LedgerError> {
    let free = |store: &Store, handle: &str| -> Result<bool, LedgerError> {
        Ok(store
            .db
            .query_row(
                "SELECT 1 FROM participant WHERE project_id = ? AND handle = ?",
                params![project_id, handle],
                |_| Ok(()),
            )
            .optional()?
            .is_none())
    };
    let mut name = store.name();
    for _ in 1..16 {
        if free(store, &format!("{}-{name}", member.handle))? {
            break;
        }
        name = store.name();
    }
    // A name is never used twice (an ended session keeps its row), so a
    // member about a thousand sessions in draws only taken ones: the last
    // name drawn then takes the first number free.
    let mut handle = format!("{}-{name}", member.handle);
    let mut number = 2;
    while !free(store, &handle)? {
        handle = format!("{}-{name}-{number}", member.handle);
        number += 1;
    }
    let at = store.at();
    store.db.execute(
        "INSERT INTO participant (project_id, handle, role, roles, agent, harness, designer, tier, member_id, created_at)
       VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        params![
            project_id,
            handle,
            role,
            member.roles,
            member.agent,
            member.harness,
            member.designer,
            member.tier,
            member.id,
            at,
        ],
    )?;
    let id = store.db.last_insert_rowid();
    store.log(
        project_id,
        "session.started",
        json!({ "handle": handle, "member": member.handle, "role": role }),
    )?;
    store.participant_row(id)
}

/// Whether a session can take a follow-up: it is on the board, or it left and
/// may come back, its member being on the staff and its conversation, if it
/// had one, still there to resume. A member that left took the conversations
/// of the sessions it had on the board with it: none of those comes back with
/// its memory gone.
pub(crate) fn can_continue(store: &Store, session: &ParticipantRow) -> Result<bool, LedgerError> {
    if session.left_at.is_none() {
        return Ok(true);
    }
    let member_here = match session.member_id {
        Some(member) => store.participant_row(member)?.left_at.is_none(),
        None => false,
    };
    let had_one = store
        .db
        .query_row(
            "SELECT 1 FROM conversation WHERE participant_id = ?",
            [session.id],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    let lost = had_one && current_conversation(store, session.id)?.is_none();
    Ok(member_here && !lost)
}

/// A session that left the board comes back for its follow-up, its lane with
/// it: the row as it is now.
pub(crate) fn bring_back(
    store: &mut Store,
    session: ParticipantRow,
) -> Result<ParticipantRow, LedgerError> {
    if session.left_at.is_none() {
        return Ok(session);
    }
    store.db.execute(
        "UPDATE participant SET left_at = NULL WHERE id = ?",
        [session.id],
    )?;
    store.log(
        session.project_id,
        "session.returned",
        json!({ "handle": session.handle, "member": session.member_handle }),
    )?;
    store.participant_row(session.id)
}

/// The session that did T-`after`, with nothing on its hands: on the board,
/// or brought back to it if the human had deleted it.
pub(crate) fn continuable_session(
    store: &mut Store,
    project_id: i64,
    after: i64,
) -> Result<ParticipantRow, LedgerError> {
    let previous = store.task_row(project_id, after)?;
    let found = previous
        .assignee_id
        .map(|id| store.participant_row(id))
        .transpose()?
        .filter(|session| session.member_id.is_some());
    let session = match found {
        Some(session) if can_continue(store, &session)? => session,
        _ => {
            return Err(LedgerError::refused_with(
                "session-ended",
                format!(
                    "the session that did T-{after} has ended: open the task for its tier instead"
                ),
                409,
            ))
        }
    };
    require_free(store, &session)?;
    bring_back(store, session)
}

/// A member session takes one task at a time: one with a task on its hands
/// is refused another, a follow-up or a task sent back to it alike, so that
/// its window is never at work on one task while the board speaks of another.
pub(crate) fn require_free(store: &Store, session: &ParticipantRow) -> Result<(), LedgerError> {
    if holds_work(store, session.id)? {
        return Err(LedgerError::refused_with(
            "session-busy",
            format!(
                "@{} is still on its work: wait for its result, or open the task for its tier",
                session.handle
            ),
            409,
        ));
    }
    Ok(())
}

/// Whether a member has a task on its hands: one task per member session
/// ends when this is false. Paused work counts: its window stays for the
/// resumption.
pub(crate) fn holds_work(store: &Store, participant_id: i64) -> Result<bool, LedgerError> {
    has_task_in(
        store,
        participant_id,
        &[&HELD_TASK_STATES[..], &["paused"]].concat(),
    )
}

/// Whether a participant is assigned a task in one of `states`.
fn has_task_in(store: &Store, participant_id: i64, states: &[&str]) -> Result<bool, LedgerError> {
    Ok(store
        .db
        .query_row(
            &format!(
                "SELECT 1 FROM task WHERE assignee_id = ? AND state IN ({})",
                sql_list(states)
            ),
            [participant_id],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}

/// A session leaves the board and its lane folds into its member's. Its
/// conversation closes with it, unless it is kept for a return
/// (`keep_conversation`).
pub(super) fn close_session(
    store: &mut Store,
    session: &ParticipantRow,
    reason: &str,
    keep_conversation: bool,
) -> Result<(), LedgerError> {
    let at = store.at();
    store.db.execute(
        "UPDATE participant SET left_at = ? WHERE id = ?",
        params![at, session.id],
    )?;
    if !keep_conversation {
        store.db.execute(
            "UPDATE conversation SET ended_at = ? WHERE participant_id = ? AND ended_at IS NULL",
            params![at, session.id],
        )?;
    }
    store.log(
        session.project_id,
        "session.ended",
        json!({ "handle": session.handle, "member": session.member_handle, "reason": reason }),
    )
}

/// A session is the human's to delete: it leaves the board, its lane folding
/// into its member's, and its conversation is kept, so a follow-up (`--after`,
/// a reopen) brings it back while its member is on the staff. One still
/// holding work (queued, working or waiting) is refused; paused work goes
/// back on the board when resumed.
pub(crate) fn end_session(
    store: &mut Store,
    project_id: i64,
    handle: &str,
    by: &str,
) -> Result<ProjectView, LedgerError> {
    store.write(|store| {
        store.participant_by_handle(project_id, by)?;
        let session = store.participant_by_handle(project_id, handle)?;
        if session.member_id.is_none() {
            return Err(LedgerError::refused_with(
                "not-a-session",
                format!("@{handle} is not a session"),
                409,
            ));
        }
        if has_task_in(store, session.id, &HELD_TASK_STATES)? {
            return Err(LedgerError::refused_with(
                "session-busy",
                format!("@{handle} still holds work: accept, cancel or pause it first"),
                409,
            ));
        }
        close_session(store, &session, &format!("ended by @{by}"), true)?;
        known_project(store, project_id)
    })
}

//! Vacation stops fresh assignments, independently of an out-of-quota hold.

use std::collections::HashSet;

use rusqlite::params;
use serde_json::json;

use super::{require_member_row, require_staff_role};
use crate::model::LedgerError;
use crate::store::Store;
use crate::tasks::release_inside;
use crate::views::participant_view;
use crate::ParticipantView;

/// The caller supplies the member's sessions it has proved unopened while
/// holding the corresponding engine steps. A ledger alone cannot know whether
/// a pane open is in flight. Only fresh pool assignments still queued for those
/// sessions are returned; reopen and resume are work already in that window.
pub(crate) fn send(
    store: &mut Store,
    project: i64,
    handle: &str,
    unopened: &[i64],
) -> Result<ParticipantView, LedgerError> {
    store.write(|store| {
        let member = store.participant_by_handle(project, handle)?;
        require_member_row(store, member.id, "goes on Vacation")?;
        require_staff_role(handle, &member)?;
        let sessions: HashSet<i64> = unopened.iter().copied().collect();
        for session in &sessions {
            let row = store.participant_row(*session)?;
            if row.project_id != project || row.member_id != Some(member.id) {
                return Err(LedgerError::refused_with(
                    "not-a-member-session",
                    format!("@{} is not a session of @{handle}", row.handle),
                    409,
                ));
            }
        }
        if member.vacation_since.is_none() {
            let at = store.at();
            store.db.execute("UPDATE participant SET vacation_since = ? WHERE id = ?", params![at, member.id])?;
            store.log(project, "member.vacation", json!({ "handle": handle, "since": at }))?;
        }
        let queued: Vec<(i64, i64)> = store.db.prepare(
            "SELECT t.number, t.assignee_id FROM task t JOIN participant s ON s.id = t.assignee_id
             WHERE t.project_id = ?1 AND s.member_id = ?2 AND t.state = 'queued' AND t.pool IS NOT NULL
               AND NOT EXISTS (SELECT 1 FROM conversation c WHERE c.participant_id = s.id)
               AND NOT EXISTS (SELECT 1 FROM message m WHERE m.task_id = t.id AND m.kind = 'task'
                               AND m.state IN ('delivering', 'delivered', 'read'))
               AND NOT EXISTS (SELECT 1 FROM message m JOIN event e
                               ON e.project_id = m.project_id AND e.kind = 'delivery.begun'
                                  AND json_extract(e.data, '$.message') = m.id
                               WHERE m.task_id = t.id AND m.kind = 'task' AND m.recipient_id = s.id)
               AND COALESCE((SELECT MAX(e.id) FROM event e WHERE e.project_id = t.project_id
                             AND e.kind = 'task.assigned' AND json_extract(e.data, '$.task') = t.number), 0)
                 > COALESCE((SELECT MAX(e.id) FROM event e WHERE e.project_id = t.project_id
                             AND e.kind = 'task.state' AND json_extract(e.data, '$.task') = t.number
                             AND json_extract(e.data, '$.from') IN ('paused', 'done', 'failed')
                             AND json_extract(e.data, '$.to') = 'queued'), 0)
             ORDER BY t.number"
        )?.query_map(params![project, member.id], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        for (number, session) in queued {
            if sessions.contains(&session) {
                release_inside(store, project, number, "its member went on Vacation", true)?;
            }
        }
        participant_view(&store.participant_row(member.id)?)
    })
}

/// Clear only Vacation. Quota holds and the work they hold are untouched.
pub(crate) fn bring_back(
    store: &mut Store,
    project: i64,
    handle: &str,
) -> Result<ParticipantView, LedgerError> {
    store.write(|store| {
        let member = store.participant_by_handle(project, handle)?;
        require_member_row(store, member.id, "returns from Vacation")?;
        require_staff_role(handle, &member)?;
        if member.vacation_since.is_some() {
            store.db.execute(
                "UPDATE participant SET vacation_since = NULL WHERE id = ?",
                [member.id],
            )?;
            store.log(
                project,
                "member.vacation_ended",
                json!({ "handle": handle }),
            )?;
        }
        participant_view(&store.participant_row(member.id)?)
    })
}

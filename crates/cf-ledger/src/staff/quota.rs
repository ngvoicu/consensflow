//! A member out of quota: until when it takes no work, and back before its reset.

use cf_base::time;
use cf_proto::ledger::ParticipantView;
use rusqlite::params;
use serde_json::json;

use crate::model::{self, quoted, LedgerError};
use crate::store::Store;
use crate::views::participant_view;

/// A member that ran out of quota takes no work until `until`; when it was
/// marked is its `outSince`. One already out stays out until the later of
/// the two resets, and keeps the time it was first marked: what its other
/// windows run into after that is still news for each of them.
pub(crate) fn mark_out(
    store: &mut Store,
    participant_id: i64,
    until: &str,
    reason: &str,
) -> Result<ParticipantView, LedgerError> {
    let Some(reset) = time::parse(until) else {
        return Err(LedgerError::refused(
            "invalid-time",
            format!("not a time: {}", quoted(until)),
        ));
    };
    model::require_text(reason, "reason", 1000)?;
    store.write(|store| {
        let member = store.participant_row(participant_id)?;
        let now = store.at();
        let out_until = member.out_until.as_deref().and_then(time::parse);
        let out = matches!((out_until, time::parse(&now)), (Some(until), Some(now)) if until > now);
        if out && out_until.is_some_and(|until| until >= reset) {
            return participant_view(&member);
        }
        let since = if out {
            member.out_since.clone()
        } else {
            Some(now)
        };
        store.db.execute(
            "UPDATE participant SET out_until = ?, out_since = ? WHERE id = ?",
            params![until, since, member.id],
        )?;
        store.log(
            member.project_id,
            "member.out",
            json!({ "handle": member.handle, "until": until, "reason": reason }),
        )?;
        participant_view(&store.participant_row(member.id)?)
    })
}

/// A member out of quota is back before its reset: one of its windows
/// answered again, or the human says so (another account, a bigger plan).
/// What its windows ran into before now is history, and the tasks held for
/// it, its own and its sessions', go on at once.
pub(crate) fn mark_back(
    store: &mut Store,
    participant_id: i64,
    because: &str,
) -> Result<ParticipantView, LedgerError> {
    model::require_text(because, "because", 1000)?;
    store.write(|store| {
        let member = store.participant_row(participant_id)?;
        let now = store.at();
        let back = match member.out_until.as_deref() {
            None => true,
            Some(until) => matches!((time::parse(until), time::parse(&now)), (Some(until), Some(now)) if until <= now),
        };
        if back {
            return participant_view(&member);
        }
        store.db.execute(
            "UPDATE participant SET out_until = NULL, out_since = ? WHERE id = ?",
            params![now, member.id],
        )?;
        store.db.execute(
            "UPDATE task SET held_until = ?
         WHERE state = 'paused' AND held_until IS NOT NULL AND held_until > ?
           AND assignee_id IN (SELECT id FROM participant WHERE id = ? OR member_id = ?)",
            params![now, now, member.id, member.id],
        )?;
        store.log(
            member.project_id,
            "member.back",
            json!({ "handle": member.handle, "because": because }),
        )?;
        participant_view(&store.participant_row(member.id)?)
    })
}

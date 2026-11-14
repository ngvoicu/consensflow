//! A member out of quota: until when it takes no work, and back before its reset.

use cf_base::time;
use cf_proto::ledger::ParticipantView;
use rusqlite::params;
use serde_json::json;

use super::HoldKind;
use crate::model::{self, quoted, LedgerError};
use crate::store::Store;
use crate::views::participant_view;

/// A member that ran out of quota takes no work until `until`, kept and logged
/// as a store keeps a time ([`time::canonical`]); when it was marked is its
/// `outSince`. One already out stays out until the later of the two resets,
/// and keeps the time it was first marked: what its other windows run into
/// after that is still news for each of them.
pub(crate) fn mark_out(
    store: &mut Store,
    participant_id: i64,
    until: &str,
    reason: &str,
) -> Result<ParticipantView, LedgerError> {
    // Compatibility with the current engine, whose four fixed reasons are its
    // existing interface. An unfamiliar reason is not guessed to be quota.
    let kind = match reason {
        "out of quota" => Some(HoldKind::Quota),
        "account unavailable" => Some(HoldKind::Account),
        "model busy" => Some(HoldKind::Busy),
        "model moved" => Some(HoldKind::Moved),
        _ => None,
    };
    mark_out_kind(store, participant_id, until, reason, kind)
}

/// Keep the kind with the end which wins; a shorter, later report must not
/// relabel the hold that is still in force.
pub(crate) fn mark_out_kind(
    store: &mut Store,
    participant_id: i64,
    until: &str,
    reason: &str,
    kind: Option<HoldKind>,
) -> Result<ParticipantView, LedgerError> {
    // Kept as a store keeps a time, however it was written.
    let (Some(reset), Some(until)) = (time::parse(until), time::canonical(until)) else {
        return Err(LedgerError::refused(
            "invalid-time",
            format!("not a time: {}", quoted(until)),
        ));
    };
    let until = until.as_str();
    model::require_text(reason, "reason", 1000)?;
    store.write(|store| {
        let member = store.participant_row(participant_id)?;
        let now = store.at();
        let out_until = member.out_until.as_deref().and_then(time::parse);
        let out = matches!((out_until, time::parse(&now)), (Some(until), Some(now)) if until > now);
        if out && out_until.is_some_and(|until| until >= reset) {
            if out_until == Some(reset) && member.out_kind.is_none() && kind.is_some() {
                store.db.execute(
                    "UPDATE participant SET out_kind = ? WHERE id = ?",
                    params![kind.map(HoldKind::as_str), member.id],
                )?;
                return participant_view(&store.participant_row(member.id)?);
            }
            return participant_view(&member);
        }
        let since = if out {
            member.out_since.clone()
        } else {
            Some(now)
        };
        store.db.execute(
            "UPDATE participant SET out_until = ?, out_since = ?, out_kind = ? WHERE id = ?",
            params![until, since, kind.map(HoldKind::as_str), member.id],
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
            "UPDATE participant SET out_until = NULL, out_since = ?, out_kind = NULL WHERE id = ?",
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

/// The kind kept with a participant's out mark.
pub(crate) fn hold_kind(
    store: &Store,
    participant_id: i64,
) -> Result<Option<HoldKind>, LedgerError> {
    let kind: Option<String> = store.db.query_row(
        "SELECT out_kind FROM participant WHERE id = ?",
        [participant_id],
        |row| row.get(0),
    )?;
    Ok(HoldKind::read(kind.as_deref()))
}

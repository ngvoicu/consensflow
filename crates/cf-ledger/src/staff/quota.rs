//! A member out of quota: until when it takes no work, and back before its reset.

use cf_base::time;
use cf_proto::ledger::ParticipantView;
use rusqlite::params;
use serde_json::json;

use super::HoldKind;
use crate::model::{self, quoted, LedgerError};
use crate::store::Store;
use crate::views::{participant_view, ParticipantRow};

/// Whether a member's old hold is for quota or an unavailable account, rather
/// than a moved model or a busy backoff. Old timed holds can lack a kind.
pub(crate) fn account_hold_for_edit(
    store: &Store,
    member: &ParticipantRow,
) -> Result<bool, LedgerError> {
    let Some(until) = member.out_until.as_deref() else {
        return Ok(false);
    };
    if until == "9999-12-31T23:59:59.999Z" {
        return Ok(false);
    }
    if until.starts_with("9999-") {
        return Ok(true);
    }
    match HoldKind::read(member.out_kind.as_deref()) {
        Some(HoldKind::Quota | HoldKind::Account | HoldKind::Moved) => Ok(true),
        Some(HoldKind::Busy) => Ok(false),
        None => {
            let busy = store.db.query_row(
                "SELECT EXISTS(SELECT 1 FROM event e WHERE e.project_id=?1 AND e.kind='member.out'
                   AND e.at=?2 AND json_valid(e.data) AND json_extract(e.data, '$.reason')='model busy'
                   AND (json_extract(e.data, '$.handle')=?3 OR json_extract(e.data, '$.handle') IN
                        (SELECT value FROM json_each(?4))))",
                params![member.project_id, member.out_since, member.handle, member.former_handles],
                |row| row.get::<_, bool>(0),
            )?;
            Ok(!busy)
        }
    }
}

/// A member that ran out of quota takes no work until `until`, kept and logged
/// as a store keeps a time ([`time::canonical`]); when it was marked is its
/// `outSince`. A known quota or account hold keeps the earlier reset. Other
/// kinds and older untyped holds retain their existing precedence.
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
    mark_out_kind(store, participant_id, until, reason, kind, true)
}

/// Keep the kind with the end which wins; a shorter, later report must not
/// relabel the hold that is still in force.
pub(crate) fn mark_out_kind(
    store: &mut Store,
    participant_id: i64,
    until: &str,
    reason: &str,
    kind: Option<HoldKind>,
    reported_reset: bool,
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
        // Main identifies moved and unavailable holds by their sentinel times
        // even when the stored kind disagrees. Untyped old holds keep main's path.
        let prior_kind = HoldKind::read(member.out_kind.as_deref()).map(|kind| {
            match member.out_until.as_deref() {
                Some("9999-12-31T23:59:59.999Z") => HoldKind::Moved,
                Some(until) if until.starts_with("9999-") => HoldKind::Account,
                _ => kind,
            }
        });
        let account_hold = reported_reset
            && member.role != "chief"
            && matches!(prior_kind, Some(HoldKind::Quota | HoldKind::Account))
            && matches!(kind, Some(HoldKind::Quota | HoldKind::Account))
            && (prior_kind == kind || kind == Some(HoldKind::Quota));
        let keep_old = out
            && out_until.is_some_and(|old| {
                if account_hold {
                    old <= reset
                } else {
                    old >= reset
                }
            });
        if keep_old {
            if out_until == Some(reset) && member.out_kind.is_none() && kind.is_some() {
                store.db.execute(
                    "UPDATE participant SET out_kind = ? WHERE id = ?",
                    params![kind.map(HoldKind::as_str), member.id],
                )?;
                return participant_view(&store.participant_row(member.id)?);
            }
            return participant_view(&member);
        }
        if out && account_hold && out_until.is_some_and(|old| reset < old) {
            store.db.execute(
                "UPDATE task SET held_until = ?1 WHERE state = 'paused' AND held_until = ?2
                   AND assignee_id IN (SELECT id FROM participant WHERE id = ?3 OR member_id = ?3)
                   AND (SELECT json_extract(e.data, '$.because') FROM event e
                        WHERE e.project_id = task.project_id AND e.kind = 'task.state'
                          AND json_valid(e.data) AND json_extract(e.data, '$.task') = task.number
                        ORDER BY e.id DESC LIMIT 1) IN ('out of quota', 'account unavailable')",
                params![until, member.out_until, member.id],
            )?;
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

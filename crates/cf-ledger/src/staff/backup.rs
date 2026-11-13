//! A dedicated backup row for a member, independent of its sessions and quota.

use rusqlite::{params, OptionalExtension};
use serde_json::json;

use super::{add_participant, require_member_row, require_staff_role, NewParticipant};
use crate::model::{self, LedgerError};
use crate::projects::NewMember;
use crate::store::Store;
use crate::views::{participant_view, ParticipantRow};
use crate::ParticipantView;

fn primary(store: &Store, project: i64, handle: &str) -> Result<ParticipantRow, LedgerError> {
    let member = store.participant_by_handle(project, handle)?;
    require_member_row(store, member.id, "has a backup")?;
    require_staff_role(handle, &member)?;
    if member.backup_of.is_some() {
        return Err(LedgerError::refused_with(
            "backup-has-backup",
            format!("@{handle} is a backup and cannot have one of its own"),
            409,
        ));
    }
    Ok(member)
}

/// Join a new row for the backup. A departed backup is not reused: its old
/// sessions and quota stay attached to the participant that ran them.
pub(crate) fn add_backup(
    store: &mut Store,
    project: i64,
    handle: &str,
    backup: &NewMember,
) -> Result<ParticipantView, LedgerError> {
    store.write(|store| {
        let owner = primary(store, project, handle)?;
        let existing: Option<i64> = store
            .db
            .query_row(
                "SELECT id FROM participant WHERE backup_of = ? AND left_at IS NULL",
                [owner.id],
                |row| row.get(0),
            )
            .optional()?;
        if existing.is_some() {
            return Err(LedgerError::refused_with(
                "backup-exists",
                format!("@{handle} already has a backup"),
                409,
            ));
        }
        let held: Vec<String> = serde_json::from_str(&owner.roles)?;
        let roles = model::require_roles(&held, || model::printed(Some(&json!(held))))?;
        // Validate the backup's own harness, model and effort without treating it
        // as an independent holder of the owner's roles. This also lets a backup
        // of an image designer run on another harness, as the staff rule permits.
        let mut config = backup.clone();
        config.roles = vec![if config.designer {
            "designer"
        } else {
            "worker"
        }
        .to_owned()];
        config.tier = owner.tier.clone().unwrap_or_else(|| "light".to_owned());
        config.check()?;
        let backup_handle = config.handle(store, project)?;
        let mut joining = NewParticipant::member(&backup_handle, &roles, &config);
        joining.tier = owner.tier.as_deref();
        joining.backup_of = Some(owner.id);
        let added = add_participant(store, project, joining)?;
        store.log(
            project,
            "backup.added",
            json!({ "handle": added.handle, "member": handle }),
        )?;
        Ok(added)
    })
}

/// Stop using a backup without rewriting its sessions or tasks.
pub(crate) fn remove_backup(
    store: &mut Store,
    project: i64,
    handle: &str,
) -> Result<ParticipantView, LedgerError> {
    store.write(|store| {
        let owner = primary(store, project, handle)?;
        let backup: Option<i64> = store
            .db
            .query_row(
                "SELECT id FROM participant WHERE backup_of = ? AND left_at IS NULL",
                [owner.id],
                |row| row.get(0),
            )
            .optional()?;
        let id = backup.ok_or_else(|| {
            LedgerError::refused_with("no-backup", format!("@{handle} has no backup"), 409)
        })?;
        let at = store.at();
        store.db.execute(
            "UPDATE participant SET left_at = ? WHERE id = ?",
            params![at, id],
        )?;
        let backup = store.participant_row(id)?;
        store.log(
            project,
            "backup.left",
            json!({ "handle": backup.handle, "member": handle }),
        )?;
        participant_view(&backup)
    })
}

/// Replace a backup atomically: any refusal leaves the former relationship in place.
pub(crate) fn change_backup(
    store: &mut Store,
    project: i64,
    handle: &str,
    backup: &NewMember,
) -> Result<ParticipantView, LedgerError> {
    store.write(|store| {
        remove_backup(store, project, handle)?;
        add_backup(store, project, handle, backup)
    })
}

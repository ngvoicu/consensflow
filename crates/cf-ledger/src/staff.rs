//! A project's staff: members who join, change roles, follow the roster's tiers
//! and leave, in three concerns of their own: who may take a task (`pools`), a
//! member out of quota (`quota`), and the sessions a member's tasks run in
//! (`sessions`).

mod backup;
mod pools;
mod quota;
mod sessions;
mod vacation;

use cf_proto::ledger::{ParticipantView, RemovedMember};
use rusqlite::{params, params_from_iter, OptionalExtension};
use serde::Serialize;
use serde_json::json;

use crate::conversations::current_conversation;
use crate::messages::{kept_through_leaving, Dropped};
use crate::model::{self, sql_list, LedgerError};
use crate::projects::NewMember;
use crate::queue::{drop_queued, send, Sent};
use crate::roles;
use crate::store::Store;
use crate::tasks::tell_stranded_waiters;
use crate::views::{participant_view, ParticipantRow, TaskRow, PARTICIPANT_SELECT};

pub(crate) use backup::{add_backup, change_backup, remove_backup};
pub(crate) use pools::{candidates, has_members_of_tier, holders_of_tier, members, nearest_tier};
pub use pools::{BackupCandidate, Candidate, HoldKind};
pub(crate) use quota::{hold_kind, mark_back, mark_out, mark_out_kind};
use sessions::close_session;
pub(crate) use sessions::{
    bring_back, can_continue, end_session, follow_up_session, has_task_in_hand, require_free,
    start_session, undecided_results, Giving,
};
pub(crate) use vacation::{
    bring_back as return_from_vacation, release_unopened, send as send_on_vacation,
};

/// A staff row from the last project. Ids are included for rows in a backup
/// relationship so a new project can reconnect the two without guessing by model.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StaffMember {
    pub harness: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub model_name: Option<String>,
    pub designer: bool,
    pub tier: Option<String>,
    pub role: String,
    pub roles: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backup_of: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vacation_since: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub out_kind: Option<HoldKind>,
}

/// A participant joining a project: its handle, its role or its roles (the
/// first leading), the harness, model and effort it runs on, the name its harness
/// gives the model, and its tier.
pub(crate) struct NewParticipant<'a> {
    pub(crate) handle: &'a str,
    pub(crate) role: Option<&'a str>,
    pub(crate) roles: &'a [&'static str],
    pub(crate) harness: Option<&'a str>,
    pub(crate) model: Option<&'a str>,
    pub(crate) effort: Option<&'a str>,
    pub(crate) model_name: Option<&'a str>,
    pub(crate) designer: bool,
    pub(crate) tier: Option<&'a str>,
    pub(crate) backup_of: Option<i64>,
}

impl<'a> NewParticipant<'a> {
    /// The human, or the chief: a participant whose handle is its role.
    pub(crate) fn coordinator(role: &'a str, harness: Option<&'a str>) -> Self {
        Self {
            handle: role,
            role: Some(role),
            roles: &[],
            harness,
            model: None,
            effort: None,
            model_name: None,
            designer: false,
            tier: None,
            backup_of: None,
        }
    }

    /// A member of the staff: named by its model ([`crate::handles`]), which the caller
    /// made its handle.
    pub(crate) fn member(
        handle: &'a str,
        roles: &'a [&'static str],
        member: &'a NewMember,
    ) -> Self {
        Self {
            handle,
            role: None,
            roles,
            harness: Some(&member.harness),
            model: member.model.as_deref(),
            effort: member.effort.as_deref(),
            model_name: member.model_name.as_deref(),
            designer: member.designer,
            tier: Some(&member.tier),
            backup_of: None,
        }
    }
}

/// A participant joins under a handle nobody in the project has; a member's joining is logged.
pub(crate) fn add_participant(
    store: &mut Store,
    project_id: i64,
    joining: NewParticipant<'_>,
) -> Result<ParticipantView, LedgerError> {
    let role = joining
        .role
        .or_else(|| joining.roles.first().copied())
        .unwrap_or_default();
    store.project_row(project_id)?;
    let taken = store
        .db
        .query_row(
            "SELECT 1 FROM participant WHERE project_id = ? AND handle = ?",
            params![project_id, joining.handle],
            |_| Ok(()),
        )
        .optional()?;
    if taken.is_some() {
        return Err(LedgerError::refused_with(
            "member-exists",
            format!("{} is already in project {project_id}", joining.handle),
            409,
        ));
    }
    let at = store.at();
    store.db.execute(
        "INSERT INTO participant (project_id, handle, role, roles, harness, model, effort, model_name, designer, tier, created_at, backup_of)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        params![
            project_id,
            joining.handle,
            role,
            json!(joining.roles).to_string(),
            joining.harness,
            joining.model,
            joining.effort,
            joining.model_name,
            i64::from(joining.designer),
            joining.tier,
            at,
            joining.backup_of,
        ],
    )?;
    let id = store.db.last_insert_rowid();
    if role != "human" && role != "chief" {
        store.log(
            project_id,
            "member.added",
            json!({ "handle": joining.handle, "role": role, "roles": joining.roles, "harness": joining.harness }),
        )?;
    }
    participant_view(&store.participant_row(id)?)
}

/// A member joins the staff under a handle made from its model ([`crate::handles`]). One
/// that left the staff comes back as itself where it ran on the same harness, model, effort
/// and designer flag: under the handle it had, in the tier and the roles given now. A member
/// is known by what it runs on, never by its name: another whose name a departed member's
/// would be joins under a handle of its own.
pub(crate) fn add_member(
    store: &mut Store,
    project_id: i64,
    member: &NewMember,
) -> Result<ParticipantView, LedgerError> {
    let roles = member.check()?;
    store.write(|store| {
        let left = store
            .db
            .query_row(
                "SELECT id, handle FROM participant
                 WHERE project_id = ? AND left_at IS NOT NULL AND member_id IS NULL
                   AND role NOT IN ('human', 'chief')
                   AND backup_of IS NULL
                   AND harness = ? AND model IS ? AND effort IS ? AND designer = ?
                 ORDER BY id DESC LIMIT 1",
                params![
                    project_id,
                    member.harness,
                    member.model,
                    member.effort,
                    i64::from(member.designer)
                ],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?;
        let Some((id, handle)) = left else {
            let handle = member.handle(store, project_id)?;
            return add_participant(
                store,
                project_id,
                NewParticipant::member(&handle, &roles, member),
            );
        };
        store.db.execute(
            "UPDATE participant SET role = ?, roles = ?, harness = ?, model = ?, effort = ?,
           model_name = ?, designer = ?, tier = ?, left_at = NULL
         WHERE id = ?",
            params![
                roles[0],
                json!(roles).to_string(),
                member.harness,
                member.model,
                member.effort,
                member.model_name,
                i64::from(member.designer),
                member.tier,
                id
            ],
        )?;
        store.log(
            project_id,
            "member.added",
            json!({ "handle": handle, "roles": roles, "harness": member.harness, "rejoined": true }),
        )?;
        participant_view(&store.participant_row(id)?)
    })
}

/// A member's roles change in place. A role it gains must fit its agent;
/// one it holds already stays though it does not (a member from before an
/// image designer had to be an image agent), until the human drops it.
///
/// Its tier follows its roles (spec new-roles, FD-04): a member whose roles had no tier was
/// given none, so a role with tiers it gains comes with the `tier` the human chose, or is
/// refused; one left with roles of no tier is kept in the lowest, as one added so is; and one
/// that keeps a role with tiers keeps its tier, or takes the `tier` that comes.
pub(crate) fn set_roles<S: AsRef<str>>(
    store: &mut Store,
    project_id: i64,
    handle: &str,
    roles: &[S],
    tier: Option<&str>,
) -> Result<ParticipantView, LedgerError> {
    let roles = model::require_roles(roles, || {
        model::printed(Some(&json!(roles
            .iter()
            .map(AsRef::as_ref)
            .collect::<Vec<_>>())))
    })?;
    if let Some(tier) = tier {
        model::require_tier(tier)?;
    }
    store.write(|store| {
        let member = store.participant_by_handle(project_id, handle)?;
        require_member_row(store, member.id, "changes roles")?;
        require_staff_role(handle, &member)?;
        if member.backup_of.is_some() {
            return Err(LedgerError::refused_with(
                "backup-roles",
                format!("@{handle} is a backup: its roles follow its member"),
                409,
            ));
        }
        let held: Vec<String> = serde_json::from_str(&member.roles)?;
        let gained: Vec<&str> = roles
            .iter()
            .copied()
            .filter(|role| !held.iter().any(|kept| kept == role))
            .collect();
        model::require_fitting_roles(handle, member.designer == 1, &gained)?;
        let tier = tier_with_roles(handle, &held, &roles, &gained, tier, member.tier.as_deref())?;
        store.db.execute(
            "UPDATE participant SET role = ?, roles = ?, tier = ? WHERE id = ?",
            params![roles[0], json!(roles).to_string(), tier, member.id],
        )?;
        store.db.execute(
            "UPDATE participant SET role = ?, roles = ?, tier = ? WHERE backup_of = ? AND left_at IS NULL",
            params![roles[0], json!(roles).to_string(), tier, member.id],
        )?;
        store.log(
            project_id,
            "member.roles",
            json!({ "handle": handle, "roles": roles }),
        )?;
        participant_view(&store.participant_row(member.id)?)
    })
}

/// The tier a member of `held` has once it holds `roles` (see [`set_roles`]).
fn tier_with_roles(
    handle: &str,
    held: &[String],
    roles: &[&str],
    gained: &[&str],
    given: Option<&str>,
    had: Option<&str>,
) -> Result<Option<String>, LedgerError> {
    if roles::takes_no_tier(roles) {
        return Ok(Some(roles::NO_TIER.to_owned()));
    }
    if let Some(given) = given {
        return Ok(Some(given.to_owned()));
    }
    if !roles::takes_no_tier(held) {
        return Ok(had.map(str::to_owned));
    }
    let tiered = gained
        .iter()
        .find_map(|role| roles::find(role).filter(|row| row.tiered))
        .map_or_else(String::new, |row| row.a_name());
    Err(LedgerError::refused_with(
        "tier-required",
        format!("{handle} has no tier: {tiered}'s tasks have one, so its tier comes with the role"),
        400,
    ))
}

/// What Edit member changes of a member (T-1143): the harness, the model and the effort
/// it runs on, the name its harness gives the model, and the tier the human chose.
#[derive(Debug, Clone, PartialEq)]
pub struct MemberEdit {
    pub harness: String,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub model_name: Option<String>,
    pub tier: String,
}

/// A member runs on another harness, model, effort or tier from its next window (T-1143),
/// and so do its sessions: a window open now keeps what it runs on. Its roles, designer flag
/// and work stay; its name follows the model it runs on (HMD-12): the member and its windows
/// take the name its new model and effort make, and the names they leave are kept.
pub(crate) fn edit_member(
    store: &mut Store,
    project_id: i64,
    handle: &str,
    edit: &MemberEdit,
) -> Result<ParticipantView, LedgerError> {
    model::require_harness(&edit.harness)?;
    if let Some(model) = &edit.model {
        model::require_model_id(model)?;
    }
    model::require_tier(&edit.tier)?;
    store.write(|store| {
        let member = store.participant_by_handle(project_id, handle)?;
        require_member_row(store, member.id, "is edited")?;
        require_staff_role(handle, &member)?;
        if member.left_at.is_some() {
            return Err(LedgerError::refused_with(
                "not-a-member",
                format!("{handle} has left the staff: add it again instead"),
                409,
            ));
        }
        let tier = if let Some(owner) = member.backup_of {
            let owner = store.participant_row(owner)?;
            if owner.tier.as_deref().is_some_and(|tier| tier != edit.tier) {
                return Err(LedgerError::refused_with(
                    "backup-tier",
                    format!("@{handle} is a backup: its tier follows @{}", owner.handle),
                    409,
                ));
            }
            owner.tier
        } else {
            Some(edit.tier.clone())
        };
        store.db.execute(
            "UPDATE participant SET harness = ?, model = ?, effort = ?, model_name = ?, tier = ?,
                    needs_edit = 0
             WHERE id = ? OR member_id = ?",
            params![
                edit.harness,
                edit.model,
                edit.effort,
                edit.model_name,
                tier,
                member.id,
                member.id
            ],
        )?;
        if member.backup_of.is_none() {
            store.db.execute(
                "UPDATE participant SET tier = ? WHERE backup_of = ? AND left_at IS NULL",
                params![tier, member.id],
            )?;
        }
        store.log(
            project_id,
            "member.edited",
            json!({
                "handle": handle,
                "from": { "harness": member.harness, "model": member.model, "effort": member.effort, "tier": member.tier },
                "to": { "harness": edit.harness, "model": edit.model, "effort": edit.effort, "tier": tier },
            }),
        )?;
        // The name holds the model (HMD-12): a member on a model now is named by it, its
        // windows with it, a window at work too. One with no model keeps the name it has.
        if let Some(model) = &edit.model {
            let shown = crate::handles::model_shown(
                edit.model_name.as_deref(),
                Some(model),
                Some(&edit.harness),
            );
            let renamed = crate::handles::model_handle(
                &shown,
                edit.effort.as_deref(),
                &edit.harness,
                |candidate| {
                    candidate != member.handle
                        && crate::handles::handle_taken(&store.db, project_id, candidate)
                            .unwrap_or(true)
                },
            );
            if renamed != member.handle {
                let windows: i64 = store.db.query_row(
                    "SELECT COUNT(*) FROM participant WHERE member_id = ?",
                    [member.id],
                    |row| row.get(0),
                )?;
                crate::handles::rename_member(
                    &store.db,
                    project_id,
                    member.id,
                    &member.handle,
                    &renamed,
                )?;
                store.log(
                    project_id,
                    "member.renamed",
                    json!({ "from": member.handle, "to": renamed, "windows": windows }),
                )?;
            }
        }
        participant_view(&store.participant_row(member.id)?)
    })
}

/// A member leaves the staff. Its open tasks are cancelled, with the
/// messages still on their way to it and its unread questions; whoever
/// asked for those tasks is told, if its window runs, and then whoever told their
/// windows words they never had hears which went, as a cancel tells it, those
/// on their way included (`Dropped::leaving`; T-389), but the windows that left.
/// The words told for a paused task stay kept for it (`kept_through_leaving`).
/// The tasks it holds are
/// the queued, working and waiting ones, and the open ones given to it or to
/// one of its sessions that wait on the board for what they need (a follow-up
/// behind its window's work): such a task has no pool and no other window, so
/// its calling off is its only end, and it would otherwise be queued, once its
/// needs were accepted, for a window nothing opens again. A paused task is
/// left for its resumption. The removal is not refused for the tasks that wait
/// for what is cancelled: the chief is told of each of them in the same step,
/// as they will never start.
pub(crate) fn remove_member(
    store: &mut Store,
    project_id: i64,
    handle: &str,
) -> Result<RemovedMember, LedgerError> {
    store.write(|store| {
        let member = store.participant_by_handle(project_id, handle)?;
        require_member_row(store, member.id, "leaves the staff")?;
        require_staff_role(handle, &member)?;
        // A backup leaves with its member. Its history stays on its own row;
        // its active work and sessions leave together with the member's.
        let backup: Option<(i64, String)> = store.db.query_row(
            "SELECT id, handle FROM participant WHERE backup_of = ? AND left_at IS NULL",
            [member.id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).optional()?;
        let mut members = vec![member.id];
        if let Some((id, backup_handle)) = &backup {
            members.push(*id);
            let at = store.at();
            store.db.execute("UPDATE participant SET left_at = ? WHERE id = ?", params![at, id])?;
            store.log(project_id, "backup.left", json!({ "handle": backup_handle, "member": handle }))?;
        }
        let sessions = store
            .db
            .prepare(&format!(
                "{PARTICIPANT_SELECT} WHERE p.member_id IN ({}) AND p.left_at IS NULL ORDER BY p.id",
                vec!["?"; members.len()].join(", ")
            ))?
            .query_map(params_from_iter(&members), ParticipantRow::read)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let windows: Vec<i64> = members.iter().copied()
            .chain(sessions.iter().map(|session| session.id))
            .collect();
        let open = store
            .db
            .prepare(&format!(
                "SELECT t.*, q.handle AS requester FROM task t JOIN participant q ON q.id = t.requester_id
         WHERE t.assignee_id IN ({})
           AND t.state IN ('open', 'queued', 'working', 'waiting')
         ORDER BY t.number",
                vec!["?"; windows.len()].join(", ")
            ))?
            .query_map(params_from_iter(&windows), |row| {
                Ok((TaskRow::read(row)?, row.get::<_, String>("requester")?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let reason = format!("@{handle} left the staff");
        // Read before they go, as a cancel reads them (`call_off`), with what is
        // on its way: the windows leave with the tasks, so no launch's end tells.
        let dropped = open
            .iter()
            .map(|(task, _)| Dropped::leaving(store, task))
            .collect::<Result<Vec<_>, _>>()?;
        for (task, _) in &open {
            drop_queued(store, task.id)?;
            store.move_task(task, "cancelled", json!({ "reason": reason }))?;
        }
        for id in &windows {
            let kept = kept_through_leaving(store, *id, &reason)?;
            store.db.execute(
                "UPDATE message SET state = 'cancelled'
           WHERE ((recipient_id = ?1 AND state IN ('queued', 'delivering', 'gated'))
              OR (sender_id = ?1 AND kind = 'question' AND state IN ('queued', 'gated')))
             AND id NOT IN (SELECT value FROM json_each(?2))",
                params![id, json!(kept).to_string()],
            )?;
        }
        for session in &sessions {
            close_session(store, session, &reason, false)?;
        }
        let at = store.at();
        store.db.execute(
            "UPDATE participant SET left_at = ? WHERE id = ?",
            params![at, member.id],
        )?;
        let cancelled: Vec<i64> = open.iter().map(|(task, _)| task.number).collect();
        store.log(
            project_id,
            "member.left",
            json!({ "handle": handle, "cancelled": cancelled }),
        )?;
        // Only work that went with it is worth a word, and only to whoever asked for it.
        if !cancelled.is_empty() {
            let numbers: Vec<String> = cancelled.iter().map(|number| format!("T-{number}")).collect();
            let body = format!(
                "@{handle} left the staff; it takes no more tasks. Cancelled with it: {}.",
                numbers.join(", ")
            );
            let mut requesters: Vec<&str> = Vec::new();
            for (_, requester) in &open {
                if !requesters.contains(&requester.as_str()) {
                    requesters.push(requester);
                }
            }
            for requester in requesters.into_iter().filter(|requester| *requester != "human") {
                tell_if_running(store, project_id, requester, &body)?;
            }
        }
        // After that word, which names the tasks and never the words: each teller
        // hears of its own, the requester too, but the windows that left.
        let leaving: Vec<&str> = std::iter::once(handle)
            .chain(backup.iter().map(|(_, handle)| handle.as_str()))
            .chain(sessions.iter().map(|session| session.handle.as_str()))
            .collect();
        for ((task, _), dropped) in open.iter().zip(&dropped) {
            let lead = format!("T-{} was cancelled: {reason}.", task.number);
            dropped.tell_the_others(store, task, &lead, &leaving)?;
        }
        // Last: what waited on the work that went is told after the word of its going, which the
        // chief is given first (it is given its notes one at a time, the oldest first; a wait told
        // before its cause reached it as news with the cause unknown).
        for (task, _) in &open {
            tell_stranded_waiters(store, project_id, task)?;
        }
        Ok(RemovedMember {
            member: participant_view(&store.participant_row(member.id)?)?,
            cancelled,
            departed: windows,
        })
    })
}

/// The members of the newest project that has any: the staff a new project starts from.
pub(crate) fn last_staff(store: &Store) -> Result<Vec<StaffMember>, LedgerError> {
    let member = format!(
        "role IN ({})
    AND left_at IS NULL AND member_id IS NULL",
        sql_list(&roles::ids())
    );
    let rows = store
        .db
        .prepare(&format!(
            "SELECT id, harness, model, effort, model_name, designer, tier, role, roles,
                    backup_of, vacation_since, out_kind,
                    EXISTS (SELECT 1 FROM participant b WHERE b.backup_of = participant.id AND b.left_at IS NULL) AS has_backup
             FROM participant
       WHERE project_id = (SELECT MAX(project_id) FROM participant WHERE {member})
         AND {member}
       ORDER BY id"
        ))?
        .query_map([], |row| {
            Ok((
                StaffMember {
                    harness: row.get("harness")?,
                    model: row.get("model")?,
                    effort: row.get("effort")?,
                    model_name: row.get("model_name")?,
                    designer: row.get::<_, i64>("designer")? == 1,
                    tier: row.get("tier")?,
                    role: row.get("role")?,
                    roles: Vec::new(),
                    id: if row.get::<_, Option<i64>>("backup_of")?.is_some()
                        || row.get::<_, i64>("has_backup")? != 0
                    { Some(row.get("id")?) } else { None },
                    backup_of: row.get("backup_of")?,
                    vacation_since: row.get("vacation_since")?,
                    out_kind: HoldKind::read(row.get::<_, Option<String>>("out_kind")?.as_deref()),
                },
                row.get::<_, String>("roles")?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    rows.into_iter()
        .map(|(member, roles)| {
            Ok(StaffMember {
                roles: serde_json::from_str(&roles)?,
                ..member
            })
        })
        .collect()
}

/// A member of the staff, never one of its sessions.
pub(crate) fn require_member_row(
    store: &Store,
    participant_id: i64,
    does: &str,
) -> Result<ParticipantRow, LedgerError> {
    let row = store.participant_row(participant_id)?;
    if row.member_id.is_some() {
        return Err(LedgerError::refused_with(
            "not-a-member",
            format!(
                "@{} is a session of @{}: a member {does}",
                row.handle,
                row.member_handle.as_deref().unwrap_or_default()
            ),
            409,
        ));
    }
    Ok(row)
}

/// One of the staff's roles, not the project's human or chief.
fn require_staff_role(handle: &str, member: &ParticipantRow) -> Result<(), LedgerError> {
    if roles::is_role(member.role.as_str()) {
        return Ok(());
    }
    Err(LedgerError::refused_with(
        "not-a-member",
        format!(
            "{handle} is the project's {}, not a member of its staff",
            member.role
        ),
        409,
    ))
}

/// A note from ConsensFlow, for a participant whose window has already started.
fn tell_if_running(
    store: &mut Store,
    project_id: i64,
    handle: &str,
    body: &str,
) -> Result<(), LedgerError> {
    let running = store
        .db
        .query_row(
            "SELECT id FROM participant WHERE project_id = ? AND handle = ? AND left_at IS NULL",
            params![project_id, handle],
            |row| row.get::<_, i64>("id"),
        )
        .optional()?;
    let Some(id) = running else { return Ok(()) };
    if current_conversation(store, id)?.is_none() {
        return Ok(());
    }
    send(
        store,
        project_id,
        &Sent {
            to: handle,
            body,
            kind: "note",
            ..Sent::default()
        },
    )?;
    Ok(())
}

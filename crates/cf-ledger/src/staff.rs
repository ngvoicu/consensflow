//! A project's staff: members who join, change roles, follow the roster's
//! tiers and leave (`src/ledger/staff.js`), in three concerns of their own:
//! who may take a task (`pools`), a member out of quota (`quota`), and the
//! sessions a member's tasks run in (`sessions`).

mod pools;
mod quota;
mod sessions;

use cf_proto::ledger::{ParticipantView, RemovedMember, StaffMember, TierChange};
use rusqlite::{params, params_from_iter, OptionalExtension};
use serde_json::json;

use crate::conversations::current_conversation;
use crate::model::{self, sql_list, LedgerError, MEMBER_ROLES};
use crate::projects::NewMember;
use crate::queue::{drop_queued, send, Sent};
use crate::store::Store;
use crate::views::{participant_view, ParticipantRow, TaskRow, PARTICIPANT_SELECT};

pub(crate) use pools::{candidates, has_members_of_tier, members, nearest_tier};
pub(crate) use quota::{mark_back, mark_out};
use sessions::close_session;
pub(crate) use sessions::{
    bring_back, can_continue, continuable_session, end_session, has_task_in_hand, require_free,
    start_session,
};

/// A participant joining a project: its handle, its role or its roles (the
/// first leading), the agent and harness it runs on, and its tier.
pub(crate) struct NewParticipant<'a> {
    pub(crate) handle: &'a str,
    pub(crate) role: Option<&'a str>,
    pub(crate) roles: &'a [&'static str],
    pub(crate) agent: Option<&'a str>,
    pub(crate) harness: Option<&'a str>,
    pub(crate) designer: bool,
    pub(crate) tier: Option<&'a str>,
}

impl<'a> NewParticipant<'a> {
    /// The human, or the chief: a participant whose handle is its role.
    pub(crate) fn coordinator(
        role: &'a str,
        agent: Option<&'a str>,
        harness: Option<&'a str>,
    ) -> Self {
        Self {
            handle: role,
            role: Some(role),
            roles: &[],
            agent,
            harness,
            designer: false,
            tier: None,
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
        "INSERT INTO participant (project_id, handle, role, roles, agent, harness, designer, tier, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        params![
            project_id,
            joining.handle,
            role,
            json!(joining.roles).to_string(),
            joining.agent,
            joining.harness,
            i64::from(joining.designer),
            joining.tier,
            at,
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

/// A member joins the staff, or rejoins it in the roles, harness, designer
/// flag and tier given now.
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
                "SELECT * FROM participant WHERE project_id = ? AND handle = ? AND left_at IS NOT NULL",
                params![project_id, member.agent],
                |row| row.get::<_, i64>("id"),
            )
            .optional()?;
        let Some(id) = left else {
            return add_participant(
                store,
                project_id,
                NewParticipant {
                    handle: &member.agent,
                    role: None,
                    roles: &roles,
                    agent: Some(&member.agent),
                    harness: Some(&member.harness),
                    designer: member.designer,
                    tier: Some(&member.tier),
                },
            );
        };
        store.db.execute(
            "UPDATE participant SET role = ?, roles = ?, harness = ?, designer = ?, tier = ?,
           left_at = NULL
         WHERE id = ?",
            params![
                roles[0],
                json!(roles).to_string(),
                member.harness,
                i64::from(member.designer),
                member.tier,
                id
            ],
        )?;
        store.log(
            project_id,
            "member.added",
            json!({ "handle": member.agent, "roles": roles, "harness": member.harness, "rejoined": true }),
        )?;
        participant_view(&store.participant_row(id)?)
    })
}

/// Members follow the roster: each active member's tier (and its
/// sessions') becomes what its saved agent has now, since the app's catalog
/// may have moved the model. Says which members changed; an agent the
/// roster no longer has (`tier_of` answers none) leaves its member as it is.
pub(crate) fn refresh_member_tiers(
    store: &mut Store,
    mut tier_of: impl FnMut(&str) -> Option<String>,
) -> Result<Vec<TierChange>, LedgerError> {
    store.write(|store| {
        let members = store
            .db
            .prepare(
                "SELECT id, project_id, handle, agent, tier FROM participant
         WHERE agent IS NOT NULL AND member_id IS NULL AND left_at IS NULL AND role != 'chief'
         ORDER BY id",
            )?
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>("id")?,
                    row.get::<_, i64>("project_id")?,
                    row.get::<_, String>("handle")?,
                    row.get::<_, String>("agent")?,
                    row.get::<_, Option<String>>("tier")?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut changed = Vec::new();
        for (id, project, handle, agent, from) in members {
            let Some(to) = tier_of(&agent) else { continue };
            if from.as_deref() == Some(to.as_str()) {
                continue;
            }
            store.db.execute(
                "UPDATE participant SET tier = ? WHERE id = ? OR member_id = ?",
                params![to, id, id],
            )?;
            store.log(
                project,
                "member.tier",
                json!({ "handle": handle, "from": from, "to": to }),
            )?;
            changed.push(TierChange {
                project,
                handle,
                from,
                to,
            });
        }
        Ok(changed)
    })
}

/// A member's roles change in place. A role it gains must fit its agent;
/// one it holds already stays though it does not (a member from before an
/// image designer had to be an image agent), until the human drops it.
pub(crate) fn set_roles<S: AsRef<str>>(
    store: &mut Store,
    project_id: i64,
    handle: &str,
    roles: &[S],
) -> Result<ParticipantView, LedgerError> {
    let roles = model::require_roles(roles, || {
        model::printed(Some(&json!(roles
            .iter()
            .map(AsRef::as_ref)
            .collect::<Vec<_>>())))
    })?;
    store.write(|store| {
        let member = store.participant_by_handle(project_id, handle)?;
        require_member_row(store, member.id, "changes roles")?;
        require_staff_role(handle, &member)?;
        let held: Vec<String> = serde_json::from_str(&member.roles)?;
        let gained: Vec<&str> = roles
            .iter()
            .copied()
            .filter(|role| !held.iter().any(|kept| kept == role))
            .collect();
        model::require_fitting_roles(
            member.agent.as_deref().unwrap_or_default(),
            member.designer == 1,
            &gained,
        )?;
        store.db.execute(
            "UPDATE participant SET role = ?, roles = ? WHERE id = ?",
            params![roles[0], json!(roles).to_string(), member.id],
        )?;
        store.log(
            project_id,
            "member.roles",
            json!({ "handle": handle, "roles": roles }),
        )?;
        participant_view(&store.participant_row(member.id)?)
    })
}

/// A member leaves the staff. Its open tasks are cancelled, with the
/// messages still on their way to it and its unread questions; whoever
/// asked for those tasks is told, if its window runs.
pub(crate) fn remove_member(
    store: &mut Store,
    project_id: i64,
    handle: &str,
) -> Result<RemovedMember, LedgerError> {
    store.write(|store| {
        let member = store.participant_by_handle(project_id, handle)?;
        require_member_row(store, member.id, "leaves the staff")?;
        require_staff_role(handle, &member)?;
        let sessions = store
            .db
            .prepare(&format!(
                "{PARTICIPANT_SELECT} WHERE p.member_id = ? AND p.left_at IS NULL ORDER BY p.id"
            ))?
            .query_map([member.id], ParticipantRow::read)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let windows: Vec<i64> = std::iter::once(member.id)
            .chain(sessions.iter().map(|session| session.id))
            .collect();
        let open = store
            .db
            .prepare(&format!(
                "SELECT t.*, q.handle AS requester FROM task t JOIN participant q ON q.id = t.requester_id
         WHERE t.assignee_id IN ({})
           AND t.state IN ('queued', 'working', 'waiting')
         ORDER BY t.number",
                vec!["?"; windows.len()].join(", ")
            ))?
            .query_map(params_from_iter(&windows), |row| {
                Ok((TaskRow::read(row)?, row.get::<_, String>("requester")?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let reason = format!("@{handle} left the staff");
        for (task, _) in &open {
            drop_queued(store, task.id)?;
            store.move_task(task, "cancelled", json!({ "reason": reason }))?;
        }
        for id in &windows {
            store.db.execute(
                "UPDATE message SET state = 'cancelled'
           WHERE (recipient_id = ? AND state IN ('queued', 'delivering', 'gated'))
              OR (sender_id = ? AND kind = 'question' AND state IN ('queued', 'gated'))",
                params![id, id],
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
        Ok(RemovedMember {
            member: participant_view(&store.participant_row(member.id)?)?,
            cancelled,
        })
    })
}

/// The members of the newest project that has any: the staff a new project starts from.
pub(crate) fn last_staff(store: &Store) -> Result<Vec<StaffMember>, LedgerError> {
    let member = format!(
        "role IN ({})
    AND left_at IS NULL AND member_id IS NULL",
        sql_list(&MEMBER_ROLES)
    );
    let rows = store
        .db
        .prepare(&format!(
            "SELECT agent, harness, role, roles FROM participant
       WHERE project_id = (SELECT MAX(project_id) FROM participant WHERE {member})
         AND {member}
       ORDER BY id"
        ))?
        .query_map([], |row| {
            Ok((
                row.get::<_, Option<String>>("agent")?,
                row.get::<_, Option<String>>("harness")?,
                row.get::<_, String>("role")?,
                row.get::<_, String>("roles")?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    rows.into_iter()
        .map(|(agent, harness, role, roles)| {
            Ok(StaffMember {
                agent,
                harness,
                role,
                roles: serde_json::from_str(&roles)?,
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
    if MEMBER_ROLES.contains(&member.role.as_str()) {
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

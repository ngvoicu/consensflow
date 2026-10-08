//! A task given: by a coordinator to a participant by name, as a follow-up
//! for the session that did another, or opened for a pool and tier of the
//! staff and assigned by the daemon to a new session of a member; and a task
//! taken back to the board for another member of its tier.

use cf_proto::ledger::{TaskCreated, TaskMoved, TaskReleased};
use rusqlite::params;
use serde_json::{json, Map, Value};

use super::{
    a_pool, delivery_body, m_list, pool_name, release_ready, require_task_state, task_by_id,
    task_row_by_id,
};
use crate::messages::{leave_pause_notes, transfer};
use crate::model::{
    self, title_of, LedgerError, ACTIVE_TASK_STATES, COORDINATOR_ROLES, MAX_BODY, POOLS, PURPOSES,
};
use crate::queue::{queue, send, Queued, Sent};
use crate::staff::{
    continuable_session, has_members_of_tier, nearest_tier, require_free, require_member_row,
    start_session, Giving,
};
use crate::store::Store;
use crate::views::TaskRow;

/// A task from the chief or the human: to a participant by name (`to`), a
/// follow-up for the session that did task `after`, or, given neither, for
/// a `pool` and `tier` of the staff, critical work naming its `purpose`;
/// what it says; the tasks it `needs`, and those on the board that come
/// `before` it is done (they wait for it).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct NewTask {
    pub from: String,
    pub to: Option<String>,
    pub after: Option<i64>,
    pub pool: Option<String>,
    pub tier: Option<String>,
    pub purpose: Option<String>,
    pub body: String,
    pub needs: Vec<u64>,
    pub before: Vec<u64>,
}

impl NewTask {
    /// The task as JSON gives it, read in the order the Node ledger checked
    /// it; who gives it, a participant by name and the task followed up are
    /// the caller's, given as text and a number.
    pub fn from_json(value: &Value) -> Result<Self, LedgerError> {
        let body = model::parse_text(value.get("body"), "body", MAX_BODY)?;
        let numbers = |field: &str| match value.get(field) {
            None => Ok(Vec::new()),
            given => model::parse_numbers(given, field),
        };
        let needs = numbers("needs")?;
        let before = numbers("before")?;
        let text = |field: &str| value.get(field).and_then(Value::as_str).map(str::to_string);
        let to = text("to");
        let after = value.get("after").and_then(Value::as_i64);
        let mut tier = text("tier");
        if to.is_none() && after.is_none() {
            let pool = value.get("pool");
            if !pool
                .and_then(Value::as_str)
                .is_some_and(|pool| POOLS.contains(&pool))
            {
                return Err(invalid_pool(&cf_base::js::text(pool)));
            }
            if pool.and_then(Value::as_str) != Some("designer") {
                tier = Some(model::parse_tier(value.get("tier"))?.to_string());
            }
        }
        let task = Self {
            from: text("from").unwrap_or_default(),
            to,
            after,
            pool: text("pool"),
            tier,
            purpose: text("purpose"),
            body,
            needs,
            before,
        };
        task.check_work()?;
        Ok(task)
    }

    /// The pool, tier and purpose of a task given by neither name nor
    /// follow-up, checked as the Node ledger did: its pool and, for any but
    /// the designer, its tier; the tier it is for (none for the designer).
    fn check_work(&self) -> Result<Option<String>, LedgerError> {
        if self.to.is_some() || self.after.is_some() {
            return Ok(self.tier.clone());
        }
        let pool = self.pool.as_deref().unwrap_or("undefined");
        if !POOLS.contains(&pool) {
            return Err(invalid_pool(pool));
        }
        if pool == "designer" {
            return Ok(None);
        }
        let tier = model::parse_tier(self.tier.as_deref().map(Value::from).as_ref())?;
        if tier == "critical"
            && !self
                .purpose
                .as_deref()
                .is_some_and(|purpose| PURPOSES.contains(&purpose))
        {
            return Err(LedgerError::refused(
                "purpose-required",
                format!("critical work names its purpose: {}", PURPOSES.join(", ")),
            ));
        }
        Ok(Some(tier.to_string()))
    }
}

fn invalid_pool(pool: &str) -> LedgerError {
    LedgerError::refused(
        "invalid-pool",
        format!("a pool is {}, not {pool}", POOLS.join(", ")),
    )
}

/// Task numbers a task needs or comes before: positive, each once, in the order first given.
fn distinct(numbers: &[u64], field: &str) -> Result<Vec<i64>, LedgerError> {
    let mut distinct = Vec::new();
    for number in numbers {
        let number = i64::try_from(*number)
            .ok()
            .filter(|number| *number > 0)
            .ok_or_else(|| {
                LedgerError::refused(
                    "invalid-needs",
                    format!("{field} is a list of task numbers (T-3, T-4)"),
                )
            })?;
        if !distinct.contains(&number) {
            distinct.push(number);
        }
    }
    Ok(distinct)
}

/// A task, from the chief or the human. Given `to`, it is queued for that
/// participant at once (the chief, the human, or the requester itself).
/// Given a pool and tier instead, it opens for the daemon to assign to a
/// member of that pool and tier. A task on the board may need other tasks:
/// it waits until each is accepted. With `before`, tasks still on the board
/// wait for this one.
pub(crate) fn create_task(
    store: &mut Store,
    project_id: i64,
    request: &NewTask,
) -> Result<TaskCreated, LedgerError> {
    model::require_text(&request.body, "body", MAX_BODY)?;
    let needed = distinct(&request.needs, "needs")?;
    let blocking = distinct(&request.before, "before")?;
    let asked = request.check_work()?;
    let named = request.to.is_some() || request.after.is_some();
    let pool = request.pool.as_deref().unwrap_or_default();
    store.write(|store| {
        let requester = store.participant_by_handle(project_id, &request.from)?;
        if !COORDINATOR_ROLES.contains(&requester.role.as_str()) {
            return Err(LedgerError::refused_with(
                "not-a-coordinator",
                format!(
                    "@{} is a {}: members do not hand out tasks",
                    requester.handle, requester.role
                ),
                403,
            ));
        }
        // Advice is the chief's alone to ask: the human gives the chief work, not its advisors.
        if pool == "advisor" && !named && requester.role != "chief" {
            return Err(LedgerError::refused_with(
                "advice-for-the-chief",
                "only the chief asks an advisor",
                403,
            ));
        }
        // A follow-up on a finished task goes to the session that did it, while
        // it is still there and free: the one case a coordinator names a window.
        // A task named for a session is held to the same rule, whether it goes
        // to the window now or waits on the board for what it needs.
        let assignee = match (request.after, &request.to) {
            (Some(after), _) => Some(continuable_session(store, project_id, after)?),
            (None, Some(to)) => {
                let named = store.participant_by_handle(project_id, to)?;
                require_free(store, &named, Giving::Task)?;
                Some(named)
            }
            (None, None) => None,
        };
        // A tier nobody on the staff holds goes to the nearest one somebody does,
        // the next one up first: light work with only critical members still goes.
        let tier = match assignee {
            None => nearest_tier(store, project_id, pool, asked.as_deref())?,
            Some(_) => asked.clone(),
        };
        if assignee.is_none() && !has_members_of_tier(store, project_id, pool, tier.as_deref())? {
            return Err(LedgerError::refused_with(
                "no-member-of-tier",
                format!(
                    "no {} is on the staff: ask the human for one, in your terminal",
                    if pool == "designer" { "image designer" } else { pool }
                ),
                409,
            ));
        }
        // A task given by name waits on the board too while what it needs is
        // not yet accepted; it goes to its window then.
        let mut blocked = false;
        if assignee.is_some() {
            for number in &needed {
                if store.task_row(project_id, *number)?.state != "accepted" {
                    blocked = true;
                    break;
                }
            }
        }
        let next: i64 = store.db.query_row(
            "SELECT COALESCE(MAX(number), 0) + 1 AS next FROM task WHERE project_id = ?",
            [project_id],
            |row| row.get("next"),
        )?;
        let at = store.at();
        let open = assignee.is_none() || blocked;
        store.db.execute(
            "INSERT INTO task (project_id, number, title, body, requester_id, assignee_id, state,
                           pool, tier, purpose, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                project_id,
                next,
                title_of(&request.body),
                request.body,
                requester.id,
                assignee.as_ref().map(|assignee| assignee.id),
                if open { "open" } else { "queued" },
                assignee.is_none().then_some(pool),
                if assignee.is_none() { tier.as_deref() } else { None },
                request.purpose,
                at,
                at,
            ],
        )?;
        let task_id = store.db.last_insert_rowid();
        for number in &needed {
            let need = store.task_row(project_id, *number)?;
            super::require_on_board(&need, "wait for")?;
            if need.state == "cancelled" {
                return Err(LedgerError::refused_with(
                    "need-cancelled",
                    format!("T-{number} is cancelled: nothing waits for it"),
                    409,
                ));
            }
            store.db.execute(
                "INSERT INTO task_need (task_id, needs_id) VALUES (?, ?)",
                params![task_id, need.id],
            )?;
        }
        for number in &blocking {
            let waits = store.task_row(project_id, *number)?;
            if waits.state != "open" {
                return Err(LedgerError::refused_with(
                    "not-on-the-board",
                    format!(
                        "T-{number} is {}: only a task still on the board can wait for a new one",
                        waits.state
                    ),
                    409,
                ));
            }
            // A plan has no circles: what the new task waits for, near or far, cannot wait for it.
            if upstream(store, task_id, waits.id)? {
                return Err(LedgerError::refused_with(
                    "circular-needs",
                    format!("T-{number} is already what T-{next} waits for: a plan has no circles"),
                    409,
                ));
            }
            store.db.execute(
                "INSERT INTO task_need (task_id, needs_id) VALUES (?, ?)",
                params![waits.id, task_id],
            )?;
        }
        // The tier asked, when the task went to the nearest one somebody holds.
        let moved = asked.filter(|asked| tier.as_deref() != Some(asked));
        let assignee = match assignee {
            Some(assignee) if !blocked => assignee,
            assignee => {
            let mut data = Map::new();
            data.insert("task".into(), json!(next));
            data.insert("from".into(), json!(requester.handle));
            match &assignee {
                None => {
                    data.insert("pool".into(), json!(pool));
                    data.insert("tier".into(), json!(tier));
                }
                Some(assignee) => {
                    data.insert("to".into(), json!(assignee.handle));
                }
            }
            if !needed.is_empty() {
                data.insert("needs".into(), json!(needed));
            }
            if !blocking.is_empty() {
                data.insert("before".into(), json!(blocking));
            }
            store.log(project_id, "task.opened", Value::Object(data))?;
            return Ok(TaskCreated {
                task: task_by_id(store, task_id)?,
                message: None,
                asked: moved,
            });
            }
        };
        let body = delivery_body(&task_row_by_id(store, task_id)?);
        let message_id = queue(
            store,
            project_id,
            &Queued {
                to: assignee.id,
                from: Some(requester.id),
                kind: "task",
                task_id: Some(task_id),
                body: &body,
                ..Queued::default()
            },
        )?;
        store.log(
            project_id,
            "task.created",
            json!({ "task": next, "from": requester.handle, "to": assignee.handle, "message": message_id }),
        )?;
        Ok(TaskCreated {
            task: task_by_id(store, task_id)?,
            message: store.message(message_id)?,
            asked: moved,
        })
    })
}

/// Whether `task_id` needs `other_id`, directly or through the tasks it needs.
fn upstream(store: &Store, task_id: i64, other_id: i64) -> Result<bool, LedgerError> {
    Ok(store
        .db
        .prepare(
            "WITH RECURSIVE upstream (id) AS (
           SELECT needs_id FROM task_need WHERE task_id = ?
           UNION
           SELECT n.needs_id FROM task_need n JOIN upstream u ON n.task_id = u.id
         )
         SELECT 1 FROM upstream WHERE id = ? LIMIT 1",
        )?
        .exists(params![task_id, other_id])?)
}

/// The daemon's choice for an open task: a new session of that member,
/// which the task is queued for from here on. What its requester was told of
/// the task's wait (that it was taken back, that it waits for a free member)
/// and has not been given yet is withdrawn: it would arrive after the wait
/// was over.
pub(crate) fn assign_task(
    store: &mut Store,
    project_id: i64,
    number: i64,
    participant_id: i64,
) -> Result<TaskMoved, LedgerError> {
    store.write(|store| {
        let task = store.task_row(project_id, number)?;
        require_task_state(&task, &["open"], "assign")?;
        let member = require_member_row(store, participant_id, "takes a task")?;
        let roles: Vec<String> = serde_json::from_str(&member.roles)?;
        let candidate = member.left_at.is_none()
            && member.project_id == project_id
            && task.pool.as_ref().is_some_and(|pool| roles.contains(pool))
            && (task.tier.is_none() || member.tier == task.tier);
        if !candidate {
            return Err(LedgerError::refused_with(
                "not-a-candidate",
                format!(
                    "@{} is not {} on this staff",
                    member.handle,
                    a_pool(task.pool.as_deref(), task.tier.as_deref())
                ),
                409,
            ));
        }
        let pool = task.pool.as_deref().unwrap_or_default();
        let session = start_session(store, project_id, &member, pool)?;
        leave_pause_notes(store, &task, &format!("was taken by @{}", member.handle))?;
        let at = store.at();
        store.db.execute(
            "UPDATE task SET assignee_id = ?, updated_at = ? WHERE id = ?",
            params![session.id, at, task.id],
        )?;
        let body = delivery_body(&task_row_by_id(store, task.id)?);
        let message_id = queue(
            store,
            project_id,
            &Queued {
                to: session.id,
                from: Some(task.requester_id),
                kind: "task",
                task_id: Some(task.id),
                body: &body,
                ..Queued::default()
            },
        )?;
        store.move_task_as(
            &task,
            "queued",
            json!({ "assignee": session.handle, "member": member.handle, "message": message_id }),
            "task.assigned",
        )?;
        Ok(TaskMoved {
            task: task_by_id(store, task.id)?,
            message: store.message(message_id)?,
        })
    })
}

/// A task given by tier goes back to the board for another member of that
/// tier: taken from a member that ran out of quota (the daemon), or
/// reassigned by the human, working or paused. The task opens again with a
/// warning for the next member, and with the words that were on their way
/// to the old one, an approved answer being delivered included, said once in
/// its brief (`transfer`); what was still held at the gate for the human is
/// withdrawn, and the requester is told so. What the requester was told of
/// the wait the task leaves (a stall, a hold) and has not been given yet is
/// withdrawn first: the note that follows says where the task is now.
pub(crate) fn release_task(
    store: &mut Store,
    project_id: i64,
    number: i64,
    because: &str,
) -> Result<TaskReleased, LedgerError> {
    model::require_text(because, "because", 1000)?;
    store.write(|store| {
        let task = store.task_row(project_id, number)?;
        require_releasable(&task)?;
        leave_pause_notes(store, &task, "was taken back")?;
        // A task paused before anyone took it has nobody to take it from.
        let member = task
            .assignee_id
            .map(|id| store.participant_row(id))
            .transpose()?;
        let carried = match &member {
            Some(member) => Some(transfer(store, &task, member, &delivery_body(&task))?),
            None => None,
        };
        // One statement: the row is never without an assignee in a working
        // state. It remembers the member it was taken from (a session's member).
        let body = match (&member, &carried) {
            (Some(member), Some(carried)) => format!(
                "{}{}\n\nReassigned from @{} ({because}); check the working tree for partial changes.",
                task.body, carried.kept, member.handle
            ),
            _ => task.body.clone(),
        };
        let at = store.at();
        store.db.execute(
            "UPDATE task SET assignee_id = NULL, state = 'open', body = ?, updated_at = ?,
           taken_from_id = COALESCE(?, taken_from_id)
         WHERE id = ?",
            params![
                body,
                at,
                member.as_ref().map(|member| member.member_id.unwrap_or(member.id)),
                task.id
            ],
        )?;
        store.log(
            project_id,
            "task.released",
            json!({
                "task": number,
                "from": task.state,
                "to": "open",
                "member": member.as_ref().map(|member| &member.handle),
                "because": because,
            }),
        )?;
        let requester = store.participant_row(task.requester_id)?;
        let pool = task.pool.as_deref();
        let tier = task.tier.as_deref();
        let note = match (&member, &carried) {
            (Some(member), Some(carried)) => {
                let withdrawn = if carried.gated.is_empty() {
                    String::new()
                } else {
                    format!(
                        " Withdrawn with it, still waiting for the human: {}.",
                        m_list(&carried.gated)
                    )
                };
                format!(
                    "T-{number} was taken back from @{} ({because}) and waits for another {}.{withdrawn}",
                    member.handle,
                    pool_name(pool, tier)
                )
            }
            _ => format!(
                "T-{number} is back on the board ({because}) and waits for {}.",
                a_pool(pool, tier)
            ),
        };
        send(
            store,
            project_id,
            &Sent {
                to: &requester.handle,
                task: Some(number),
                kind: "note",
                body: &note,
                ..Sent::default()
            },
        )?;
        release_ready(store, project_id)?;
        Ok(TaskReleased {
            task: task_by_id(store, task.id)?,
        })
    })
}

/// Whether task T-`number` may go back to the board for its tier, without moving it; says why not.
pub(crate) fn check_release(
    store: &Store,
    project_id: i64,
    number: i64,
) -> Result<(), LedgerError> {
    require_releasable(&store.task_row(project_id, number)?)
}

/// A task in a window, or waiting for one, given by its tier: only such a task goes back to the board.
fn require_releasable(task: &TaskRow) -> Result<(), LedgerError> {
    let releasable = [&["queued"], &ACTIVE_TASK_STATES[..], &["paused"]].concat();
    require_task_state(task, &releasable, "release")?;
    if task.pool.is_none() {
        return Err(LedgerError::refused_with(
            "invalid-transition",
            format!(
                "cannot release T-{}: it was given by name, not by tier",
                task.number
            ),
            409,
        ));
    }
    Ok(())
}

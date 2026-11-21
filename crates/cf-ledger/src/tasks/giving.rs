//! A task given: by a coordinator to a participant by name, as a follow-up
//! for the session that did another, or opened for a pool and tier of the
//! staff and assigned by the daemon to a new session of a member; and a task
//! taken back to the board for another member of its tier.

use cf_proto::ledger::{TaskCreated, TaskMoved, TaskPlaced, TaskReleased, TaskRetiered};
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Map, Value};

use super::{delivery_body, m_list, release_ready, require_task_state, task_by_id, task_row_by_id};
use crate::messages::{leave_pause_notes, transfer, Afterwards};
use crate::model::{
    self, title_of, LedgerError, ACTIVE_TASK_STATES, COORDINATOR_ROLES, MAX_BODY, PURPOSES,
};
use crate::queue::{queue, send, Queued, Sent};
use crate::roles::{self, a_pool, pool_name, Role};
use crate::staff::{
    follow_up_session, holders_of_tier, may_take_slot, place, require_free, Placed,
    require_member_row, start_session, Giving,
};
use crate::store::Store;
use crate::views::{ParticipantRow, TaskRow};

/// Fresh work cannot be named for a member on Vacation, including by naming
/// one of its sessions. Follow-ups and reopened work use their own paths.
fn require_available_for_new_task(
    store: &Store,
    taker: &ParticipantRow,
) -> Result<(), LedgerError> {
    let owner = taker
        .member_id
        .map(|id| store.participant_row(id))
        .transpose()?;
    let member = owner.as_ref().unwrap_or(taker);
    if member.vacation_since.is_some() {
        return Err(LedgerError::refused_with(
            "member-on-vacation",
            format!("@{} is on Vacation", member.handle),
            409,
        ));
    }
    Ok(())
}

/// A task from the chief or the human: to a participant by name (`to`), a
/// follow-up for the session that did task `after`, or, given neither, for
/// a `pool` and `tier` of the staff, critical work naming its `purpose`;
/// what it says; the tasks it `needs`, and those on the board that come
/// `before` it is done (they wait for it).
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize)]
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
            if !pool.and_then(Value::as_str).is_some_and(roles::is_role) {
                return Err(invalid_pool(&cf_base::js::text(pool)));
            }
            // A tier that is said is read here as it always was. One that is left out is the
            // ledger's to ask for where the task is given (`check_work`), after a plan's key is
            // looked up: a plan an earlier version took under a key (a section of a role that
            // had no tier) still replays its saved answer.
            if value.get("tier").is_some()
                && !pool.and_then(Value::as_str).is_some_and(roles::untiered)
            {
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
        // A tier left out is asked for where the task is given (`create_task`), after a plan's
        // key is looked up.
        if task.tier.is_some() || task.to.is_some() || task.after.is_some() {
            task.check_work()?;
        }
        Ok(task)
    }

    /// The pool, tier and purpose of a task given by neither name nor
    /// follow-up, checked as the Node ledger did: its pool and, for a role with
    /// tiers, its tier; the tier it is for (none for a role with none).
    fn check_work(&self) -> Result<Option<String>, LedgerError> {
        if self.to.is_some() || self.after.is_some() {
            return Ok(self.tier.clone());
        }
        let pool = self.pool.as_deref().unwrap_or("undefined");
        if !roles::is_role(pool) {
            return Err(invalid_pool(pool));
        }
        if roles::untiered(pool) {
            return Ok(None);
        }
        let tier = model::parse_tier(self.tier.as_deref().map(Value::from).as_ref())?;
        if !roles::offers(pool, tier) {
            return Err(crate::staff::slots::not_offered_for(pool, tier));
        }
        if tier != "critical" && self.purpose.is_some() {
            return Err(LedgerError::refused(
                "purpose-without-critical",
                "a purpose comes with --tier critical",
            ));
        }
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

/// The one role a request to give a task asks for (`POST /api/tasks`, whose fields are the
/// switches of `cf task add`), or none (the worker's task, or a follow-up's or the chief's own
/// that asks for none), checked before anything is looked for. What it asks beside it is
/// refused in words that name what was asked, never dropped for something else (T-1453): two
/// roles; `--self` with a role; a tier or a purpose for a role with no tier. A tier or a
/// purpose with `--after` or `--self`, and `--self` with `--after`, are taken as on main, the
/// extra ignored. A role asked for a follow-up is held to the window's in [`create_task`].
pub fn asked_role(request: &Map<String, Value>) -> Result<Option<&'static Role>, LedgerError> {
    let on = |field: &str| request.get(field) == Some(&Value::Bool(true));
    let given = |field: &str| request.get(field).is_some_and(|value| !value.is_null());
    let refused =
        |code: &'static str, words: String| Err(LedgerError::refused_with(code, words, 400));
    let asked = roles::asked(on);
    if let [_, _, ..] = asked.as_slice() {
        let names: Vec<String> = asked.iter().map(|role| role.a_name()).collect();
        return refused(
            "two-roles",
            format!(
                "a task is for one role, and this one asks for {}: add a task for each",
                said_together(&names)
            ),
        );
    }
    let role = asked.first().copied();
    if on("self") {
        if let Some(switch) = role.and_then(|role| role.switch) {
            return refused(
                "self-takes-no-role",
                format!("a --self task is your own step: leave out {switch}"),
            );
        }
    }
    if let Some(role) = role.filter(|role| !role.tiered()) {
        let switches: Vec<&str> = [("tier", "--tier"), ("purpose", "--purpose")]
            .into_iter()
            .filter(|(field, _)| given(field))
            .map(|(_, switch)| switch)
            .collect();
        if !switches.is_empty() {
            let (extra, leave) = if switches.len() == 2 {
                ("", switches.join(" and "))
            } else if switches[0] == "--purpose" {
                (", and so no purpose", switches[0].to_owned())
            } else {
                ("", switches[0].to_owned())
            };
            return refused(
                "no-tier-for-role",
                format!(
                    "{} task has no tier{extra}: leave out {leave}",
                    role.a_names()
                ),
            );
        }
    }
    Ok(role)
}

/// `a`, `a and b`, `a, b and c`.
fn said_together(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [one] => one.clone(),
        [first @ .., last] => format!("{} and {last}", first.join(", ")),
    }
}

/// A follow-up that asks for role `asked` goes to the window of T-`after` only where that
/// window began for `asked` and its member still holds it (T-1453): a window's role is the one
/// its session began for (`participant.role` of the session, which nothing changes after), and
/// every follow-up it takes is the next step of that work. A T-`after` with no window, or one of
/// no session, is left to [`follow_up_session`], which refuses it in its words.
fn require_window_of(
    store: &Store,
    project_id: i64,
    after: i64,
    asked: &Role,
) -> Result<(), LedgerError> {
    let previous = store.task_row(project_id, after)?;
    let Some(window) = previous
        .assignee_id
        .map(|id| store.participant_row(id))
        .transpose()?
    else {
        return Ok(());
    };
    let Some(member_id) = window.member_id else {
        return Ok(());
    };
    let whose =
        roles::find(&window.role).map_or_else(|| format!("a {}'s", window.role), Role::a_names);
    let refused = |words: String| {
        Err(LedgerError::refused_with(
            "follow-up-of-another-role",
            words,
            409,
        ))
    };
    if window.role != asked.id {
        return refused(format!(
            "T-{after} is {whose} task; {} task starts in a new window: leave out --after",
            asked.a_names()
        ));
    }
    let member = store.participant_row(member_id)?;
    let held: Vec<String> = serde_json::from_str(&member.roles)?;
    if !held.iter().any(|role| role == asked.id) {
        return refused(format!(
            "T-{after} is {whose} task, and @{} is no {} now: {} task starts in a new window: leave out --after",
            member.handle,
            asked.name,
            asked.a_names()
        ));
    }
    Ok(())
}

fn invalid_pool(pool: &str) -> LedgerError {
    LedgerError::refused(
        "invalid-pool",
        format!("a pool is {}, not {pool}", roles::ids().join(", ")),
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
/// participant at once (the chief, the human, or the requester itself). Given
/// `after`, it is queued for the session that did that task at once when nothing
/// that window did waits for the chief's decision, and otherwise waits on the board
/// for those tasks to be accepted: T-`after` while it is at work or its result is
/// in, and every other task of the session whose result is in and not decided.
/// Given a pool and tier instead, it opens for the daemon to assign to a
/// member of that pool and tier. A task on the board may need other tasks:
/// it waits until each is accepted, one the human cleared from the board
/// included when it was accepted. With `before`, tasks still on the board
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
        if pool == roles::ADVISOR.id && !named && requester.role != "chief" {
            return Err(LedgerError::refused_with(
                "advice-for-the-chief",
                "only the chief asks an advisor",
                403,
            ));
        }
        // A follow-up goes to the session that did the task followed, while it
        // is still there: the one case a coordinator names a window. The session
        // must be free, and hold no result the chief has not decided; where the
        // task followed is still on its work or has its result in, or another task
        // of the session has, the follow-up waits on the board behind them, which
        // are among the tasks it needs. A task
        // named for a session is held to the free rule, whether it goes to the
        // window now or waits on the board for what it needs.
        let mut behind = Vec::new();
        let assignee = match (request.after, &request.to) {
            (Some(after), _) => {
                // A role asked for it is the window's own, or the follow-up starts nowhere.
                if let Some(asked) = request.pool.as_deref().and_then(roles::find) {
                    require_window_of(store, project_id, after, asked)?;
                }
                let follow_up = follow_up_session(store, project_id, after)?;
                behind = follow_up.behind;
                Some(follow_up.session)
            }
            (None, Some(to)) => {
                let named = store.participant_by_handle(project_id, to)?;
                require_available_for_new_task(store, &named)?;
                require_free(store, &named, Giving::Task)?;
                Some(named)
            }
            (None, None) => None,
        };
        let needed = [needed.as_slice(), &behind]
            .concat()
            .into_iter()
            .fold(Vec::new(), |mut all, number| {
                if !all.contains(&number) {
                    all.push(number);
                }
                all
            });
        // Where the task goes: its slot, else the nearest held one outside Critical (the
        // chief is told whose); else it waits, and the dispatcher tells the way on.
        let tier = match (assignee.as_ref(), asked.as_deref()) {
            (None, Some(asked)) => Some(match place(store, project_id, pool, Some(asked), request.purpose.as_deref())? {
                Placed::Fell(to) => to,
                Placed::Stays(_) | Placed::Waits { .. } => asked.to_owned(),
            }),
            _ => asked.clone(),
        };
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
            // Never accepted, so it never releases what waits for it, deleted or not.
            if need.state == "cancelled" {
                return Err(LedgerError::refused_with(
                    "need-cancelled",
                    format!("T-{number} is cancelled: nothing waits for it"),
                    409,
                ));
            }
            // The human's delete only takes a task off the board: it keeps its state, and a
            // waiter reads it as before (`blocked_by`, `release_ready`), so an accepted task the
            // human cleared still releases a new one at once. Any other deleted task would leave
            // its waiter blocked for good: a failed one is not reopened any more.
            if need.state != "accepted" {
                super::require_on_board(&need, "wait for")?;
            }
            store.db.execute(
                "INSERT INTO task_need (task_id, needs_id) VALUES (?, ?)",
                params![task_id, need.id],
            )?;
        }
        for number in &blocking {
            let waits = store.task_row(project_id, *number)?;
            if waits.state != "open" {
                return Err(cannot_wait_for_a_new_one(store, &waits)?);
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

/// The refusal of a `--before` that names a task which is not on the board: only
/// an open one can wait for a new task. A task in a window is not pulled back, by
/// design, so the refusal says what to do instead (T-675, the field report of
/// 2026-10-27, where a chief cancelled a running review and added it again with
/// its whole brief restated; and the review T-1212, finding 4: advice a task cannot
/// follow is no advice). Add the new task alone, then, by who the task is given to and
/// whether its brief can still be edited, not by its state alone:
/// - a task for a tier, queued for its window with its brief not begun, is put behind the
///   new one with `cf task edit T-n --needs +T-new` (T-1108); one given by name, a follow-up
///   or the chief's own step, has no pool to wait on the board in, and is not told to;
/// - a worker's task in its window (for a tier or by name, a follow-up included) is stopped
///   with a pause and sent on when the new task is accepted, or, paused already, sent on;
/// - the chief's own step is not paused (`own-work`): it is finished later, with `cf task
///   done`, when the new task is accepted;
/// - any of them is cancelled and added again with `--needs` (`--self --needs` for the
///   chief's own step).
///
/// The new task is not on the board, since the refusal rolls it back, so it is not named by
/// number.
fn cannot_wait_for_a_new_one(store: &Store, waits: &TaskRow) -> Result<LedgerError, LedgerError> {
    let number = waits.number;
    let alone = "a task in a window is not pulled back";
    let again = "cancel it and add it again with --needs on the new task";
    let instead = if !matches!(
        waits.state.as_str(),
        "queued" | "working" | "waiting" | "paused"
    ) {
        "add that work again with --needs on the new task if it is still wanted".to_owned()
    } else if super::own_steps::is_own_step(store, waits)? {
        format!(
            "{alone}. T-{number} is your own step, which is not paused: finish it later with cf task done T-{number} \"…\" once the new task is accepted, or cancel it and add it again with --self --needs on the new task"
        )
    } else if waits.state == "paused" {
        format!(
            "{alone}. Add the new task without --before, and send T-{number} on with cf task resume T-{number} \"…\" once the new task is accepted; or {again}"
        )
    } else if waits.state == "queued" && waits.pool.is_some() && !brief_begun(store, waits)? {
        format!(
            "{alone}. Add the new task without --before, then put T-{number} behind it with cf task edit T-{number} --needs +T-<the new task>; or {again}"
        )
    } else {
        format!(
            "{alone}. Add the new task without --before, pause T-{number} now (cf task pause T-{number}), and send it on with cf task resume T-{number} \"…\" once the new task is accepted; or {again}"
        )
    };
    Ok(LedgerError::refused_with(
        "not-on-the-board",
        format!(
            "T-{number} is {}: only a task still on the board can wait for a new one; {instead}",
            waits.state
        ),
        409,
    ))
}

/// Whether a task message of `task` has begun to reach its window (being pasted, pasted or
/// read): what `cf task edit` refuses a task for, telling its window instead.
fn brief_begun(store: &Store, task: &TaskRow) -> Result<bool, LedgerError> {
    Ok(store
        .db
        .prepare(
            "SELECT 1 FROM message WHERE task_id = ? AND kind = 'task'
               AND state IN ('delivering', 'delivered', 'read')",
        )?
        .exists([task.id])?)
}

/// Whether `task_id` needs `other_id`, directly or through the tasks it needs.
pub(super) fn upstream(store: &Store, task_id: i64, other_id: i64) -> Result<bool, LedgerError> {
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

/// Whether `participant_id` may take task `number` in a new window: what `assign_task` asks
/// of its member, but not that the task is open. A member of the task's role (at the task's
/// tier where the task has one) that is not on Vacation; and a member at Critical takes only a
/// task at Critical with its purpose, in every assignment. Critical's rule goes by the role's
/// effective slot: the image designer's has no tier, whatever tier an old ledger stored for it.
/// The sweep asks it before it releases a held task to a backup.
pub(crate) fn check_taker(
    store: &Store,
    project_id: i64,
    number: i64,
    participant_id: i64,
) -> Result<(), LedgerError> {
    let task = store.task_row(project_id, number)?;
    check_taker_row(store, project_id, &task, participant_id).map(|_| ())
}

fn check_taker_row(
    store: &Store,
    project_id: i64,
    task: &TaskRow,
    participant_id: i64,
) -> Result<ParticipantRow, LedgerError> {
    let member = require_member_row(store, participant_id, "takes a task")?;
    require_available_for_new_task(store, &member)?;
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
    let tiered = task
        .pool
        .as_deref()
        .and_then(roles::find)
        .is_some_and(roles::Role::tiered);
    if tiered
        && member.tier.as_deref() == Some("critical")
        && !(task.tier.as_deref() == Some("critical")
            && may_take_slot(
                task.pool.as_deref().unwrap_or_default(),
                "critical",
                task.purpose.as_deref(),
            ))
    {
        return Err(LedgerError::refused_with(
            "critical-takes-critical-work",
            format!(
                "@{} holds a critical slot, which takes only critical work given with its purpose",
                member.handle
            ),
            409,
        ));
    }
    Ok(member)
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
        let member = check_taker_row(store, project_id, &task, participant_id)?;
        let pool = task.pool.as_deref().unwrap_or_default();
        let session = start_session(store, project_id, &member, pool)?;
        leave_pause_notes(
            store,
            &task,
            &format!("was taken by @{}", member.handle),
            Afterwards::MovesOn,
        )?;
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
            withdrawn: Vec::new(),
            landing: Vec::new(),
        })
    })
}

/// Where an open task stands at a dispatcher pass: the placement. A task from before the
/// roles had tiers (no tier, for a role that has them now) takes its role's home tier
/// first (`task.retiered`). Then, as a task given now would be placed: it stays in its slot
/// where a member holds it and the task may take it; else it falls to the nearest held slot
/// of its role outside Critical, the next one up before the next one down (its tier becomes
/// that slot's, and `Fell` says which it was for, which it is for and who holds it); else it
/// waits, and `Waits` says who holds the Critical slot, which takes only critical work. A
/// task that is not open, or given by name, stays.
pub(crate) fn retier_open_task(
    store: &mut Store,
    project_id: i64,
    number: i64,
) -> Result<TaskPlaced, LedgerError> {
    store.write(|store| {
        let task = store.task_row(project_id, number)?;
        let (true, Some(pool)) = (task.state == "open", task.pool.as_deref()) else {
            return Ok(TaskPlaced::Stays);
        };
        if !roles::find(pool).is_some_and(roles::Role::tiered) {
            return Ok(TaskPlaced::Stays);
        }
        let home = roles::find(pool).map_or("standard", |role| role.home);
        let asked = task.tier.as_deref().unwrap_or(home).to_owned();
        let at = store.at();
        if task.tier.is_none() {
            store.db.execute(
                "UPDATE task SET tier = ?, updated_at = ? WHERE id = ?",
                params![asked, at, task.id],
            )?;
            store.log(
                project_id,
                "task.retiered",
                json!({ "task": number, "from": null, "to": asked }),
            )?;
        }
        match place(store, project_id, pool, Some(&asked), task.purpose.as_deref())? {
            Placed::Stays(_) => Ok(TaskPlaced::Stays),
            Placed::Waits { critical } => Ok(TaskPlaced::Waits { critical }),
            Placed::Fell(tier) => {
                store.db.execute(
                    "UPDATE task SET tier = ?, updated_at = ? WHERE id = ?",
                    params![tier, at, task.id],
                )?;
                store.log(
                    project_id,
                    "task.retiered",
                    json!({ "task": number, "from": asked, "to": tier }),
                )?;
                Ok(TaskPlaced::Fell(TaskRetiered {
                    from: asked,
                    holders: holders_of_tier(store, project_id, pool, &tier)?,
                    to: tier,
                }))
            }
        }
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
    release_inside(store, project_id, number, because, false)
}

/// [`release_task`], for a caller that is in a step of its own. `untouched` says
/// that no window began the task (its brief had not reached the member it is taken
/// from): the next member is not warned to check the tree for partial changes.
pub(crate) fn release_inside(
    store: &mut Store,
    project_id: i64,
    number: i64,
    because: &str,
    untouched: bool,
) -> Result<TaskReleased, LedgerError> {
    release_told(store, project_id, number, because, untouched, true)
}

/// [`release_task`] for a task the caller gives to its member's backup in the same step: its
/// requester is told once, by the note of that move (`tell_backup_took`), so the release says
/// nothing of its own unless it asks something of the requester (a follow-up left bound, words
/// withdrawn from the gate).
pub(crate) fn hand_task_on(
    store: &mut Store,
    project_id: i64,
    number: i64,
    because: &str,
) -> Result<TaskReleased, LedgerError> {
    release_told(store, project_id, number, because, false, false)
}

fn release_told(
    store: &mut Store,
    project_id: i64,
    number: i64,
    because: &str,
    untouched: bool,
    tell: bool,
) -> Result<TaskReleased, LedgerError> {
    model::require_text(because, "because", 1000)?;
    store.write(|store| {
        let task = store.task_row(project_id, number)?;
        require_releasable(&task)?;
        leave_pause_notes(store, &task, "was taken back", Afterwards::MovesOn)?;
        // A task paused before anyone took it has nobody to take it from.
        let member = task
            .assignee_id
            .map(|id| store.participant_row(id))
            .transpose()?;
        let mut place = format!("T-{} is open for reassignment.", task.number);
        if !tell {
            if let Some(window) = &member {
                let owner = store.participant_row(window.member_id.unwrap_or(window.id))?;
                let backup: Option<String> = store.db.query_row(
                    "SELECT handle FROM participant WHERE backup_of = ? AND left_at IS NULL",
                    [owner.id], |row| row.get(0),
                ).optional()?;
                if let Some(backup) = backup {
                    place.push_str(&format!(" Backup @{backup} of @{} was selected.", owner.handle));
                }
            }
        }
        crate::holding::close_task(store, &task, Some(&place))?;
        let carried = match &member {
            Some(member) => Some(transfer(store, &task, member, &delivery_body(&task))?),
            None => None,
        };
        // One statement: the row is never without an assignee in a working
        // state. It remembers the member it was taken from (a session's member),
        // and is no pause of its project's suspension now, for a Resume to resume.
        // A hold goes with the member it was taken from: the task waits on the board,
        // held for nobody's quota, and its line says no hold (T-1500's finding 4).
        let body = match (&member, &carried) {
            (Some(_), Some(carried)) if untouched => format!("{}{}", task.body, carried.kept),
            (Some(member), Some(carried)) => format!(
                "{}{}\n\nReassigned from @{} ({because}); check the working tree for partial changes.",
                task.body, carried.kept, member.handle
            ),
            _ => task.body.clone(),
        };
        let at = store.at();
        store.db.execute(
            "UPDATE task SET assignee_id = NULL, state = 'open', body = ?, updated_at = ?,
           taken_from_id = COALESCE(?, taken_from_id), paused_with_project = 0, held_until = NULL
         WHERE id = ?",
            params![
                body,
                at,
                member.as_ref().map(|member| member.member_id.unwrap_or(member.id)),
                task.id
            ],
        )?;
        if let Some(window) = task.assignee_id {
            super::pausing::shut_tracked_doors(store, &task, window)?;
        }
        crate::tasks::end_asides(store, task.id, &at)?;
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
        // The follow-ups bound to the window that lost the task stay bound to it, which
        // never finished it: they are named, with what to do.
        let bound: Vec<String> = match &member {
            Some(member) => store
                .db
                .prepare(
                    "SELECT t.number FROM task t JOIN task_need n ON n.task_id = t.id
               WHERE n.needs_id = ? AND t.state IN ('open', 'paused') AND t.assignee_id = ?
               ORDER BY t.number",
                )?
                .query_map(params![task.id, member.id], |row| {
                    Ok(format!("T-{}", row.get::<_, i64>(0)?))
                })?
                .collect::<rusqlite::Result<_>>()?,
            None => Vec::new(),
        };
        // It goes on by itself unless a follow-up stays bound or a message went with it: then
        // the chief has a decision, and it is typed; else shown (RS-25).
        let asks = !bound.is_empty() || carried.as_ref().is_some_and(|c| !c.gated.is_empty());
        let note = match (&member, bound.as_slice()) {
            (Some(member), [only]) => format!(
                "{note} {only} stays bound to @{}, which no longer has T-{number}: cancel it, or add it again for a tier with --needs T-{number}.",
                member.handle
            ),
            (Some(member), [_, _, ..]) => format!(
                "{note} {} stay bound to @{}, which no longer has T-{number}: cancel them, or add them again for a tier with --needs T-{number}.",
                bound.join(", "),
                member.handle
            ),
            _ => note,
        };
        if tell || asks {
            send(
                store,
                project_id,
                &Sent {
                    to: &requester.handle,
                    task: Some(number),
                    kind: "note",
                    body: &note,
                    shown: (!asks)
                        .then(|| note.strip_prefix(&format!("T-{number} ")))
                        .flatten(),
                    ..Sent::default()
                },
            )?;
        }
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

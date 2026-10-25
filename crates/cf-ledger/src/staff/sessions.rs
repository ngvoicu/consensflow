//! A member's sessions, each a named window from its first task until the
//! human deletes it, and after that for a follow-up, and what makes one
//! busy: the task it has in hand, or the follow-up that waits for it.

use cf_proto::ledger::ProjectView;
use rusqlite::{params, OptionalExtension};
use serde_json::json;

use crate::conversations::current_conversation;
use crate::model::{sql_list, LedgerError, HELD_TASK_STATES, MEMBER_ROLES};
use crate::projects::known_project;
use crate::store::Store;
use crate::views::{ParticipantRow, TaskRow};

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

/// The session that did T-`after`, and whether a follow-up for it goes behind
/// that task.
pub(crate) struct FollowUp {
    pub session: ParticipantRow,
    /// The window is still on T-`after` (or will be: it waits for its needs, or
    /// is paused): the follow-up waits for it to be accepted, as it would for
    /// any task it needs, and goes into the window then.
    pub behind: bool,
}

/// The session that did T-`after`, on the board (brought back if the human had
/// deleted it) for a follow-up. A follow-up of a task whose result is not in
/// goes behind it (`FollowUp::behind`) and asks nothing of the session. One of
/// a task whose result is in goes to the session now, and is refused while the
/// session has another task in hand (`require_free`); unless follow-ups already
/// wait for that task's acceptance, which the new one lines up behind (by
/// number), but for those it is put `before`.
pub(crate) fn follow_up_session(
    store: &mut Store,
    project_id: i64,
    after: i64,
    before: &[i64],
) -> Result<FollowUp, LedgerError> {
    let previous = store.task_row(project_id, after)?;
    if previous.assignee_id.is_none() {
        return Err(LedgerError::refused_with(
            "no-window-yet",
            format!(
                "T-{after} has no window yet: nobody has taken it. Add the next step for a tier with --needs T-{after}, and it waits until T-{after} is accepted"
            ),
            409,
        ));
    }
    let assignee = previous
        .assignee_id
        .map(|id| store.participant_row(id))
        .transpose()?;
    // A task given to a member by name runs in its lane, which is no window to
    // continue; its follow-up waits for it as a task for a tier.
    if let Some(lane) = assignee
        .as_ref()
        .filter(|lane| lane.member_id.is_none() && lane.left_at.is_none())
        .filter(|lane| MEMBER_ROLES.contains(&lane.role.as_str()))
    {
        return Err(LedgerError::refused_with(
            "no-window-yet",
            format!(
                "T-{after} was given to @{} by name, which has no window of its own to continue. Add the next step for a tier with --needs T-{after}, and it waits until T-{after} is accepted",
                lane.handle
            ),
            409,
        ));
    }
    let found = assignee.filter(|session| session.member_id.is_some());
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
    // A session the human ended is not on a task's work: a task of its that is
    // paused waits for its resumption, not for a follow-up.
    let at_work = HELD_TASK_STATES.contains(&previous.state.as_str())
        || matches!(previous.state.as_str(), "open" | "paused");
    // A task whose result is in but not accepted still has follow-ups waiting for
    // it, and one added now lines up behind them (by number), not ahead.
    let others_wait =
        previous.state == "done" && others_wait_for(store, &previous, session.id, before)?;
    let behind = session.left_at.is_none() && (at_work || others_wait);
    if !behind {
        require_free(store, &session, Giving::Task)?;
    }
    Ok(FollowUp {
        session: bring_back(store, session)?,
        behind,
    })
}

/// Whether open tasks bound to `session` wait for `task` to be accepted, other
/// than those the new follow-up is put `before` (the chief says it goes ahead of
/// them: the rule of 2026-10-15).
fn others_wait_for(
    store: &Store,
    task: &TaskRow,
    session: i64,
    before: &[i64],
) -> Result<bool, LedgerError> {
    let waiting: Vec<i64> = store
        .db
        .prepare(
            "SELECT t.number FROM task_need n JOIN task t ON t.id = n.task_id
       WHERE n.needs_id = ? AND t.state = 'open' AND t.assignee_id = ?",
        )?
        .query_map(params![task.id, session], |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(waiting.iter().any(|number| !before.contains(number)))
}

/// How a task comes to a session, which is what a refusal can tell it to do
/// instead of waiting.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Giving {
    /// A follow-up, or a task named for the session: it can be opened for a
    /// tier instead.
    Task,
    /// Task `number`, finished, sent back to the session that did it: it is
    /// that task, and cannot be opened for a tier; what it still needs can be
    /// given as a new one.
    SentBack(i64),
    /// Task `number`, paused, resumed into its session's window: it counts as in
    /// hand while paused, and only another task its window is at work on (queued,
    /// working or waiting) makes the session busy. A follow-up paused while it
    /// waited on the board for its needs is one: the session may have taken
    /// another task since. Another task of the session that is paused too does
    /// not: the session goes on with one, and the other is resumed once that one
    /// has its result (a ledger written before one task per session left some
    /// holding several, and the Resume tells the chief to resume them one at a
    /// time; Astra's review of 2026-10-17).
    Resumed(i64),
}

/// A member session takes one task at a time, and this is the one rule that
/// every door giving a session a task holds it to: a follow-up (`--after`, but
/// for one of the task the window is still on: that waits behind it,
/// `follow_up_session`), a task sent back to it (a reopen) and a task named for
/// it are refused
/// `session-busy` while it has a task in hand ([`has_task_in_hand`]), and so is
/// a paused task resumed into its window while the window is at work on another,
/// so that it is never at work on one task while the board speaks of another.
/// The refusal says what the door can do instead (`giving`). A task that waits
/// on the board for what it needs does not make its session busy: the release
/// (`release_ready`) gives the session's window those one at a time, each once
/// the window is free, so a door that gives the session another task meanwhile
/// leaves the board and the window speaking of one (the owner's report of
/// 2026-10-14, a follow-up refused while another waited for the same idle
/// window). The chief, the human and a member's own lane are no sessions, and
/// are held to nothing.
pub(crate) fn require_free(
    store: &Store,
    taker: &ParticipantRow,
    giving: Giving,
) -> Result<(), LedgerError> {
    if taker.member_id.is_some() && on_other_work(store, taker.id, giving)? {
        let instead = match giving {
            Giving::Task => ", or open the task for its tier".to_owned(),
            Giving::SentBack(number) => {
                format!(" before sending T-{number} back, or open a new task for its tier")
            }
            Giving::Resumed(number) => format!(" before resuming T-{number}"),
        };
        return Err(LedgerError::refused_with(
            "session-busy",
            format!(
                "@{} is still on its work: wait for its result{instead}",
                taker.handle
            ),
            409,
        ));
    }
    Ok(())
}

/// Whether a participant has a task in hand that is not the one `giving` gives
/// it: any, but for a task resumed, which is its own already, and which only a
/// task at work keeps out of its window.
fn on_other_work(store: &Store, participant_id: i64, giving: Giving) -> Result<bool, LedgerError> {
    let Giving::Resumed(number) = giving else {
        return has_task_in_hand(store, participant_id);
    };
    Ok(store
        .db
        .query_row(
            &format!(
                "SELECT 1 FROM task WHERE assignee_id = ? AND number != ? AND state IN ({})",
                sql_list(&HELD_TASK_STATES)
            ),
            params![participant_id, number],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}

/// Whether a participant's window has a task in hand: queued, working or
/// waiting, or paused, whose window stays for the resumption. A follow-up that
/// waits on the board for what it needs is not: nothing of it has reached the
/// window, which closes when it has no task in hand, and the follow-up goes,
/// once its needs are accepted, to a session with none in hand; of several
/// waiting for one session, one at a time (`release_ready`).
pub(crate) fn has_task_in_hand(store: &Store, participant_id: i64) -> Result<bool, LedgerError> {
    has_task_in(
        store,
        participant_id,
        &[&HELD_TASK_STATES[..], &["paused"]].concat(),
    )
}

/// The numbers of the tasks that wait on the board for a participant, oldest
/// first: given it, and open until what they need is accepted.
fn waiting_for(store: &Store, participant_id: i64) -> Result<Vec<i64>, LedgerError> {
    Ok(store
        .db
        .prepare("SELECT number FROM task WHERE assignee_id = ? AND state = 'open' ORDER BY id")?
        .query_map([participant_id], |row| row.get(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?)
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
/// back on the board when resumed. So is one that tasks wait for on the board
/// (follow-ups given it, waiting for their needs): their release would queue
/// them for a session no window opens for again.
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
        let waiting = waiting_for(store, session.id)?;
        if !waiting.is_empty() {
            let numbers: Vec<String> = waiting.iter().map(|number| format!("T-{number}")).collect();
            return Err(LedgerError::refused_with(
                "session-busy",
                format!(
                    "@{handle} has work waiting for it on the board ({}): cancel it first",
                    numbers.join(", ")
                ),
                409,
            ));
        }
        close_session(store, &session, &format!("ended by @{by}"), true)?;
        known_project(store, project_id)
    })
}

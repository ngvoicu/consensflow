//! A task finished: its result recorded and accepted, or sent back; called
//! off by a coordinator, or given up by the daemon; and taken off the board
//! for good by the human once finished. A result is put to its requester to
//! decide on, so a decision taken before the requester was given it withdraws
//! it, in the decision's own transaction: accepting and sending back do
//! (`withdraw_result`). A cancel has no such step to take: a task is called
//! off only before it is done, and `call_off` withdraws every message of it
//! still on its way, a result an older build left among them. A task is not
//! called off while an open task waits for it (`require_unawaited`): what
//! waits for a task is released when it is accepted, so the waiter would be
//! blocked for good. A member's removal cancels the member's tasks whatever
//! waits for them, and a ledger an older build left with such a waiter has
//! one too: the chief is told of each waiter once, at the removal's step
//! (`tell_stranded_waiters`) or at the ledger's opening (`note_stranded_waits`),
//! and the log keeps that it was (`tell_stranded`).

use cf_proto::ledger::{TaskMoved, TaskView};
use rusqlite::params;
use serde_json::json;

use super::pausing::shut_doors;
use super::{
    carrier_body, delivery_body, own_steps, require_on_board, require_task_state, task_by_id,
};
use crate::messages::{
    fold, leave_pause_notes, say_again, tell_sent_back, told_waiting, warnings_on_their_way,
    warnings_queued, Dropped,
};
use crate::model::{
    self, require_active, sql_list, LedgerError, ACTIVE_TASK_STATES, FINISHED_TASK_STATES,
    MAX_BODY, MEMBER_ROLES,
};
use crate::queue::{drop_queued, queue, send, withdraw_gated, withdraw_result, Queued, Sent};
use crate::staff::{bring_back, can_continue, has_task_in_hand, require_free, Giving};
use crate::store::Store;
use crate::views::{ParticipantRow, TaskRow};

/// The assignee's answer finishes the task and is queued for whoever asked for
/// it; the chief's own step is finished by its word (`own_steps`). While words
/// told to its window are on their way to it (`told_waiting`), the answer is to
/// a task without them and is refused, whoever gives it (Astra's look at round
/// 4, 2026-10-16: a worker's `cf task done` finished the task before the words
/// came, and they never did).
pub(crate) fn record_result(
    store: &mut Store,
    project_id: i64,
    number: i64,
    body: &str,
) -> Result<TaskMoved, LedgerError> {
    model::require_text(body, "body", MAX_BODY)?;
    store.write(|store| {
        let task = store.task_row(project_id, number)?;
        if own_steps::is_own_step(store, &task)? {
            return own_steps::finish(store, project_id, &task, body);
        }
        require_task_state(&task, &ACTIVE_TASK_STATES, "record a result for")?;
        if let Some(assignee) = task.assignee_id {
            if told_waiting(store, assignee, task.id)? {
                return Err(LedgerError::refused_with(
                    "words-on-their-way",
                    format!(
                        "T-{number} has words on their way to you: run cf task done again once you have read them"
                    ),
                    409,
                ));
            }
        }
        let message_id = queue(
            store,
            project_id,
            &Queued {
                to: task.requester_id,
                from: task.assignee_id,
                kind: "result",
                task_id: Some(task.id),
                body,
                ..Queued::default()
            },
        )?;
        store.move_task(&task, "done", json!({ "result": message_id }))?;
        release_ready(store, project_id)?;
        Ok(TaskMoved {
            task: task_by_id(store, task.id)?,
            message: store.message(message_id)?,
        })
    })
}

/// A coordinator accepts a task's result: the task is finished, and the
/// tasks given by name that waited for it go to their windows. The result is
/// withdrawn if its requester has not been given it yet: it asks for the
/// decision just taken. A question its window has open at a door is shut out
/// of waiting ([`shut_doors`]).
pub(crate) fn accept_task(
    store: &mut Store,
    project_id: i64,
    number: i64,
    by: &str,
) -> Result<TaskView, LedgerError> {
    store.write(|store| {
        store.participant_by_handle(project_id, by)?;
        let task = store.task_row(project_id, number)?;
        require_task_state(&task, &["done"], "accept")?;
        require_result_received(store, &task, by)?;
        withdraw_gated(store, task.id, &format!("accepted by @{by}"))?;
        withdraw_result(store, &task, "was accepted")?;
        // What is still on its way to the member (an answer that came after its
        // result, say) has no window left to take it.
        if let Some(assignee) = task.assignee_id {
            let member = store.participant_row(assignee)?;
            store.db.execute(
                "UPDATE message SET state = 'cancelled', reason = ?
           WHERE task_id = ? AND recipient_id = ? AND state = 'queued'",
                params![
                    format!(
                        "T-{} was accepted before it reached @{}",
                        task.number, member.handle
                    ),
                    task.id,
                    member.id
                ],
            )?;
        }
        store.move_task(&task, "accepted", json!({ "by": by }))?;
        release_ready(store, project_id)?;
        // A question the window asked through its harness's tool, still open at its
        // door, is not answered any more (`answer` refuses it): the door is told so at
        // once, as a pause tells it, and not when its poll gives up.
        if let Some(window) = task.assignee_id {
            shut_doors(store, &task, window)?;
        }
        task_by_id(store, task.id)
    })
}

/// A result still waiting for the human's approval has not reached the
/// chief: deciding it is the human's alone, who may accept it or send it back.
fn require_result_received(store: &Store, task: &TaskRow, by: &str) -> Result<(), LedgerError> {
    if by == "human" {
        return Ok(());
    }
    let held = store
        .db
        .prepare("SELECT 1 FROM message WHERE task_id = ? AND kind = 'result' AND state = 'gated'")?
        .exists([task.id])?;
    if held {
        return Err(LedgerError::refused_with(
            "result-gated",
            format!(
                "T-{}'s result waits for the human's approval: it reaches you once they pass it on",
                task.number
            ),
            409,
        ));
    }
    Ok(())
}

/// The tasks given to a participant (by name, or as a follow-up) that wait on
/// the board for what they need go to their windows now: each with every task
/// it needs accepted, oldest first, unless its assignee is a member session
/// with a task in hand. That one waits for the session to be free, as it
/// waited for what it needs, and goes when the task in hand is over; of two
/// waiting for one session the first goes and the second waits behind it.
/// Called in the step of whatever made a release possible: an acceptance, or
/// the end of a session's task in hand: its result, its calling off, its
/// failing (`fail_task`, and a delivery that fails for good), and its being
/// taken back for its tier. It refuses nothing of its own, which would fail
/// that step for something else's sake. A session can have several waiting:
/// a door refuses it only while it has a task in hand (`require_free`), and
/// `--before` orders them by what each needs. A follow-up of a task still on its
/// work is one of them: that task is among what it needs, and the window is
/// given the follow-up once the task is accepted.
pub(crate) fn release_ready(store: &mut Store, project_id: i64) -> Result<(), LedgerError> {
    let waiting = store
        .db
        .prepare(
            "SELECT t.* FROM task t
       WHERE t.project_id = ? AND t.state = 'open' AND t.assignee_id IS NOT NULL
         AND EXISTS (SELECT 1 FROM task_need n WHERE n.task_id = t.id)
         AND NOT EXISTS (SELECT 1 FROM task_need n JOIN task d ON d.id = n.needs_id
                         WHERE n.task_id = t.id AND d.state != 'accepted')
       ORDER BY t.id",
        )?
        .query_map([project_id], TaskRow::read)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for task in waiting {
        let mut assignee = store.participant_of(task.assignee_id)?;
        // Never queued for a member's window that has left, which nothing opens: the
        // session the human deleted comes back for it, as for any follow-up, and a
        // session whose member left, or a lane that did, cannot, so the task ends
        // here (a removal cancels such tasks itself; this is for a ledger an older
        // build left).
        if a_members_window(&assignee) && assignee.left_at.is_some() {
            if can_continue(store, &assignee)? {
                assignee = bring_back(store, assignee)?;
            } else {
                cancel_unreachable(store, project_id, &task, &assignee)?;
                continue;
            }
        }
        if assignee.member_id.is_some() && has_task_in_hand(store, assignee.id)? {
            continue;
        }
        let body = delivery_body(&task);
        let message_id = queue(
            store,
            project_id,
            &Queued {
                to: assignee.id,
                from: Some(task.requester_id),
                kind: "task",
                task_id: Some(task.id),
                body: &body,
                ..Queued::default()
            },
        )?;
        store.move_task(&task, "queued", json!({ "message": message_id }))?;
    }
    Ok(())
}

/// Whether `window` is a member's: a session, or the member's own lane.
fn a_members_window(window: &ParticipantRow) -> bool {
    window.member_id.is_some() || MEMBER_ROLES.contains(&window.role.as_str())
}

/// A task that waited on the board for its needs and cannot go to its window,
/// which left: it is cancelled, its requester told, and the tasks that wait for
/// it are told to the chief as stranded. The note says what is true: the member
/// left the staff (a lane, or a session whose member went), or the member stays
/// and the conversation of the session the human deleted was lost.
fn cancel_unreachable(
    store: &mut Store,
    project_id: i64,
    task: &TaskRow,
    window: &ParticipantRow,
) -> Result<(), LedgerError> {
    let member_left = match window.member_id {
        Some(member) => store.participant_row(member)?.left_at.is_some(),
        None => true,
    };
    let why = if member_left {
        let member = window.member_handle.as_deref().unwrap_or(&window.handle);
        format!("@{member} left the staff")
    } else {
        format!("the conversation of @{} was lost", window.handle)
    };
    drop_queued(store, task.id)?;
    store.move_task(task, "cancelled", json!({ "reason": why }))?;
    let requester = store.participant_row(task.requester_id)?;
    send(
        store,
        project_id,
        &Sent {
            to: &requester.handle,
            task: Some(task.number),
            kind: "note",
            body: &format!(
                "T-{} was cancelled: {why}, so no window can take it.",
                task.number
            ),
            ..Sent::default()
        },
    )?;
    tell_stranded_waiters(store, project_id, task)
}

/// A follow-up on a finished or failed task: it goes back to its assignee's
/// queue, and a session the human deleted is brought back to the board for it.
/// A session with another task in hand takes none, and is refused as a
/// follow-up is, `session-busy`, in the words of a task sent back (it is not
/// opened for a tier; what it needs can be given as a new task): its window
/// has one task to work on. The follow-up is a task message that carries what
/// the window kept for the task (which is what delivers what a delivery that
/// failed let go of), and the brief before it when none is received or on its
/// way. A result its requester has not been given yet is withdrawn: it asks
/// for the decision just taken, and the work it reports is sent back.
pub(crate) fn reopen_task(
    store: &mut Store,
    project_id: i64,
    number: i64,
    by: &str,
    body: &str,
) -> Result<TaskMoved, LedgerError> {
    model::require_text(body, "body", MAX_BODY)?;
    store.write(|store| {
        let author = store.participant_by_handle(project_id, by)?;
        let task = store.task_row(project_id, number)?;
        require_task_state(&task, &["done", "failed"], "reopen")?;
        require_result_received(store, &task, by)?;
        let found = store.participant_of(task.assignee_id)?;
        if found.member_id.is_some() && !can_continue(store, &found)? {
            return Err(LedgerError::refused_with(
                "session-ended",
                format!(
                    "@{} has ended: open the task for its tier instead",
                    found.handle
                ),
                409,
            ));
        }
        require_free(store, &found, Giving::SentBack(number))?;
        let assignee = match found.member_id {
            Some(_) => bring_back(store, found)?,
            None => found,
        };
        require_active(&assignee)?;
        withdraw_gated(store, task.id, &format!("sent back by @{by}"))?;
        withdraw_result(store, &task, "was sent back")?;
        let words = carrier_body(store, &task, assignee.id, body)?;
        let message_id = queue(
            store,
            project_id,
            &Queued {
                to: assignee.id,
                from: Some(author.id),
                kind: "task",
                task_id: Some(task.id),
                body: &words,
                ..Queued::default()
            },
        )?;
        fold(store, &task, assignee.id, message_id)?;
        store.move_task(&task, "queued", json!({ "by": by, "message": message_id }))?;
        tell_sent_back(store, &task, assignee.id, by)?;
        Ok(TaskMoved {
            task: task_by_id(store, task.id)?,
            message: store.message(message_id)?,
        })
    })
}

/// A task is called off, and whoever asked for it hears so in the same
/// step, unless it cancelled the task itself: what was cancelled, and the
/// window that was stopped when a member's had begun on it (the daemon
/// stops that window on its next look). Whoever told the window words it
/// never had hears that they went, but the one who called the task off.
/// Refused, `waited-for`, while an open or paused task waits for it.
pub(crate) fn cancel_task(
    store: &mut Store,
    project_id: i64,
    number: i64,
    by: &str,
) -> Result<TaskView, LedgerError> {
    store.write(|store| {
        let off = call_off(store, project_id, number, by, Ending::Cancelled)?;
        let task = &off.task;
        let requester = store.participant_row(task.requester_id)?;
        let lead = format!("@{by} cancelled T-{number} ({})", task.title);
        if requester.handle != by {
            let stopped = off.window.as_ref().map_or_else(String::new, |window| {
                format!(": @{}'s window was stopped", window.handle)
            });
            let dropped = off.dropped.said_to(&requester.handle);
            send(
                store,
                project_id,
                &Sent {
                    to: &requester.handle,
                    task: Some(number),
                    kind: "note",
                    body: &format!("{lead}{stopped}.{dropped}"),
                    ..Sent::default()
                },
            )?;
        }
        off.dropped
            .tell_the_others(store, task, &format!("{lead}."), &[by, &requester.handle])?;
        task_by_id(store, task.id)
    })
}

/// How a task is called off: cancelled, or its brief declined at the gate.
#[derive(Clone, Copy)]
pub(crate) enum Ending {
    Cancelled,
    Declined,
}

/// What a cancel called off: the task as it was, the member's window that had
/// begun on it, if one had, and the words told to its window that it never had.
pub(crate) struct CalledOff {
    pub(crate) task: TaskRow,
    pub(crate) window: Option<ParticipantRow>,
    pub(crate) dropped: Dropped,
}

/// The cancel itself, for an operation that tells the requester in its own
/// words (the human's decline of a brief). Whatever of the task is still on
/// its way, a delivery into a window included, is withdrawn, so nothing of
/// it is delivered or tried again. The member's window it gives is one whose
/// brief or the words that resume the task went in. Refused, `waited-for`, while
/// an open or paused task waits for it, the human's decline of its brief included:
/// the waiter is released when the task is accepted, never when it is cancelled.
/// `ending` is how it ends, which the refusal says in its words.
pub(crate) fn call_off(
    store: &mut Store,
    project_id: i64,
    number: i64,
    by: &str,
    ending: Ending,
) -> Result<CalledOff, LedgerError> {
    store.participant_by_handle(project_id, by)?;
    let task = store.task_row(project_id, number)?;
    let cancellable = [&["open", "queued"], &ACTIVE_TASK_STATES[..], &["paused"]].concat();
    require_task_state(&task, &cancellable, "cancel")?;
    require_unawaited(store, &task, ending)?;
    let dropped = Dropped::of(store, &task)?;
    let assignee = task
        .assignee_id
        .map(|id| store.participant_row(id))
        .transpose()?;
    let began = match &assignee {
        Some(assignee) if MEMBER_ROLES.contains(&assignee.role.as_str()) => store
            .db
            .prepare(
                "SELECT 1 FROM message WHERE task_id = ? AND recipient_id = ? AND kind = 'task'
           AND state IN ('delivering', 'delivered')",
            )?
            .exists(params![task.id, assignee.id])?,
        _ => false,
    };
    // A warning its pause notes say is kept before they go with the rest; one
    // that says it on its way runs its course, and says it again only if it
    // does not arrive (`withdrawn_on_its_way`).
    let warnings = warnings_queued(store, &task)?;
    let on_their_way = warnings_on_their_way(store, &task)?;
    store.db.execute(
        "UPDATE message SET state = 'cancelled', reason = ?
       WHERE task_id = ? AND state IN ('queued', 'delivering', 'gated')
         AND id NOT IN (SELECT value FROM json_each(?))",
        params![
            format!("cancelled by @{by}"),
            task.id,
            json!(on_their_way).to_string()
        ],
    )?;
    // A note of several that names it, still queued, names it no more.
    leave_pause_notes(store, &task, "cancelled")?;
    say_again(store, &task, "cancelled", warnings)?;
    store.move_task(&task, "cancelled", json!({ "by": by }))?;
    release_ready(store, project_id)?;
    Ok(CalledOff {
        task,
        window: assignee.filter(|_| began),
        dropped,
    })
}

/// The tasks that wait for `task`, oldest first: those still open or paused
/// (a paused one goes back to the board when resumed) with `task` among what
/// they need. One that has gone to a window had everything it needed
/// accepted, and one cancelled or failed waits for nothing.
fn waiters(store: &Store, task: &TaskRow) -> Result<Vec<i64>, LedgerError> {
    Ok(store
        .db
        .prepare(
            "SELECT t.number FROM task_need n JOIN task t ON t.id = n.task_id
       WHERE n.needs_id = ? AND t.state IN ('open', 'paused') ORDER BY t.number",
        )?
        .query_map([task.id], |row| row.get::<_, i64>(0))?
        .collect::<rusqlite::Result<_>>()?)
}

/// "T-2, T-3": task numbers in a sentence.
fn task_numbers(numbers: &[i64]) -> String {
    let names: Vec<String> = numbers.iter().map(|number| format!("T-{number}")).collect();
    names.join(", ")
}

/// A task an open task waits for is not called off: the waiter is released
/// when the task is accepted, so it would read "blocked by" it for good and
/// nothing would tell anyone. Whoever cancels names the waiters to cancel
/// first, or lets the task finish; the human who declines its brief, which
/// never started, passes it on.
fn require_unawaited(store: &Store, task: &TaskRow, ending: Ending) -> Result<(), LedgerError> {
    let waiting = waiters(store, task)?;
    if waiting.is_empty() {
        return Ok(());
    }
    let (verb, them) = match waiting.as_slice() {
        [only] => ("waits", format!("T-{only}")),
        _ => ("wait", "them".to_owned()),
    };
    let number = task.number;
    let otherwise = match ending {
        Ending::Cancelled => format!("let T-{number} finish"),
        Ending::Declined => format!("pass T-{number} on"),
    };
    Err(LedgerError::refused_with(
        "waited-for",
        format!(
            "T-{number} cannot be cancelled while {names} {verb} for it: cancel {them} first, or {otherwise}",
            names = task_numbers(&waiting),
        ),
        409,
    ))
}

/// The chief is told, once, that the open or paused task `number` waits for a
/// cancelled task and will never start: a note that names what it waits on,
/// and the log keeps that it was (`task.stranded`, in the same step), so no
/// removal and no opening says it twice. Nothing is said of a task that waits
/// for no cancelled one, of one already told, or in a project with no chief.
pub(crate) fn tell_stranded(
    store: &mut Store,
    project_id: i64,
    number: i64,
) -> Result<(), LedgerError> {
    let told = store
        .db
        .prepare(
            "SELECT 1 FROM event
       WHERE kind = 'task.stranded' AND project_id = ? AND json_extract(data, '$.task') = ?",
        )?
        .exists(params![project_id, number])?;
    let chief: Option<String> = store
        .db
        .prepare(
            "SELECT handle FROM participant
       WHERE project_id = ? AND role = 'chief' AND left_at IS NULL ORDER BY id DESC",
        )?
        .query_map([project_id], |row| row.get(0))?
        .next()
        .transpose()?;
    let cancelled: Vec<i64> = store
        .db
        .prepare(
            "SELECT d.number FROM task t JOIN task_need n ON n.task_id = t.id
         JOIN task d ON d.id = n.needs_id
       WHERE t.project_id = ? AND t.number = ? AND d.state = 'cancelled' ORDER BY d.number",
        )?
        .query_map(params![project_id, number], |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    let Some(chief) = chief.filter(|_| !told && !cancelled.is_empty()) else {
        return Ok(());
    };
    let told = send(
        store,
        project_id,
        &Sent {
            to: &chief,
            task: Some(number),
            kind: "note",
            body: &format!(
                "T-{number} waits on cancelled {} and will never start.",
                task_numbers(&cancelled)
            ),
            ..Sent::default()
        },
    )?;
    store.log(
        project_id,
        "task.stranded",
        json!({ "task": number, "needs": cancelled, "message": told.id }),
    )
}

/// The chief is told of each task that waits for `task`, which a step that
/// does not refuse for its waiters has just cancelled (a member's removal).
/// Called after all the step's cancels, so a waiter of several is told once,
/// naming each.
pub(crate) fn tell_stranded_waiters(
    store: &mut Store,
    project_id: i64,
    task: &TaskRow,
) -> Result<(), LedgerError> {
    for number in waiters(store, task)? {
        tell_stranded(store, project_id, number)?;
    }
    Ok(())
}

/// What a ledger an older build left with a task waiting for a cancelled one
/// says at its opening: up to 3.0.5 a cancel was never refused for the tasks
/// that waited for it, and the waiter read "blocked by" it for good. Each such
/// open or paused task is told to its project's chief once (`tell_stranded`).
pub(crate) fn note_stranded_waits(store: &mut Store) -> Result<(), LedgerError> {
    store.write(|store| {
        let stranded: Vec<(i64, i64)> = store
            .db
            .prepare(
                "SELECT t.project_id, t.number FROM task t
           WHERE t.state IN ('open', 'paused')
             AND EXISTS (SELECT 1 FROM task_need n JOIN task d ON d.id = n.needs_id
                         WHERE n.task_id = t.id AND d.state = 'cancelled')
           ORDER BY t.project_id, t.number",
            )?
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        for (project, number) in stranded {
            tell_stranded(store, project, number)?;
        }
        Ok(())
    })
}

/// The daemon gives up on a task: its pane died, or its launch never came up.
/// The tasks that wait for it (a follow-up queued behind it, a task that
/// needs it) stay on the board: they are released when it is accepted, which
/// a reopening can still bring about. Each one's requester is told once that
/// it waits for a failed task, and what to do: cancel the waiter, or reopen
/// the task.
pub(crate) fn fail_task(
    store: &mut Store,
    project_id: i64,
    number: i64,
    reason: &str,
) -> Result<TaskView, LedgerError> {
    model::require_text(reason, "reason", 1000)?;
    store.write(|store| {
        let task = store.task_row(project_id, number)?;
        let failable = [&["open", "queued"], &ACTIVE_TASK_STATES[..]].concat();
        require_task_state(&task, &failable, "fail")?;
        drop_queued(store, task.id)?;
        store.move_task(&task, "failed", json!({ "reason": reason }))?;
        release_ready(store, project_id)?;
        for waiting in waiters(store, &task)? {
            let waiter = store.task_row(project_id, waiting)?;
            let requester = store.participant_row(waiter.requester_id)?;
            send(
                store,
                project_id,
                &Sent {
                    to: &requester.handle,
                    task: Some(waiting),
                    kind: "note",
                    body: &format!(
                        "T-{waiting} waits for T-{number}, which failed: cancel T-{waiting}, or reopen T-{number} and let it finish."
                    ),
                    ..Sent::default()
                },
            )?;
        }
        task_by_id(store, task.id)
    })
}

/// The human takes finished tasks off the board for good: each leaves the
/// board and `cf task list`, and keeps its row, so its number is never
/// given again, the threads that name it stay whole and `cf task get` still
/// reads it. Only a finished task goes, and none a task not yet finished
/// still needs; all of them go, or none. Nobody is told: the human tidies
/// the board.
pub(crate) fn delete_tasks(
    store: &mut Store,
    project_id: i64,
    numbers: &[i64],
) -> Result<Vec<TaskView>, LedgerError> {
    let finished = sql_list(&FINISHED_TASK_STATES);
    let mut distinct: Vec<i64> = Vec::new();
    for number in numbers {
        if !distinct.contains(number) {
            distinct.push(*number);
        }
    }
    store.write(|store| {
        distinct
            .into_iter()
            .map(|number| {
                let task = store.task_row(project_id, number)?;
                require_on_board(&task, "delete")?;
                if !FINISHED_TASK_STATES.contains(&task.state.as_str()) {
                    let next = if task.state == "done" {
                        "the chief accepts it or sends it back first"
                    } else {
                        "cancel it first"
                    };
                    return Err(LedgerError::refused_with(
                        "not-finished",
                        format!(
                            "T-{number} is {}: only a finished task leaves the board; {next}",
                            task.state
                        ),
                        409,
                    ));
                }
                let needing = store
                    .db
                    .prepare(&format!(
                        "SELECT w.number FROM task_need n JOIN task w ON w.id = n.task_id
           WHERE n.needs_id = ? AND w.state NOT IN ({finished}) ORDER BY w.number"
                    ))?
                    .query_map([task.id], |row| {
                        Ok(format!("T-{}", row.get::<_, i64>("number")?))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                if let [needed_by] = needing.as_slice() {
                    return Err(LedgerError::refused_with(
                        "task-needed",
                        format!(
                            "{needed_by} still needs T-{number}: it stays on the board until {needed_by} is finished"
                        ),
                        409,
                    ));
                }
                if !needing.is_empty() {
                    return Err(LedgerError::refused_with(
                        "task-needed",
                        format!(
                            "{} still need T-{number}: it stays on the board until they are finished",
                            needing.join(", ")
                        ),
                        409,
                    ));
                }
                let at = store.at();
                store.db.execute(
                    "UPDATE task SET deleted_at = ? WHERE id = ?",
                    params![at, task.id],
                )?;
                store.log(
                    project_id,
                    "task.deleted",
                    json!({ "task": number, "state": task.state }),
                )?;
                task_by_id(store, task.id)
            })
            .collect()
    })
}

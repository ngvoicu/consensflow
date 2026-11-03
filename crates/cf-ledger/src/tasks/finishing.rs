//! A task finished: its result recorded and accepted, or sent back; called
//! off by a coordinator, or given up by the daemon; and taken off the board
//! for good by the human once finished. A result is put to its requester to
//! decide on, so a decision taken before the requester was given it withdraws
//! it, in the decision's own transaction: accepting, sending back and calling
//! the task off do (`withdraw_result`). A failed task is accepted too, on the
//! words of what the coordinator checked, which are its result
//! (`accept_failed_task`). A task that is done is called off as a decision of its
//! result (the chief's text names it as one): its result is decided as an
//! acceptance decides it, and its window, whose turn ended with the result, is
//! stopped for nothing (it closes by itself when it holds nothing else, as after
//! an acceptance, and stays while it holds a paused task). A task still at work is
//! called off with every message of it still on its way withdrawn (`call_off`), a
//! result an older build left among them. A task is not
//! called off while an open task waits for it (`require_unawaited`): what
//! waits for a task is released when it is accepted, so the waiter would be
//! blocked for good. A member's removal cancels the member's tasks whatever
//! waits for them, and a ledger an older build left with such a waiter has
//! one too: the chief is told of each waiter once, at the removal's step
//! (`tell_stranded_waiters`) or at the ledger's opening (`note_stranded_waits`),
//! and the log keeps that it was (`tell_stranded`).

use cf_base::js;
use cf_proto::ledger::{TaskMoved, TaskView};
use rusqlite::{params, OptionalExtension};
use serde_json::json;

use super::pausing::shut_doors;
use super::{
    carrier_body, delivery_body, own_steps, require_on_board, require_task_state, resume_words,
    task_by_id,
};
use crate::messages::{
    drop_queued_but_told, fold, leave_pause_notes, notes_waiting, put_back_paused_answers,
    queued_paused_answers, release_carried, say_again, tell_sent_back, warnings_on_their_way,
    warnings_queued, withdraw_open_questions, withdraw_paused_answers, Afterwards, Dropped,
};
use crate::model::{
    self, require_active, sql_list, LedgerError, ACTIVE_TASK_STATES, FINISHED_TASK_STATES,
    MAX_BODY, MEMBER_ROLES,
};
use crate::queue::{drop_queued, queue, send, withdraw_gated, withdraw_result, Queued, Sent};
use crate::staff::{
    bring_back, can_continue, has_task_in_hand, require_free, undecided_results, Giving,
};
use crate::store::Store;
use crate::views::{ParticipantRow, TaskRow};

/// The assignee's answer finishes the task and is queued for whoever asked for
/// it; the chief's own step is finished by its word (`own_steps`). While notes
/// to its window about it are on their way there (`notes_waiting`), the answer is to
/// a task without them and is refused, whoever gives it (Astra's look at round
/// 4, 2026-10-16: a worker's `cf task done` finished the task before the words
/// came, and they never did).
pub(crate) fn record_result(
    store: &mut Store,
    project_id: i64,
    number: i64,
    body: &str,
) -> Result<TaskMoved, LedgerError> {
    record(store, project_id, number, body, &ACTIVE_TASK_STATES, false)
}

/// The chief or the human takes what the worker said last as the result (Finish):
/// of a task at work, or of a paused one, whose worker's turn ended with its
/// answer after the tell that paused it (`turn_end`'s paused answer). The pause
/// is the finisher's own to end: the notes it left queued for the requester go
/// ([`leave_pause_notes`]), as a resume's do. `left` is how many background jobs and
/// agents its window was counted running, which finishing does not end and the close after
/// the accept does: kept with the move to done, for the accept to say (`left_running`).
pub(crate) fn record_finished(
    store: &mut Store,
    project_id: i64,
    number: i64,
    body: &str,
    left: (usize, usize),
) -> Result<TaskMoved, LedgerError> {
    record_counted(
        store,
        project_id,
        number,
        body,
        &FINISHABLE_TASK_STATES,
        true,
        left,
    )
}

/// How many background jobs and agents the window of T-`number` was counted running when the
/// chief or the human finished the task (`record_finished`): the closing of its window after
/// the accept ends them (T-691, the field report of 2026-10-26, where a parity run was stopped
/// that way). None where the task was finished by its worker's own word, which waits for the
/// work to end, or the finish counted none. What ran then may have ended since; the count is
/// what was known when the chief decided to finish.
pub(crate) fn left_running(
    store: &Store,
    project_id: i64,
    number: i64,
) -> Result<(i64, i64), LedgerError> {
    let counted: Option<(Option<i64>, Option<i64>)> = store
        .db
        .query_row(
            "SELECT json_extract(data, '$.jobs'), json_extract(data, '$.agents') FROM event
              WHERE project_id = ?1 AND kind = 'task.state' AND json_valid(data)
                AND json_extract(data, '$.task') = ?2 AND json_extract(data, '$.to') = 'done'
              ORDER BY id DESC LIMIT 1",
            params![project_id, number],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    Ok(counted.map_or((0, 0), |(jobs, agents)| {
        (jobs.unwrap_or(0), agents.unwrap_or(0))
    }))
}

/// The states a task can be finished at the word of the chief or the human.
const FINISHABLE_TASK_STATES: [&str; 3] = ["working", "waiting", "paused"];

/// `finished_by_word`: the chief or the human finishes the task, so its window's open
/// questions are withdrawn with the move; the window's own result leaves them to be
/// answered until the task is accepted.
fn record(
    store: &mut Store,
    project_id: i64,
    number: i64,
    body: &str,
    states: &[&str],
    finished_by_word: bool,
) -> Result<TaskMoved, LedgerError> {
    record_counted(
        store,
        project_id,
        number,
        body,
        states,
        finished_by_word,
        (0, 0),
    )
}

/// [`record`], and where the finish was at a word, the `left` jobs and agents its window was
/// counted running, kept with the move to done for the accept to say.
fn record_counted(
    store: &mut Store,
    project_id: i64,
    number: i64,
    body: &str,
    states: &[&str],
    finished_by_word: bool,
    left: (usize, usize),
) -> Result<TaskMoved, LedgerError> {
    model::require_text(body, "body", MAX_BODY)?;
    store.write(|store| {
        let task = store.task_row(project_id, number)?;
        if own_steps::is_own_step(store, &task)? {
            return own_steps::finish(store, project_id, &task, body);
        }
        require_task_state(&task, states, "record a result for")?;
        if let Some(assignee) = task.assignee_id {
            if notes_waiting(store, assignee, task.id)? {
                return Err(LedgerError::refused_with(
                    "words-on-their-way",
                    format!(
                        "T-{number} has words on their way to you: run cf task done again once you have read them"
                    ),
                    409,
                ));
            }
        }
        // The words that resumed a task its window is at work on (`work_while_resumed`), or
        // that waits for an answer meanwhile, wait for its rest, and the window has not
        // read them: its own `cf task done` is refused as it is for notes on their way,
        // since what it would finish is the task without them. The chief's Finish takes
        // the answer of the turn at work, and the words go with it: left queued for a task
        // that is done, a reopen of it would put them in with its follow-up.
        if matches!(task.state.as_str(), "working" | "waiting") {
            if let Some((words, state)) = resume_words(store, &task)? {
                if !finished_by_word {
                    return Err(LedgerError::refused_with(
                        "words-on-their-way",
                        format!(
                            "T-{number} has words on their way to you: run cf task done again once you have read them"
                        ),
                        409,
                    ));
                }
                if state != "delivering" {
                    store.db.execute(
                        "UPDATE message SET state = 'cancelled', reason = ? WHERE id = ?",
                        params![format!("T-{number} was finished before they went in"), words],
                    )?;
                    release_carried(store, task.id)?;
                }
            }
        }
        // The words the paused task's window ended its turn with are the result: the
        // note that gave them is moot.
        if task.state == "paused" {
            withdraw_paused_answers(store, &task, "finished")?;
            leave_pause_notes(store, &task, "finished", Afterwards::MovesOn)?;
        }
        // Finished at a word with a question of its window still open (it waited for the
        // answer, or was paused on it), nobody waits for the answer any more: the question is
        // withdrawn, and `cf answer` no longer reaches it (T-1119).
        if finished_by_word {
            withdraw_open_questions(store, &task, "was finished")?;
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
        // What a finish at a word counted running is kept for the accept to say.
        let detail = if finished_by_word {
            json!({ "result": message_id, "jobs": left.0, "agents": left.1 })
        } else {
            json!({ "result": message_id })
        };
        store.move_task(&task, "done", detail)?;
        release_ready(store, project_id)?;
        // A question its window asked through its harness's tool, still open at its door, is
        // shut out of waiting as an acceptance does.
        if let (true, Some(assignee)) = (finished_by_word, task.assignee_id) {
            shut_doors(store, &task, assignee)?;
        }
        Ok(TaskMoved {
            task: task_by_id(store, task.id)?,
            message: store.message(message_id)?,
            withdrawn: Vec::new(),
            landing: Vec::new(),
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
        accepted(store, project_id, &task, by, None)
    })
}

/// A coordinator accepts a failed task whose work they checked (T-1111: a designer's
/// session died with "Selected model is at capacity" after it had saved both files, and the
/// task that needed it stayed stuck). The words of what they checked are required, and
/// they are the task's result: kept in its thread from whoever wrote them, received as
/// they are written, never pasted. What waits for the task goes on as it does for any
/// accepted task. A failed task nobody accepts stays failed, and is reopened as it was.
pub(crate) fn accept_failed_task(
    store: &mut Store,
    project_id: i64,
    number: i64,
    by: &str,
    checked: &str,
) -> Result<TaskView, LedgerError> {
    store.write(|store| {
        let author = store.participant_by_handle(project_id, by)?;
        let task = store.task_row(project_id, number)?;
        require_task_state(&task, &["failed"], "accept")?;
        if js::trim(checked).is_empty() {
            return Err(LedgerError::refused(
                "checked-needed",
                format!(
                    "T-{number} failed: accepting it takes the words of what you checked, which are its result: cf task accept T-{number} \"what I checked\""
                ),
            ));
        }
        model::require_text(checked, "body", MAX_BODY)?;
        accepted(store, project_id, &task, by, Some((author.id, checked)))
    })
}

/// What both accepts do once `task` may be accepted: what the human has not passed on, and
/// what its requester was not given yet, is withdrawn; what is still on its way to its
/// window goes no further; the task moves, with the result that `checked` (the one who
/// accepts it, and what they checked) makes of a failed task; and what waits for it is
/// released.
fn accepted(
    store: &mut Store,
    project_id: i64,
    task: &TaskRow,
    by: &str,
    checked: Option<(i64, &str)>,
) -> Result<TaskView, LedgerError> {
    require_result_received(store, task, by)?;
    withdraw_gated(store, task.id, &format!("accepted by @{by}"))?;
    withdraw_result(store, task, "was accepted")?;
    // What is still on its way to the member (an answer that came after its
    // result, say, or words told to a window that failed before it had them)
    // has no window left to take it.
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
    let detail = match checked {
        // The words are the result, held by the one who wrote them: no hand-off, so no gate.
        Some((author, words)) => {
            let result = queue(
                store,
                project_id,
                &Queued {
                    to: author,
                    from: Some(author),
                    kind: "result",
                    task_id: Some(task.id),
                    body: words,
                    from_window: true,
                    ..Queued::default()
                },
            )?;
            json!({ "by": by, "result": result })
        }
        None => json!({ "by": by }),
    };
    store.move_task(task, "accepted", detail)?;
    release_ready(store, project_id)?;
    // A question the window asked through its harness's tool, still open at its
    // door, is not answered any more (`answer` refuses it): the door is told so at
    // once, as a pause tells it, and not when its poll gives up.
    if let Some(window) = task.assignee_id {
        shut_doors(store, task, window)?;
    }
    task_by_id(store, task.id)
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
        // A window takes nothing new while a task of it is in hand, nor while one has its
        // result in and waits for the chief's decision, which could send it back (T-1118).
        if assignee.member_id.is_some()
            && (has_task_in_hand(store, assignee.id)?
                || !undecided_results(store, assignee.id)?.is_empty())
        {
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
/// which left: it is cancelled, its requester told, whoever told its window
/// words it never had told which went, as a cancel tells them (T-389), and the
/// tasks that wait for it are told to the chief as stranded. The note says what
/// is true: the member left the staff (a lane, or a session whose member went),
/// or the member stays and the conversation of the session the human deleted
/// was lost.
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
    let dropped = Dropped::of(store, task)?;
    drop_queued(store, task.id)?;
    store.move_task(task, "cancelled", json!({ "reason": why }))?;
    let requester = store.participant_row(task.requester_id)?;
    let lead = format!(
        "T-{} was cancelled: {why}, so no window can take it.",
        task.number
    );
    send(
        store,
        project_id,
        &Sent {
            to: &requester.handle,
            task: Some(task.number),
            kind: "note",
            body: &format!("{lead}{}", dropped.said_to(&requester.handle)),
            ..Sent::default()
        },
    )?;
    dropped.tell_the_others(store, task, &lead, &[&requester.handle])?;
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
            withdrawn: Vec::new(),
            landing: Vec::new(),
        })
    })
}

/// A task is called off, and whoever asked for it hears so in the same
/// step, unless it cancelled the task itself: what was cancelled, and the
/// window that was stopped when a member's had begun on it (the daemon
/// stops that window on its next look). Whoever told the window words it
/// never had hears that they went, but the one who called the task off.
/// Refused, `waited-for`, while an open or paused task waits for it. A task that
/// is done is called off too, as a decision on its result (`call_off`): its
/// window is told nothing and stopped for nothing.
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
/// begun on it, if one had and is still on the board, and the words told to its
/// window that it never had.
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
///
/// A task that is done is called off too: the chief's text offers it beside
/// accepting and sending back, and a result that cannot be cancelled leaves
/// acceptance as the only decision on work that may never have been done (the
/// report of 2026-11-02, T-1122). It is the decision it is, so it is taken as an
/// acceptance is: a result held at the gate is the human's to decide
/// (`require_result_received`), a result the requester was not given yet is
/// withdrawn, one being pasted or given stays as it was, and what is still queued
/// for the window is not delivered. The window's turn ended with the result: none
/// is stopped, so the requester is not told one was, and the window closes by itself
/// when it holds nothing else (`close_if_free`), as after an acceptance, and stays
/// while it holds a paused task, which this leaves as it was. Nothing is released:
/// the tasks that wait for it are refused above, and a task already in a window keeps
/// its need on a cancelled task, as one cancelled under it would.
pub(crate) fn call_off(
    store: &mut Store,
    project_id: i64,
    number: i64,
    by: &str,
    ending: Ending,
) -> Result<CalledOff, LedgerError> {
    store.participant_by_handle(project_id, by)?;
    let task = store.task_row(project_id, number)?;
    let cancellable = [
        &["open", "queued"],
        &ACTIVE_TASK_STATES[..],
        &["paused", "done"],
    ]
    .concat();
    require_task_state(&task, &cancellable, "cancel")?;
    let decided = task.state == "done";
    if decided {
        require_result_received(store, &task, by)?;
    }
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
    let mut warnings = Vec::new();
    if decided {
        // The decision on a result, as an acceptance takes it: what asks for the
        // decision goes, what is on its way to the window is not delivered, and
        // what the requester was given, or is being given, stays as it was.
        withdraw_gated(store, task.id, &format!("cancelled by @{by}"))?;
        withdraw_result(store, &task, "was cancelled")?;
        if let Some(window) = task.assignee_id {
            store.db.execute(
                "UPDATE message SET state = 'cancelled', reason = ?
               WHERE task_id = ? AND recipient_id = ? AND state = 'queued'",
                params![
                    format!(
                        "T-{} was cancelled before it reached @{}",
                        task.number,
                        store.participant_row(window)?.handle
                    ),
                    task.id,
                    window
                ],
            )?;
        }
    } else {
        // A warning its pause notes say is kept before they go with the rest; one
        // that says it on its way runs its course, and says it again only if it
        // does not arrive (`withdrawn_on_its_way`).
        warnings = warnings_queued(store, &task)?;
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
    }
    // A note of several that names it, still queued, names it no more.
    leave_pause_notes(store, &task, "cancelled", Afterwards::MovesOn)?;
    say_again(store, &task, "cancelled", warnings)?;
    store.move_task(&task, "cancelled", json!({ "by": by }))?;
    release_ready(store, project_id)?;
    Ok(CalledOff {
        task,
        // A window that left the board with its member is no window to stop, and
        // neither is one whose turn ended with the result of a task that is done.
        window: assignee.filter(|window| began && !decided && window.left_at.is_none()),
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

/// What a note of the failed task `number` adds of the tasks that wait for it and were asked
/// for by the same requester, after the words of the failure: a sentence led by a space, none
/// where nothing waits.
fn wait_for_it(number: i64, waiting: &[i64]) -> String {
    match waiting {
        [] => String::new(),
        [only] => {
            format!(
                " T-{only} waits for it: cancel T-{only}, or reopen T-{number} and let it finish."
            )
        }
        _ => format!(
            " {} wait for it: cancel them, or reopen T-{number} and let it finish.",
            task_numbers(waiting)
        ),
    }
}

/// What the note of the failed `task` to the requester `requester` adds of the tasks that wait
/// for it and were asked for by that requester ([`wait_for_it`]): a sentence led by a space,
/// none where nothing of theirs waits.
pub(crate) fn waiting_for_it(
    store: &Store,
    task: &TaskRow,
    requester: &str,
) -> Result<String, LedgerError> {
    let mut here = Vec::new();
    for waiting in waiters(store, task)? {
        let waiter = store.task_row(task.project_id, waiting)?;
        if store.participant_row(waiter.requester_id)?.handle == requester {
            here.push(waiting);
        }
    }
    Ok(wait_for_it(task.number, &here))
}

/// The requester of each task that waits for the failed `task` is told, in a note of its own
/// about the waiter, that it waits for a failed task and what to do: cancel the waiter,
/// reopen the task, or accept it on what they checked. Not `named`, whose note of the failure names the tasks it asked for
/// ([`waiting_for_it`]). Called after the words of the failure, which the requester is given
/// first.
fn tell_other_waiters(
    store: &mut Store,
    task: &TaskRow,
    named: Option<&str>,
) -> Result<(), LedgerError> {
    let number = task.number;
    for waiting in waiters(store, task)? {
        let waiter = store.task_row(task.project_id, waiting)?;
        let requester = store.participant_row(waiter.requester_id)?;
        if named == Some(requester.handle.as_str()) {
            continue;
        }
        send(
            store,
            task.project_id,
            &Sent {
                to: &requester.handle,
                task: Some(waiting),
                kind: "note",
                body: &format!(
                    "T-{waiting} waits for T-{number}, which failed: cancel T-{waiting}, reopen T-{number} and let it finish, or accept T-{number} with what you checked: cf task accept T-{number} \"…\""
                ),
                ..Sent::default()
            },
        )?;
    }
    Ok(())
}

/// A task the daemon gave up on through a delivery that did not arrive
/// (`give_up_delivery`), whose requester the engine has told why in the words of the
/// delivery: the requester `named` there has the tasks that wait for it in that note
/// ([`waiting_for_it`]), and the other requesters of waiters are told now, each in a note of
/// its own, after those words.
pub(crate) fn tell_waiting_for_failed(
    store: &mut Store,
    project_id: i64,
    number: i64,
    named: &str,
) -> Result<(), LedgerError> {
    store.write(|store| {
        let task = store.task_row(project_id, number)?;
        if task.state != "failed" {
            return Ok(());
        }
        tell_other_waiters(store, &task, Some(named))
    })
}

/// [`waiting_for_it`] of task `number`, for a note the engine writes to `requester`.
pub(crate) fn waiting_for_it_of(
    store: &Store,
    project_id: i64,
    number: i64,
    requester: &str,
) -> Result<String, LedgerError> {
    let task = store.task_row(project_id, number)?;
    if task.state != "failed" {
        return Ok(String::new());
    }
    waiting_for_it(store, &task, requester)
}

/// A task an open task waits for is not called off: the waiter is released
/// when the task is accepted, so it would read "blocked by" it for good and
/// nothing would tell anyone. Whoever cancels names the waiters to cancel
/// first, or lets the task finish, or accepts it where it is done and has no
/// finishing left; the human who declines its brief, which never started, passes
/// it on.
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
    // What a done task has left to do is to be accepted, not to finish.
    let says = match ending {
        Ending::Cancelled if task.state == "done" => {
            format!("accept T-{number}, or cancel {them} first")
        }
        Ending::Cancelled => format!("cancel {them} first, or let T-{number} finish"),
        Ending::Declined => format!("cancel {them} first, or pass T-{number} on"),
    };
    Err(LedgerError::refused_with(
        "waited-for",
        format!(
            "T-{number} cannot be cancelled while {names} {verb} for it: {says}",
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

/// What a failed task's requester is told to do with it, in one sentence: reopen it, or
/// accept it with what they checked (T-1111), which the chief does when the work it
/// reports is done though the window died. One fate, said in these words wherever a
/// task is said to have failed: the ledger's give-up and the engine's note of a delivery
/// that failed its task.
pub fn reopen_or_accept(number: i64) -> String {
    format!(
        " Reopen it with: cf task reopen T-{number} \"…\", or accept it with what you checked: cf task accept T-{number} \"…\""
    )
}

/// A task fails: its pane died, or its launch never came up. Words told to its
/// window that it never had go with what was queued, and whoever told them hears
/// which, as a cancel tells it (T-389), but its requester. The tasks that wait for
/// it (a follow-up queued behind it, a task that needs it) stay on the board: they
/// are released when it is accepted, which a reopening can still bring about, or
/// the chief's word on its work (`accept_failed_task`). Each
/// one's requester is told once that it waits for a failed task, and what to do:
/// cancel the waiter, reopen the task, or accept it on what they checked. This is Node's ledger's `failTask`, which
/// its recordings call, and only they: the daemon gives up with [`give_up_task`].
pub(crate) fn fail_task(
    store: &mut Store,
    project_id: i64,
    number: i64,
    reason: &str,
) -> Result<TaskView, LedgerError> {
    fail(store, project_id, number, reason, false)
}

/// The daemon gives up on a task, as [`fail_task`] fails it, but for the words
/// told to its window that it never had: they wait for its reopen, which carries
/// them, as those a delivery given up for good leaves do (the chief's decision on
/// Calliope's P3-4 of T-446). Its requester hears why in the same step, which
/// words wait and how to reopen it, in one note (her review of T-437), and each
/// other teller hears the same of its own.
pub(crate) fn give_up_task(
    store: &mut Store,
    project_id: i64,
    number: i64,
    reason: &str,
) -> Result<TaskView, LedgerError> {
    fail(store, project_id, number, reason, true)
}

fn fail(
    store: &mut Store,
    project_id: i64,
    number: i64,
    reason: &str,
    given_up: bool,
) -> Result<TaskView, LedgerError> {
    model::require_text(reason, "reason", 1000)?;
    store.write(|store| {
        let task = store.task_row(project_id, number)?;
        let failable = [&["open", "queued"], &ACTIVE_TASK_STATES[..]].concat();
        require_task_state(&task, &failable, "fail")?;
        let told = Dropped::of(store, &task)?;
        // The answer of a window's turn on a tell, noted for the requester and not yet given
        // (an early resume, then a resumed turn that failed), is put back queued after the
        // drop: the requester is given it at its next rest, in the words of a task that has
        // moved on, then the note that the task failed (T-442). It does not wait for the
        // reopen.
        let answers = queued_paused_answers(store, &task)?;
        if given_up {
            drop_queued_but_told(store, task.id)?;
        } else {
            drop_queued(store, task.id)?;
        }
        put_back_paused_answers(store, &answers)?;
        store.move_task(&task, "failed", json!({ "reason": reason }))?;
        release_ready(store, project_id)?;
        let requester = store.participant_row(task.requester_id)?;
        let failed = |waiting: String| {
            format!(
                "T-{number} failed: {reason}{waiting}.{}",
                reopen_or_accept(number)
            )
        };
        if given_up {
            told.tell_each(store, &task, &[&requester.handle], |teller| {
                failed(told.waiting_for_the_reopen(Some(teller)))
            })?;
        } else {
            // A reopening carries none of them.
            told.tell_the_others(
                store,
                &task,
                &format!("T-{number} failed: {reason}."),
                &[&requester.handle],
            )?;
        }
        // What the requester is told of a failure is one note, the failure first: it is given
        // its notes one at a time, the oldest first, so a note of what waits for the task
        // written before the task's own reached it as a consequence with its cause unknown,
        // and the cause reached it a turn later, as news it had dealt with (the field board of
        // 2026-11-02: m-6900 "T-303 waits for T-302, which failed", m-6901 "T-302 failed",
        // written a millisecond apart). The tasks that wait for it and were asked for by the
        // same requester are named in that note; the others' requesters are told after it, each
        // in a note of its own.
        if given_up {
            let words = failed(told.waiting_for_the_reopen(None));
            let waiting = waiting_for_it(store, &task, &requester.handle)?;
            send(
                store,
                project_id,
                &Sent {
                    to: &requester.handle,
                    task: Some(number),
                    kind: "note",
                    body: &format!("{words}{waiting}"),
                    ..Sent::default()
                },
            )?;
        }
        tell_other_waiters(store, &task, given_up.then_some(requester.handle.as_str()))?;
        task_by_id(store, task.id)
    })
}

/// The human takes finished tasks off the board for good: each leaves the
/// board and `cf task list`, and keeps its row, so its number is never
/// given again, the threads that name it stay whole and `cf task get` still
/// reads it. Only a finished task goes; all of them go, or none. Nobody is
/// told: the human tidies the board.
///
/// What waits for a task reads it as before once it is gone (nothing that
/// decides a wait looks at `deleted_at`: `release_ready` and the waiter's
/// `needs` take its state), so a task another needs goes too when its state
/// cannot change: an accepted one is accepted for good, a cancelled one never
/// will be. A failed one stays while a task not yet finished needs it: the
/// chief can reopen it, and a deleted task is not reopened (`require_on_board`),
/// so deleting it would leave that task waiting for good.
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
                let needing = if task.state == "failed" {
                    store
                        .db
                        .prepare(&format!(
                            "SELECT w.number FROM task_need n JOIN task w ON w.id = n.task_id
           WHERE n.needs_id = ? AND w.state NOT IN ({finished}) ORDER BY w.number"
                        ))?
                        .query_map([task.id], |row| {
                            Ok(format!("T-{}", row.get::<_, i64>("number")?))
                        })?
                        .collect::<rusqlite::Result<Vec<_>>>()?
                } else {
                    Vec::new()
                };
                if let [needed_by] = needing.as_slice() {
                    return Err(LedgerError::refused_with(
                        "task-needed",
                        format!(
                            "{needed_by} still needs T-{number}, which failed and may be reopened: it stays on the board until {needed_by} is finished"
                        ),
                        409,
                    ));
                }
                if !needing.is_empty() {
                    return Err(LedgerError::refused_with(
                        "task-needed",
                        format!(
                            "{} still need T-{number}, which failed and may be reopened: it stays on the board until they are finished",
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

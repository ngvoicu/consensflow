//! Messages: notes, questions and their answers, delivered into a window one at
//! a time and oldest first, except the human's, which wait in the app until
//! read (`delivery`), or taken back out of a window that had them
//! (`taken_back`); what a window keeps, which rides in the paste of the
//! words that send it on (`carrying`); what makes a message received, and what
//! that does to its task (`receipt`); the human's gate, which holds one agent's
//! word to another until the human passes it on or declines it (`gate`); the
//! one asked, told to answer again when an answer did not stand (`again`); and
//! the note that tells a requester its tasks stalled, one for the tasks of a
//! pass, which goes when they are resumed (`pauses`); the note that gives it the
//! last complete answer of a paused task's window, which a resume does not take
//! back (`paused_answers`); the note that tells
//! it a task is held for its member's quota, and that it goes on (`holds`); the
//! note that says a conversation its role again, which the ledger knows for one
//! (`roles`); and the words told to a task's window before the window has it,
//! kept for it (`told`).

mod again;
mod carrying;
mod continuations;
mod delivery;
mod gate;
mod holds;
mod paused_answers;
mod pauses;
mod receipt;
mod roles;
mod taken_back;
mod told;

use cf_proto::ledger::{MessageView, Question};
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};

use crate::model::{self, require_active, LedgerError, FINISHED_TASK_STATES, MAX_BODY};
use crate::questions::{render_choices, render_questions, require_choices, require_questions};
use crate::queue::{queue, send, withdraw, Queued, Sent};
use crate::store::Store;
use crate::tasks::{pause_task_naming, paused_by_consensflow, take_over_pause};
use crate::views::{message_view, TaskRow, MESSAGE_SELECT};

/// The durable withdrawal reason of a question whose task left its window.
pub const MOVED_QUESTION: &str = "the task moved to another window";
pub(crate) const RESTARTED_QUESTION: &str = "ConsensFlow restarted";
pub(crate) const CLOSED_PROJECT_QUESTION: &str = "the project closed";

pub(crate) use again::tell_sent_back;
pub(crate) use carrying::{
    adopt, fold, reconcile_released, release_carried, release_stranded, transfer,
};
pub use continuations::{
    continuation, continuation_in, continuation_tasks, cut_turn, cut_turn_unquoted,
};
pub(crate) use continuations::{withdraw_continuations, withdraw_stale};
pub(crate) use continuations::{withdraw_each, withdraw_queued_note};
#[cfg(feature = "test-support")]
pub(crate) use delivery::forget_receipt;
pub use delivery::GivenUp;
pub(crate) use delivery::{
    admit, begin_delivery, cancel_message, confirm_delivery, confirm_delivery_telling,
    confirm_sent_by_hand, confirm_unproven, delivery_begun_at, delivery_begun_ms, fail_delivery,
    first_received, give_up_delivery, given_back_because, handed_over, in_flight, last_given,
    last_pasted, last_pastes, next_delivery, next_delivery_of, note_since, paste_into, pasted_into,
    received_of, retry_delivery, with_work,
};
pub use delivery::{HandOver, WITHDRAWN_AFTER_HANDED};
pub(crate) use gate::{approve_message, decline_message, mark_read};
pub use holds::is_busy_note;
pub(crate) use holds::{
    note_hold, note_hold_busy, note_hold_moved, note_hold_unavailable, said as hold_said,
    tell_goes_on,
};
pub(crate) use paused_answers::{
    answer_to_current_pause, given as paused_answer_given, note as note_paused_answer,
    put_back as put_back_paused_answers, queued as queued_paused_answers,
    recover_after_lost_answer, tell_on_its_way, tell_pause_ways_on,
    withdraw as withdraw_paused_answers,
};
pub(crate) use pauses::{
    join_pause_note, leave_pause_notes, note_held_for_context, note_pause, note_stall, say_again,
    warnings_on_their_way, warnings_queued, Afterwards,
};
pub use pauses::{MAY_STILL_RUN, NOT_PROVEN_GONE};
pub(crate) use receipt::{
    claim_answer, door_born_closed, open_questions, questions_to_report, receive_read,
    receive_read_part, reconcile, reconcile_answered_on_open, release_all_claims, release_claims,
    settle_claim,
};
pub use receipt::{Read, TURN_ON_OPEN_QUESTION};
pub(crate) use roles::{note_role, role_notes_to, role_of_note};
pub(crate) use taken_back::take_back;
pub(crate) use told::{
    drop_queued_but_told, kept_through_leaving, launch_called_off, notes_waiting,
    notes_waiting_for_finish, sent_at_work_of, tell_queued, told_unreceived, withdraw_at_work,
    Dropped,
};
pub use told::{is_told, sent_at_work, told_at_work, waiting_for_the_reopen};

/// A note: from a participant (ConsensFlow itself when none) to another,
/// about one of the project's tasks or none.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct NewNote {
    pub from: Option<String>,
    pub to: String,
    pub body: String,
    pub task: Option<i64>,
}

/// A note of ConsensFlow's own that asks its reader no decision: to a chief it is shown,
/// not typed (`queue`'s [`crate::queue::SHOWN`]), `aside` its words on the task's line;
/// to anyone else it is queued as [`note`] queues one.
pub(crate) fn show(
    store: &mut Store,
    project_id: i64,
    note: &NewNote,
    aside: &str,
) -> Result<MessageView, LedgerError> {
    send(
        store,
        project_id,
        &Sent {
            from: note.from.as_deref(),
            to: &note.to,
            body: &note.body,
            task: note.task,
            kind: "note",
            shown: Some(aside),
            ..Sent::default()
        },
    )
}

/// A question for a coordinator: who asks it and whom, about which task,
/// and what it asks, in words or as a harness's question tool asked it, with
/// options; an urgent one is the chief's `cf tell` to a task's window.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct NewQuestion {
    pub from: Option<String>,
    pub to: String,
    pub body: Option<String>,
    pub task: Option<i64>,
    pub questions: Option<Value>,
    pub urgent: bool,
}

pub(crate) fn note(
    store: &mut Store,
    project_id: i64,
    note: &NewNote,
) -> Result<MessageView, LedgerError> {
    send(
        store,
        project_id,
        &Sent {
            from: note.from.as_deref(),
            to: &note.to,
            body: &note.body,
            task: note.task,
            kind: "note",
            ..Sent::default()
        },
    )
}

/// A note that reports the ended turn `turn` of a window to its task's requester (T-2197):
/// as [`note`], and its `message.sent` event carries `report: turn`, which is how the ledger
/// knows the turn was told (`questions_to_report`).
pub(crate) fn report_note(
    store: &mut Store,
    project_id: i64,
    note: &NewNote,
    turn: &str,
) -> Result<MessageView, LedgerError> {
    send(
        store,
        project_id,
        &Sent {
            from: note.from.as_deref(),
            to: &note.to,
            body: &note.body,
            task: note.task,
            kind: "note",
            report: Some(turn),
            ..Sent::default()
        },
    )
}

/// A question for a coordinator; the asker's task waits for the answer. The
/// human is never asked on the board: the chief asks them in its own
/// terminal, where they work with it. With questions, the question carries
/// options as a harness's own question tool asked them, and its text is
/// rendered from them. An urgent question is the chief's `cf tell` to a
/// task's window: the task is paused for it, and the question says so.
pub(crate) fn ask(
    store: &mut Store,
    project_id: i64,
    question: &NewQuestion,
) -> Result<MessageView, LedgerError> {
    Ok(ask_naming(store, project_id, question)?.0)
}

/// [`ask`], and with the question the told notes its pause cancelled (an urgent question
/// pauses its task, and the pause takes back what the chief wrote for the window).
pub(crate) fn ask_naming(
    store: &mut Store,
    project_id: i64,
    question: &NewQuestion,
) -> Result<(MessageView, Vec<MessageView>), LedgerError> {
    let Some(from) = question.from.as_deref() else {
        return Err(LedgerError::refused(
            "unknown-participant",
            "a question names who asks it",
        ));
    };
    let options = question
        .questions
        .as_ref()
        .map(require_questions)
        .transpose()?;
    store.write(|store| {
        if store.participant_by_handle(project_id, &question.to)?.role == "human" {
            return Err(LedgerError::refused_with(
                "ask-in-your-terminal",
                "the human is asked in the chief's terminal, not on the board",
                403,
            ));
        }
        // The pause first, as one step with the tell: what it withdraws from
        // the window is never the tell, and a tell refused below pauses nothing.
        let mut withdrawn = Vec::new();
        if question.urgent {
            let Some(number) = question.task else {
                return Err(LedgerError::refused_with(
                    "unknown-task",
                    format!("no task T-undefined in project {project_id}"),
                    404,
                ));
            };
            // A task paused already is paused for the tell all the same: one its
            // project's suspension paused is the teller's from now on.
            // A task ConsensFlow paused on its own (held, stalled) is the teller's now: its
            // stop is asked, and it goes on only by a resume (T-1220's P1-1).
            let task = store.task_row(project_id, number)?;
            if task.state == "paused" && !paused_by_consensflow(store, &task)? {
                take_over_pause(store, &task, from)?;
            } else {
                withdrawn = pause_task_naming(store, project_id, number, Some(from), None)?.1;
            }
        }
        let body = match &options {
            Some(options) => render_questions(options),
            None => question.body.clone().unwrap_or_default(),
        };
        // The window that asks about its task, and the task as it is now.
        let asking = match question.task {
            Some(number) => {
                let row = store.task_row(project_id, number)?;
                let asker = store.participant_by_handle(project_id, from)?;
                // The named task may be the topic of a question from a later
                // holding. Only the last delivered holding decides where its
                // answer goes, including when the topic is an earlier task.
                let holding = crate::holding::of(store, asker.id)?;
                let other_holding = match holding {
                    Some(h) if h.active && h.task_id != row.id => crate::holding::matches(
                        store,
                        asker.id,
                        Some(h.task_id),
                        Some(h.first_brief),
                    )?
                    .then_some(h),
                    _ => None,
                };
                if !question.urgent && other_holding.is_none() {
                    if let Some(held) = holding.filter(|h| h.task_id == row.id) {
                        if !crate::holding::matches(
                            store,
                            asker.id,
                            Some(held.task_id),
                            Some(held.first_brief),
                        )? {
                            if row.assignee_id == Some(asker.id)
                                && matches!(row.state.as_str(), "accepted" | "cancelled" | "failed")
                            {
                                return Err(LedgerError::refused_with(
                                    "task-ended",
                                    format!(
                                        "T-{number} is {}: this window cannot ask on it now",
                                        row.state
                                    ),
                                    409,
                                ));
                            }
                            return Err(LedgerError::refused_with(
                                "question-moved",
                                format!(
                                    "T-{number} {}: this window cannot ask on it now",
                                    moved_task_location(store, &row)?
                                ),
                                409,
                            ));
                        }
                    }
                }
                if let Some(holding) = other_holding.filter(|_| !question.urgent) {
                    Some(store.task_row(project_id, holding.task_number)?)
                } else {
                    (row.assignee_id == Some(asker.id) && !question.urgent).then_some(row)
                }
            }
            None => None,
        };
        let shut = match &asking {
            Some(row) => door_born_closed(store, row)?,
            None => false,
        };
        let message = send(
            store,
            project_id,
            &Sent {
                from: Some(from),
                to: &question.to,
                body: &body,
                task: question.task,
                kind: "question",
                questions: options.as_deref(),
                urgent: question.urgent,
                door_closed: shut,
                ..Sent::default()
            },
        )?;
        if !question.urgent {
            let asker = store.participant_by_handle(project_id, from)?;
            if !matches!(asker.role.as_str(), "chief" | "human") {
                crate::holding::bind(store, message.id, asker.id)?;
            }
        }
        if let Some(row) = asking {
            reconcile(store, row.id)?;
        }
        Ok((message, withdrawn))
    })
}

/// The answer goes back to whoever asked; the one asked answers. A plain
/// question's answer is delivered into the asker's window. A question with
/// options is answered by choice (or by text: one line per question of an ask
/// of several, any text for an ask of one, `require_choices`): its harness
/// door can claim and receive the answer. An answer on its way ends the task's
/// wait, but still reaches the window as before. A question on a task that is over (accepted, cancelled
/// or failed) takes no answer, from either side: nobody waits for it. A
/// question answered is never typed in after (`settle_answered`). `from` is the answerer's participant
/// id: handles repeat across projects (every chief is `chief`), ids never do.
pub(crate) fn answer(
    store: &mut Store,
    question_id: i64,
    from: i64,
    body: Option<&Value>,
    choices: Option<&Value>,
) -> Result<MessageView, LedgerError> {
    store.write(|store| {
        let question = store
            .message(question_id)?
            .filter(|question| question.kind == "question")
            .ok_or_else(|| {
                LedgerError::refused_with(
                    "not-a-question",
                    format!("message {question_id} is not a question"),
                    409,
                )
            })?;
        let answerer = store.participant_row(from)?;
        let (asker_id, mentioned_task): (Option<i64>, Option<i64>) = store.db.query_row(
            "SELECT sender_id, task_id FROM message WHERE id = ?",
            [question_id],
            |row| Ok((row.get("sender_id")?, row.get("task_id")?)),
        )?;
        let binding = crate::holding::binding_task(store, question_id)?;
        let task_id = binding.unwrap_or(mentioned_task);
        // The one asked, or (a question with options) the asker itself: its
        // window may have answered first, and the board's copy takes that answer.
        let from_window = !question.questions.is_null() && Some(answerer.id) == asker_id;
        if answerer.id != question.recipient_id && !from_window {
            return Err(LedgerError::refused_with(
                "not-your-question",
                format!(
                    "the question was put to {}, not {}",
                    question.recipient, answerer.handle
                ),
                403,
            ));
        }
        // A question its own task's result closed says what reaches the window now, before
        // the refusals about the window it was asked in.
        if question.state == "cancelled" {
            let task = match binding {
                Some(_) => crate::holding::task_of_question(store, question_id)?,
                None => message_task(store, question_id)?,
            };
            if let Some(task) = task
                .filter(|task| question.reason.as_deref() == Some(&gave_result_reason(task.number)))
            {
                return Err(closed_by_the_result(question_id, &task));
            }
        }
        if crate::holding::awaiting_handover(store, question_id)? {
            return Err(LedgerError::refused_with("handover-pending", "This window is receiving another brief. Wait for the send to finish, then try again.", 409));
        }
        if crate::holding::question_is_live(store, question_id)? == Some(false) {
            if let Some(task) = crate::holding::task_of_question(store, question_id)? {
                if FINISHED_TASK_STATES.contains(&task.state.as_str()) {
                    return Err(finished_task_takes_no_answer(store, &task)?);
                }
                return Err(moved_task_takes_no_answer(store, &task)?);
            }
            return Err(LedgerError::refused_with("question-window-closed", "the window that asked this question has ended; start a new task with what you meant to answer", 409));
        }
        let answer_task = match binding {
            Some(_) => crate::holding::task_of_question(store, question_id)?,
            None => message_task(store, question_id)?,
        };
        if let Some(task) = answer_task {
            if binding.is_some() && question.reason.as_deref() == Some(MOVED_QUESTION) {
                return Err(moved_task_takes_no_answer(store, &task)?);
            }
            if FINISHED_TASK_STATES.contains(&task.state.as_str()) {
                return Err(if binding.is_some() {
                    finished_task_takes_no_answer(store, &task)?
                } else {
                    legacy_finished_task_takes_no_answer(&task)
                });
            }
        }
        if answered(store, question_id)? {
            return Err(LedgerError::refused_with(
                "already-answered",
                format!("m-{question_id} has its answer"),
                409,
            ));
        }
        // A question the ledger withdrew with no answer ever given (its task was finished
        // while it was open) is answered by no one: its words, if the chief took them, are the
        // result. One whose answer failed or was cancelled still obliges, and takes another.
        let finished_task = if binding.is_some() {
            crate::holding::task_of_question(store, question_id)?
        } else {
            None
        };
        let finished = finished_task.is_some_and(|task| {
            question.reason.as_deref() == Some(&format!("T-{} was finished", task.number))
        });
        if question.state == "cancelled" && (finished || !has_any_answer(store, question_id)?) {
            return Err(LedgerError::refused_with(
                "question-withdrawn",
                format!(
                    "m-{question_id} was withdrawn{}: nobody waits for this answer",
                    question
                        .reason
                        .as_deref()
                        .map_or_else(String::new, |reason| format!(" ({reason})"))
                ),
                409,
            ));
        }
        let asked: Option<Vec<Question>> = match &question.questions {
            Value::Null => None,
            questions => Some(serde_json::from_value(questions.clone())?),
        };
        let picks = asked
            .as_deref()
            .map(|asked| require_choices(asked, choices, body))
            .transpose()?;
        let text = match (&asked, &picks) {
            (Some(asked), Some(picks)) => render_choices(asked, picks),
            _ => body.and_then(Value::as_str).unwrap_or_default().to_string(),
        };
        model::require_text(&text, "body", MAX_BODY)?;
        require_active(&answerer)?;
        let asker = store.participant_of(asker_id)?;
        require_active(&asker)?;
        let id = queue(
            store,
            question.project_id,
            &Queued {
                to: asker.id,
                from: Some(answerer.id),
                kind: "answer",
                task_id,
                reply_to: Some(question_id),
                body: &text,
                from_window,
                choices: picks.as_deref(),
                ..Queued::default()
            },
        )?;
        store.log(
            question.project_id,
            "message.sent",
            json!({ "message": id, "kind": "answer", "from": answerer.handle, "to": question.sender }),
        )?;
        // If the window's turn was observed first, its note may still be waiting
        // for the chief. The answer and the change of that note are one write.
        if let Some(task) = message_task(store, id)? {
            paused_answers::show_answered(store, &task, question_id, false)?;
        }
        // A question still gated is answered before the one asked saw it: it goes
        // no further. So is one still queued that the asker's own window answered.
        // One still queued that the one asked answered is theirs: they answered it.
        if question.state == "gated" || (question.state == "queued" && from_window) {
            withdraw(store, question_id, &format!("answered by @{}", answerer.handle))?;
        }
        settle_answered(store, question_id)?;
        crate::waiting_questions::end(store, question_id)?;
        let answer = known_message(store, id)?;
        // An answer on its way ends the wait; a gated one still waits for the human.
        if let Some(task_id) = task_id {
            reconcile(store, task_id)?;
        }
        Ok(answer)
    })
}

/// The task may have moved again since the question was withdrawn. Say where it is now,
/// not where it first went, and direct the answerer's words to the task itself.
fn moved_task_takes_no_answer(store: &Store, task: &TaskRow) -> Result<LedgerError, LedgerError> {
    let number = task.number;
    let place = moved_task_location(store, task)?;
    let holder_gone = match task.assignee_id {
        Some(id) => {
            let holder = store.participant_row(id)?;
            holder.left_at.is_some()
                || holder.member_id.is_some_and(|member| {
                    store
                        .participant_row(member)
                        .is_ok_and(|p| p.left_at.is_some())
                })
        }
        None => false,
    };
    let tell = if task.deleted_at.is_some() {
        "This task was deleted. Start a new task with what you meant to answer, or use cf tell on the new task.".to_owned()
    } else if holder_gone {
        "Its former window has ended. Start a new task with what you meant to answer.".to_owned()
    } else {
        match task.state.as_str() {
        "done" | "failed" => "Start a new task with what you meant to answer.".to_owned(),
        "accepted" | "cancelled" => "This task cannot receive a tell. Start a new task with what you meant to answer, or use cf tell on the new task.".to_owned(),
        _ => format!("Use cf tell T-{number} \"…\" for what you meant to answer."),
    }
    };
    Ok(LedgerError::refused_with(
        "question-moved",
        format!("T-{number} {place}. {tell}"),
        409,
    ))
}

/// The current location, shared with notices about undelivered answers.
pub(crate) fn moved_task_location(store: &Store, task: &TaskRow) -> Result<String, LedgerError> {
    Ok(if matches!(task.state.as_str(), "accepted" | "cancelled") {
        // The old assignee is still recorded, but no longer holds the task.
        format!("is {}", task.state)
    } else if let Some(window) = task.assignee_id {
        let window = store.participant_row(window)?;
        let member = match window.member_id {
            Some(id) => store.participant_row(id)?,
            None => window,
        };
        if let Some(owner) = member.backup_of {
            let owner = store.participant_row(owner)?;
            format!(
                "is {} with backup @{} of @{}",
                task.state, member.handle, owner.handle
            )
        } else {
            format!("is {} with @{}", task.state, member.handle)
        }
    } else {
        match task.pool.as_deref() {
            Some(pool) => {
                let name = crate::roles::pool_name(Some(pool), task.tier.as_deref());
                let tier = if task.tier.is_some() { " tier" } else { "" };
                format!("is {} for its {name}{tier}", task.state)
            }
            None => format!("is {} and has no member", task.state),
        }
    })
}

/// The refusal of an answer to a question on a task that is over (accepted,
/// cancelled or failed): nobody waits for it, and a failed task is the one that
/// can be gone on with, so it is told how.
fn finished_task_takes_no_answer(
    store: &Store,
    task: &TaskRow,
) -> Result<LedgerError, LedgerError> {
    if task.deleted_at.is_some()
        || task
            .assignee_id
            .is_some_and(|id| store.participant_row(id).is_ok_and(|p| p.left_at.is_some()))
    {
        return moved_task_takes_no_answer(store, task);
    }
    let number = task.number;
    let (code, words) = match task.state.as_str() {
        "cancelled" => (
            "task-cancelled",
            format!("T-{number} is cancelled: nobody waits for this answer. Start a new task with what you meant to answer."),
        ),
        "accepted" => (
            "task-accepted",
            format!(
                "T-{number} is accepted, so nobody waits for this answer. Start a new task with what you meant to answer."
            ),
        ),
        _ => (
            "task-failed",
            format!(
                "T-{number} failed, so nobody waits for this answer. Start a new task with what you meant to answer."
            ),
        ),
    };
    Ok(LedgerError::refused_with(code, words, 409))
}

/// Why a question that its task's result closed was withdrawn (T-2197).
pub(crate) fn gave_result_reason(number: i64) -> String {
    format!("T-{number} gave its result with it unanswered")
}

/// The refusal of an answer to a question its task's result closed: what sends words to its
/// window as the task stands now.
fn closed_by_the_result(question: i64, task: &TaskRow) -> LedgerError {
    let number = task.number;
    let way = match task.state.as_str() {
        "done" => format!(
            "To send words to its window, reopen the task: `cf task reopen T-{number} \"…\"`."
        ),
        "accepted" => format!(
            "To send words to its window, add a follow-up: `cf task add --after T-{number} \"…\"`."
        ),
        "queued" | "working" | "waiting" | "paused" => {
            format!("To send words to its window: `cf tell T-{number} \"…\"`.")
        }
        _ => "Start a new task with what you meant to answer.".to_owned(),
    };
    LedgerError::refused_with(
        "question-withdrawn",
        format!("m-{question} is closed: T-{number} gave its result with it unanswered. {way}"),
        409,
    )
}

/// Closes the questions `ids` of the task its result closed (T-2197): withdrawn with why,
/// whatever tracked them, their doors shut and their holding closed, so no answer is taken
/// for any, whether the window stays open or not.
pub(crate) fn close_by_the_result(
    store: &Store,
    task: &TaskRow,
    ids: &[i64],
) -> Result<(), LedgerError> {
    let reason = gave_result_reason(task.number);
    for id in ids {
        store.db.execute(
            "UPDATE message SET state = 'cancelled', reason = ?1,
                 door_closed_at = COALESCE(door_closed_at, (SELECT updated_at FROM task WHERE id = ?2))
             WHERE id = ?3 AND kind = 'question'",
            params![reason, task.id, id],
        )?;
        store.db.execute(
            "UPDATE question_binding SET closed = 1 WHERE question_id = ? AND closed = 0",
            [id],
        )?;
    }
    Ok(())
}

/// A question recorded before holding tracking keeps the earlier refusal.
fn legacy_finished_task_takes_no_answer(task: &TaskRow) -> LedgerError {
    let number = task.number;
    let (code, words) = match task.state.as_str() {
        "cancelled" => (
            "task-cancelled",
            format!("T-{number} is cancelled: nobody waits for this answer"),
        ),
        "accepted" => (
            "task-accepted",
            format!("T-{number} is accepted, so it is finished and nobody waits for this answer: to give what is still wanted, add a task for the same window with cf task add --after T-{number} \"…\""),
        ),
        _ => (
            "task-failed",
            format!("T-{number} failed, so nobody waits for this answer: reopen T-{number} to go on with it (cf task reopen T-{number} \"…\")"),
        ),
    };
    LedgerError::refused_with(code, words, 409)
}

/// The questions a task's window asked, still open, are withdrawn when the task is finished
/// while it waits for their answers (the chief's Finish):
/// nobody waits for them now. A question not yet given to the chief is never given, and one
/// the chief has can no longer be answered (`answer` refuses it). One with an answer that
/// stands is the answerer's, and an urgent one is the chief's tell, not the window's.
pub(crate) fn withdraw_open_questions(
    store: &Store,
    task: &TaskRow,
    why: &str,
) -> Result<(), LedgerError> {
    let Some(window) = task.assignee_id else {
        return Ok(());
    };
    if why == "was finished" {
        store.db.execute(
            &format!(
                "UPDATE message SET state = 'cancelled', reason = ?1
                 WHERE task_id = ?2 AND sender_id = ?3 AND kind = 'question' AND urgent = 0
                   AND id NOT IN (SELECT question_id FROM question_binding)
                   AND state IN ('queued', 'delivering', 'gated', 'delivered', 'read')
                   AND NOT {}",
                has_standing_answer("message")
            ),
            params![format!("T-{} {why}", task.number), task.id, window],
        )?;
    }
    store.db.execute(
        &format!(
            "UPDATE message SET state = 'cancelled', reason = ?1
              WHERE {} = ?2 AND sender_id = ?3 AND kind = 'question' AND urgent = 0
                AND id IN (SELECT question_id FROM question_binding)
                AND (state IN ('queued', 'delivering', 'gated', 'delivered', 'read', 'failed')
                     OR (state = 'cancelled' AND reason IS NOT ?4 AND reason IS NOT ?5 AND reason IS NOT ?6\n                     AND reason IS NOT ('T-' || (SELECT number FROM task WHERE id = ?2) || ' gave its result with it unanswered') AND EXISTS (
                         SELECT 1 FROM message a WHERE a.reply_to = message.id AND a.kind = 'answer')))
                AND NOT {}",
            crate::holding::task_for_question_sql("message"),
            has_standing_answer("message")
        ),
        params![format!("T-{} {why}", task.number), task.id, window, MOVED_QUESTION, RESTARTED_QUESTION, CLOSED_PROJECT_QUESTION],
    )?;
    Ok(())
}

/// SQL that says the question called `question` (a table alias) has an answer
/// that stands: one on its way, being delivered, delivered, read, or still held
/// at the gate. One that was cancelled (declined, withdrawn, taken back with its
/// task) or that failed to arrive is none: the question takes another. The one
/// rule by which a second answer is refused (`answered`), by which the
/// chief's handoff lists what it has yet to answer (`chief_open_work`), and by
/// which a question is never typed in once answered (`settle_answered`).
pub(crate) fn has_standing_answer(question: &str) -> String {
    format!("EXISTS ({})", standing_answers(question))
}

/// SQL that finds the ids of the answers that stand ([`has_standing_answer`])
/// to the question called `question`.
fn standing_answers(question: &str) -> String {
    format!(
        "SELECT a.id FROM message a WHERE a.reply_to = {question}.id AND a.kind = 'answer'
                   AND a.state NOT IN ('cancelled', 'failed')"
    )
}

/// Whether a question has any answer, whatever became of it.
fn has_any_answer(store: &Store, question_id: i64) -> Result<bool, LedgerError> {
    Ok(store
        .db
        .prepare("SELECT 1 FROM message WHERE reply_to = ? AND kind = 'answer'")?
        .exists([question_id])?)
}

/// Whether a question has an answer that stands ([`has_standing_answer`]).
fn answered(store: &Store, question_id: i64) -> Result<bool, LedgerError> {
    Ok(store
        .db
        .prepare(&format!(
            "SELECT 1 FROM message q WHERE q.id = ? AND {}",
            has_standing_answer("q")
        ))?
        .exists([question_id])?)
}

/// A question that has an answer standing ([`has_standing_answer`]) is never
/// typed into its recipient's window, however it was read (T-260): the one
/// asked answered it, so it has it. Still `queued`, it is `read`, its receipt
/// naming the answer, read when the answer was written, which is when the one
/// asked had it: answered as it waited, or put back in the queue by a paste that
/// was not proved after it was answered. Any other question, or one in any other
/// state, is left as it is.
///
/// - No event is logged of it: the row says it, its receipt `{"answered": id}`
///   and its `delivered_at`, the answer's `created_at`; the answer's own
///   `message.sent` is the event, as it is of a question withdrawn at the gate
///   by its answer, and the clock is not read again (which kept Node's
///   recordings of the answer in place).
/// - A question answered while its paste is in flight (`delivering`) is the
///   paste's: confirmed, it is `delivered`, typed in with its "Run in your
///   shell: cf answer m-…" line, and `cf answer` then refuses it
///   (`already-answered`). Retried, it is settled here.
pub(super) fn settle_answered(store: &mut Store, question_id: i64) -> Result<(), LedgerError> {
    let question = known_message(store, question_id)?;
    if question.kind != "question" || question.state != "queued" {
        return Ok(());
    }
    let answer: Option<(i64, String)> = store
        .db
        .query_row(
            &format!(
                "SELECT id, created_at FROM message WHERE id = (
                   SELECT ({} ORDER BY a.id LIMIT 1) FROM message q WHERE q.id = ?)",
                standing_answers("q")
            ),
            [question_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    if let Some((answer, at)) = answer {
        store.db.execute(
            "UPDATE message SET state = 'read', delivered_at = ?, receipt = ? WHERE id = ?",
            params![at, json!({ "answered": answer }).to_string(), question_id],
        )?;
    }
    Ok(())
}

/// What is on its way to a participant (queued, or being delivered), oldest first.
pub(crate) fn pending(store: &Store, participant_id: i64) -> Result<Vec<MessageView>, LedgerError> {
    messages(
        store,
        &format!(
            "{MESSAGE_SELECT} WHERE m.recipient_id = ? AND m.state IN ('queued', 'delivering')
       ORDER BY m.id"
        ),
        params![participant_id],
    )
}

/// A participant's messages, newest first, at most `limit`, whole: what
/// reached it or is on its way, never what still waits for the human. `cf
/// inbox` lists them over the local API.
pub(crate) fn inbox(
    store: &Store,
    participant_id: i64,
    limit: i64,
) -> Result<Vec<MessageView>, LedgerError> {
    messages(
        store,
        &format!(
            "{MESSAGE_SELECT} WHERE m.recipient_id = ? AND m.state != 'gated' ORDER BY m.id DESC LIMIT ?"
        ),
        params![participant_id, limit],
    )
}

/// The newest note ConsensFlow itself left the participant (one with no sender) whose
/// body opens with `opening`; none where it left none.
pub(crate) fn newest_own_note(
    store: &Store,
    participant_id: i64,
    opening: &str,
) -> Result<Option<MessageView>, LedgerError> {
    let length = i64::try_from(opening.chars().count()).unwrap_or(i64::MAX);
    Ok(messages(
        store,
        &format!(
            "{MESSAGE_SELECT} WHERE m.recipient_id = ? AND m.sender_id IS NULL AND m.kind = 'note' \
             AND substr(m.body, 1, ?) = ? ORDER BY m.id DESC LIMIT 1"
        ),
        params![participant_id, length, opening],
    )?
    .pop())
}

/// The messages a `MESSAGE_SELECT` finds.
fn messages(
    store: &Store,
    sql: &str,
    values: impl rusqlite::Params,
) -> Result<Vec<MessageView>, LedgerError> {
    Ok(store
        .db
        .prepare(sql)?
        .query_map(values, message_view)?
        .collect::<rusqlite::Result<Vec<_>>>()?)
}

/// A message the operation has just found or made.
fn known_message(store: &Store, id: i64) -> Result<MessageView, LedgerError> {
    store.message(id)?.ok_or_else(|| {
        LedgerError::refused_with("unknown-message", format!("no message {id}"), 404)
    })
}

/// A message in `state`, or a refusal saying which state it is in.
fn require_message(store: &Store, id: i64, state: &str) -> Result<MessageView, LedgerError> {
    let message = known_message(store, id)?;
    if message.state != state {
        return Err(LedgerError::refused_with(
            "invalid-transition",
            format!("message {id} is {}, not {state}", message.state),
            409,
        ));
    }
    Ok(message)
}

/// The task a message is about, if any.
fn message_task(store: &Store, message_id: i64) -> Result<Option<TaskRow>, LedgerError> {
    Ok(store
        .db
        .query_row(
            "SELECT t.* FROM task t JOIN message m ON m.task_id = t.id WHERE m.id = ?",
            [message_id],
            TaskRow::read,
        )
        .optional()?)
}

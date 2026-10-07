//! An answer that did not stand, and the one asked told to answer again. A
//! question's task waits until an answer to it is received, and an answer
//! that was declined, did not arrive or was withdrawn is none: the task waits
//! for another, which only `cf answer m-q` gives, and nobody else knows it is
//! owed. So whoever was asked is told, in the words the decline of an answer
//! has always used.

use std::fmt::Display;

use cf_proto::ledger::MessageView;
use rusqlite::params;

use crate::model::LedgerError;
use crate::queue::{send, Sent};
use crate::store::Store;
use crate::views::TaskRow;

/// What an answerer is told to do once its answer did not stand: the question
/// is open still, and is answered again.
pub(super) fn answer_again(question: impl Display) -> String {
    format!("Answer it again: cf answer m-{question} \"…\"")
}

/// The questions the window asked on `task` that wait to be answered again:
/// each had an answer that did not stand (it was declined, withdrawn or
/// failed) and has none on its way or received, so the task waits for it until
/// the one asked answers once more. The question's id, and the id of whoever
/// it was put to.
fn to_answer_again(store: &Store, task: i64, window: i64) -> Result<Vec<(i64, i64)>, LedgerError> {
    Ok(store
        .db
        .prepare(
            "SELECT q.id, q.recipient_id FROM message q
             WHERE q.task_id = ?1 AND q.kind = 'question' AND q.sender_id = ?2 AND q.urgent = 0
               AND EXISTS (SELECT 1 FROM message a WHERE a.reply_to = q.id AND a.kind = 'answer'
                           AND a.state IN ('cancelled', 'failed'))
               AND NOT EXISTS (SELECT 1 FROM message a WHERE a.reply_to = q.id AND a.kind = 'answer'
                               AND a.state IN ('gated', 'queued', 'delivering', 'delivered', 'read'))
             ORDER BY q.id",
        )?
        .query_map(params![task, window], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?)
}

/// A note from ConsensFlow to whoever was asked `question`, on task `task`
/// (its number): `lead` says what happened, and the words that follow what to
/// do. One who has left the project is not told: nothing waits on them.
fn tell(
    store: &mut Store,
    project_id: i64,
    asked: i64,
    task: i64,
    lead: &str,
    question: i64,
) -> Result<(), LedgerError> {
    let asked = store.participant_row(asked)?;
    if asked.left_at.is_some() {
        return Ok(());
    }
    send(
        store,
        project_id,
        &Sent {
            to: &asked.handle,
            task: Some(task),
            kind: "note",
            body: &format!("{lead} {}", answer_again(question)),
            ..Sent::default()
        },
    )?;
    Ok(())
}

/// A delivery of `answer` was given up for good, because `why`: the task it
/// was for waits for another, unless it is over, and whoever was asked is told.
pub(super) fn tell_failed_answer(
    store: &mut Store,
    answer: &MessageView,
    task: &TaskRow,
    why: &str,
) -> Result<(), LedgerError> {
    let (Some(question), true) = (
        answer.reply_to,
        matches!(
            task.state.as_str(),
            "queued" | "working" | "waiting" | "paused"
        ),
    ) else {
        return Ok(());
    };
    for (waiting, asked) in to_answer_again(store, task.id, answer.recipient_id)? {
        if waiting == question {
            let lead = format!(
                "Your answer m-{} to m-{question} did not reach @{}: {why}.",
                answer.id, answer.recipient
            );
            tell(
                store,
                answer.project_id,
                asked,
                task.number,
                &lead,
                question,
            )?;
        }
    }
    Ok(())
}

/// `task` was sent back to `window` by `by`, and waits for the answers that
/// did not stand, whether they were withdrawn with the words they answered, or
/// cancelled with the task before, or failed: whoever was asked is told.
pub(crate) fn tell_sent_back(
    store: &mut Store,
    task: &TaskRow,
    window: i64,
    by: &str,
) -> Result<(), LedgerError> {
    for (question, asked) in to_answer_again(store, task.id, window)? {
        let lead = format!(
            "T-{} was sent back by @{by}, and m-{question} still waits for an answer: the one it had did not stand.",
            task.number
        );
        tell(store, task.project_id, asked, task.number, &lead, question)?;
    }
    Ok(())
}

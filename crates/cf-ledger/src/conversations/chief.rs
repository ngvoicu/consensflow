//! The chief across a Switch chief: the switch itself, what it was switched
//! from, and what a chief that takes over reads, its history and its open work.

use cf_proto::ledger::{
    ChiefConversation, ChiefOpenWork, LastSwitch, ProjectView, SwitchedFrom, TaskView,
};
use rusqlite::params;
use serde_json::{json, Value};

use super::transcripts::items_of;
use crate::messages::{has_standing_answer, hold_said, is_busy_note};
use crate::model::{self, sql_list, LedgerError, FINISHED_TASK_STATES};
use crate::projects::known_project;
use crate::store::Store;
use crate::views::{conversation_view, message_view, task_view, MESSAGE_SELECT, TASK_SELECT};

/// The human's Switch chief: the harness the chief runs on from now on and the model of it
/// the human picked (CHF-04), and whether the old chief was stopped mid-turn.
#[derive(Debug, Clone, PartialEq)]
pub struct ChiefSwitch {
    pub harness: String,
    pub model: String,
    /// The effort it runs its model at; none for its harness's own.
    pub effort: Option<String>,
    /// What the harness's list calls the model, its id (Devin's `swe-2` for the `swe-2-high` its
    /// launch is given); none where the list was never read.
    pub model_name: Option<String>,
    pub cut: bool,
}

impl ChiefSwitch {
    /// The switch as JSON gives it: its harness, then its model.
    pub fn from_json(value: &Value) -> Result<Self, LedgerError> {
        let harness = model::parse_harness(value.get("harness"))?;
        let model = model::parse_model_id(value.get("model"))?;
        let effort = model::parse_effort(value.get("effort"))?;
        let model_name = model::parse_model_name(value.get("modelName"))?;
        Ok(Self {
            harness: harness.to_string(),
            model,
            effort,
            model_name,
            cut: value.get("cut") == Some(&Value::Bool(true)),
        })
    }
}

/// What the chief said and was told before its current conversation: every
/// earlier conversation of the chief, oldest first, with the harness it ran
/// on and its copied items in order. `cf history` pages it for a chief the
/// human switched in.
pub(crate) fn chief_history(
    store: &Store,
    project_id: i64,
) -> Result<Vec<ChiefConversation>, LedgerError> {
    let chief = store.participant_by_handle(project_id, "chief")?;
    let conversations = store
        .db
        .prepare(
            "SELECT * FROM conversation WHERE participant_id = ? AND ended_at IS NOT NULL ORDER BY id",
        )?
        .query_map([chief.id], conversation_view)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    conversations
        .into_iter()
        .map(|conversation| {
            Ok(ChiefConversation {
                items: items_of(store, conversation.id)?,
                conversation,
            })
        })
        .collect()
}

/// What waits on the chief now, for a chief that takes over: members'
/// questions to it without an answer, results it has not decided on, its
/// own unfinished tasks, and the tasks it gave that are held ([`held_tasks`]). A question waits only while someone waits for
/// its answer: one on a task that is over (accepted, cancelled or failed)
/// does not, whatever became of its answer. Otherwise it waits when it has no
/// answer that stands, by the rule that refuses a second answer
/// ([`has_standing_answer`]): an answer held at the gate answers it, and one
/// that failed to arrive or was cancelled does not.
pub(crate) fn chief_open_work(
    store: &Store,
    project_id: i64,
) -> Result<ChiefOpenWork, LedgerError> {
    let chief = store.participant_by_handle(project_id, "chief")?;
    let finished = sql_list(&FINISHED_TASK_STATES);
    let standing_answer = has_standing_answer("m");
    let questions = store
        .db
        .prepare(&format!(
            "{MESSAGE_SELECT}
         WHERE m.project_id = ? AND m.recipient_id = ? AND m.kind = 'question'
           AND m.state NOT IN ('gated', 'cancelled')
           AND NOT {standing_answer}
           AND NOT EXISTS (
             SELECT 1 FROM task q WHERE q.id = m.task_id AND q.state IN ({finished})
           )
         ORDER BY m.id"
        ))?
        .query_map(params![project_id, chief.id], message_view)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let results = store
        .db
        .prepare(&format!(
            "{TASK_SELECT} WHERE t.project_id = ? AND t.requester_id = ? AND t.state = 'done'
         ORDER BY t.number"
        ))?
        .query_map(params![project_id, chief.id], task_view)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let own = store
        .db
        .prepare(&format!(
            "{TASK_SELECT} WHERE t.project_id = ? AND t.assignee_id = ?
           AND t.state IN ('queued', 'working', 'waiting', 'paused')
         ORDER BY t.number"
        ))?
        .query_map(params![project_id, chief.id], task_view)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let held = held_tasks(store, project_id, chief.id)?;
    Ok(ChiefOpenWork {
        questions,
        results,
        own,
        held,
    })
}

/// The tasks `chief` gave that are held now by a hold it is told of once (out of quota, an
/// account unavailable, a model moved: `holds`), each with what its line says of the hold as
/// its `aside`. The first note of a hold is typed to the chief once and its repeats are shown
/// (RS-25), so a chief switched since, in a conversation of its own, would never be typed it:
/// its handoff names them (T-1500's finding 2). The hold is read from its newest note to the
/// chief; a model busy for a moment is tried again by itself and is not listed.
fn held_tasks(store: &Store, project_id: i64, chief: i64) -> Result<Vec<TaskView>, LedgerError> {
    let tasks = store
        .db
        .prepare(&format!(
            "{TASK_SELECT} WHERE t.project_id = ? AND t.requester_id = ?
           AND t.state = 'paused' AND t.held_until IS NOT NULL
         ORDER BY t.number"
        ))?
        .query_map(params![project_id, chief], task_view)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut held = Vec::new();
    for mut task in tasks {
        let notes = store
            .db
            .prepare(
                "SELECT body FROM message
                  WHERE task_id = ? AND recipient_id = ? AND sender_id IS NULL AND kind = 'note'
                  ORDER BY id DESC",
            )?
            .query_map(params![task.id, chief], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let hold = notes.iter().find_map(|body| {
            if is_busy_note(task.number, body) {
                Some(None)
            } else {
                hold_said(task.number, body).map(Some)
            }
        });
        if let Some(Some(said)) = hold {
            task.aside = Some(said);
            held.push(task);
        }
    }
    Ok(held)
}

/// The human's Switch chief: the chief runs on the harness and the model picked from now
/// on, and on no saved agent: one an older chief named is let go. Its conversation ends
/// here: a conversation belongs to one harness, and every switch starts a fresh one that
/// reads the history, whose first launch has asked for no model yet. What it was out of
/// quota for was the old harness's account, so that clears. The window is the caller's to
/// close before and open after. The chief keeps what it was switched from (the harness and
/// the model picked for it, or the saved agent an old chief ran on), and whether the old
/// chief was stopped mid-turn, for the handoff to say.
pub(crate) fn switch_chief(
    store: &mut Store,
    project_id: i64,
    switch: &ChiefSwitch,
) -> Result<ProjectView, LedgerError> {
    model::require_harness(&switch.harness)?;
    model::require_model_id(&switch.model)?;
    model::parse_model_name(switch.model_name.as_ref().map(|name| json!(name)).as_ref())?;
    store.write(|store| {
        let chief = store.participant_by_handle(project_id, "chief")?;
        store.db.execute(
            "UPDATE participant SET harness = ?, model = ?, effort = ?, model_name = ?, agent = NULL, launched_model = NULL,
           gone_model = NULL, out_until = NULL, needs_edit = 0, switched_from_harness = ?, switched_from_model = ?,
           switched_from_agent = ?, switched_from_cut = ?
         WHERE id = ?",
            params![
                switch.harness,
                switch.model,
                switch.effort,
                switch.model_name,
                chief.harness,
                chief.model,
                chief.agent,
                i64::from(switch.cut),
                chief.id
            ],
        )?;
        let at = store.at();
        store.db.execute(
            "UPDATE conversation SET ended_at = ? WHERE participant_id = ? AND ended_at IS NULL",
            params![at, chief.id],
        )?;
        // A chief from before a chief was a model ran on a saved agent: its switch keeps it,
        // as Node's event and handoff named it (Calliope's P3, T-955).
        let mut from = json!({ "harness": chief.harness, "model": chief.model });
        if let Some(agent) = &chief.agent {
            from["agent"] = json!(agent);
        }
        store.log(
            project_id,
            "chief.switched",
            json!({
                "from": from,
                "to": { "harness": switch.harness, "model": switch.model },
                "cut": switch.cut,
            }),
        )?;
        known_project(store, project_id)
    })
}

/// The chief read its history (`cf history`): which page, or what it searched for.
pub(crate) fn history_read(
    store: &mut Store,
    project_id: i64,
    page: i64,
    find: Option<&str>,
    tools: bool,
) -> Result<(), LedgerError> {
    store.write(|store| {
        store.project_row(project_id)?;
        store.log(
            project_id,
            "chief.history.read",
            json!({ "page": page, "find": find, "tools": tools }),
        )
    })
}

/// The project's latest Switch chief: what the chief was switched from (its
/// harness, and the model picked for it where one was, else the saved agent an old chief
/// ran on) and whether its turn was cut; none before any.
pub(crate) fn last_switch(
    store: &Store,
    project_id: i64,
) -> Result<Option<LastSwitch>, LedgerError> {
    let chief = store.participant_by_handle(project_id, "chief")?;
    Ok(chief.switched_from_harness.map(|harness| LastSwitch {
        from: SwitchedFrom {
            harness,
            model: chief.switched_from_model.or(chief.switched_from_agent),
        },
        cut: chief.switched_from_cut == 1,
    }))
}

/// A chief the update could not tell the model of (`needs_edit`, T-1202) whose harness's
/// record names the model and the effort its conversation ran on last: it runs on them, and
/// needs no edit. Pi's thinking is held as its effort, as Pi's is everywhere.
pub(crate) fn chief_ran_on(
    store: &mut Store,
    participant_id: i64,
    model: &str,
    effort: Option<&str>,
) -> Result<(), LedgerError> {
    model::require_model_id(model)?;
    store.write(|store| {
        store.db.execute(
            "UPDATE participant SET model = ?, effort = ?, model_name = NULL, needs_edit = 0
              WHERE id = ? AND role = 'chief'",
            params![model, effort, participant_id],
        )?;
        Ok(())
    })
}

/// The chief's launch asked its harness for `model`, none where it left the model to the
/// harness's own default: what the next reopen compares the record's last model with
/// (CHF-05).
pub(crate) fn chief_launched(
    store: &mut Store,
    participant_id: i64,
    model: Option<&str>,
) -> Result<(), LedgerError> {
    if let Some(model) = model {
        model::require_model_id(model)?;
    }
    store.write(|store| {
        store.db.execute(
            "UPDATE participant SET launched_model = ? WHERE id = ? AND role = 'chief'",
            params![model, participant_id],
        )?;
        Ok(())
    })
}

/// The chief's launch found its harness no longer has `gone`: no reopen asks for it again,
/// and the chief runs on `model` (its harness's default, none where the harness is left
/// to choose it) where its record names none (CHF-06). Its next launch has asked for
/// nothing yet.
pub(crate) fn chief_gone(
    store: &mut Store,
    participant_id: i64,
    gone: &str,
    model: Option<&str>,
) -> Result<(), LedgerError> {
    model::require_model_id(gone)?;
    if let Some(model) = model {
        model::require_model_id(model)?;
    }
    store.write(|store| {
        store.db.execute(
            "UPDATE participant SET model = ?, model_name = NULL, gone_model = ?, launched_model = NULL
             WHERE id = ? AND role = 'chief'",
            params![model, gone, participant_id],
        )?;
        Ok(())
    })
}

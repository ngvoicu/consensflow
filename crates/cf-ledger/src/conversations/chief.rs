//! The chief across a Switch chief: the switch itself, what it was switched
//! from, and what a chief that takes over reads, its history and its open work.

use cf_proto::ledger::{
    ChiefConversation, ChiefOpenWork, HistoryItem, LastSwitch, ProjectView, SwitchedFrom,
};
use rusqlite::params;
use serde_json::{json, Value};

use crate::model::{self, LedgerError};
use crate::projects::known_project;
use crate::store::Store;
use crate::views::{conversation_view, message_view, task_view, MESSAGE_SELECT, TASK_SELECT};

/// The human's Switch chief: the saved agent the chief runs on from now on,
/// on its harness, and whether the old chief was stopped mid-turn.
#[derive(Debug, Clone, PartialEq)]
pub struct ChiefSwitch {
    pub harness: String,
    pub agent: String,
    pub cut: bool,
}

impl ChiefSwitch {
    /// The switch as JSON gives it, read in the order the Node ledger checked it.
    pub fn from_json(value: &Value) -> Result<Self, LedgerError> {
        let harness = model::parse_harness(value.get("harness"))?;
        let agent = model::parse_agent_id(value.get("agent"), &[])?;
        Ok(Self {
            harness: harness.to_string(),
            agent,
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
    let mut items = store.db.prepare(
        "SELECT item_id, role, text, complete, at FROM transcript WHERE conversation_id = ? ORDER BY seq",
    )?;
    conversations
        .into_iter()
        .map(|conversation| {
            let items = items
                .query_map([conversation.id], |row| {
                    Ok(HistoryItem {
                        id: row.get("item_id")?,
                        role: row.get("role")?,
                        text: row.get("text")?,
                        complete: row.get::<_, i64>("complete")? == 1,
                        at: row.get("at")?,
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(ChiefConversation {
                conversation,
                items,
            })
        })
        .collect()
}

/// What waits on the chief now, for a chief that takes over: members'
/// questions to it without an answer, results it has not decided on, and
/// its own unfinished tasks.
pub(crate) fn chief_open_work(
    store: &Store,
    project_id: i64,
) -> Result<ChiefOpenWork, LedgerError> {
    let chief = store.participant_by_handle(project_id, "chief")?;
    let questions = store
        .db
        .prepare(&format!(
            "{MESSAGE_SELECT}
         WHERE m.project_id = ? AND m.recipient_id = ? AND m.kind = 'question'
           AND m.state NOT IN ('gated', 'cancelled')
           AND NOT EXISTS (
             SELECT 1 FROM message a WHERE a.reply_to = m.id AND a.kind = 'answer'
               AND a.state NOT IN ('gated', 'cancelled')
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
    Ok(ChiefOpenWork {
        questions,
        results,
        own,
    })
}

/// The human's Switch chief: the chief runs on the saved agent (its model
/// and effort) on its harness from now on, never on a harness's own
/// default. Its conversation ends here: a conversation belongs to one
/// harness, and every switch starts a fresh one that reads the history.
/// What it was out of quota for was the old harness's account, so that
/// clears. The window is the caller's to close before and open after. The
/// chief keeps what it was switched from, and whether the old chief was
/// stopped mid-turn, for the handoff to say.
pub(crate) fn switch_chief(
    store: &mut Store,
    project_id: i64,
    switch: &ChiefSwitch,
) -> Result<ProjectView, LedgerError> {
    model::require_harness(&switch.harness)?;
    model::require_agent_id(&switch.agent, &[])?;
    store.write(|store| {
        let chief = store.participant_by_handle(project_id, "chief")?;
        store.db.execute(
            "UPDATE participant SET harness = ?, agent = ?, out_until = NULL,
           switched_from_harness = ?, switched_from_agent = ?, switched_from_cut = ?
         WHERE id = ?",
            params![
                switch.harness,
                switch.agent,
                chief.harness,
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
        store.log(
            project_id,
            "chief.switched",
            json!({
                "from": { "harness": chief.harness, "agent": chief.agent },
                "to": { "harness": switch.harness, "agent": switch.agent },
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
/// harness and agent) and whether its turn was cut; none before any.
pub(crate) fn last_switch(
    store: &Store,
    project_id: i64,
) -> Result<Option<LastSwitch>, LedgerError> {
    let chief = store.participant_by_handle(project_id, "chief")?;
    Ok(chief.switched_from_harness.map(|harness| LastSwitch {
        from: SwitchedFrom {
            harness,
            agent: chief.switched_from_agent,
        },
        cut: chief.switched_from_cut == 1,
    }))
}

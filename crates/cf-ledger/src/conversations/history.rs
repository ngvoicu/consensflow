//! A participant's whole history, as the history viewer reads it: every
//! conversation it had, the current one too, with the agent it ran on and what
//! ended it as far as the ledger's log says, and every item copied of each.

use std::collections::{HashMap, HashSet};

use cf_proto::ledger::{ConversationEnd, ParticipantConversation};
use serde_json::Value;

use super::transcripts::items_of;
use crate::model::LedgerError;
use crate::store::Store;
use crate::views::conversation_view;

/// Every conversation of the participant `handle`, oldest first (in the order
/// each began), with its copied items in order. Nothing is written.
pub(crate) fn participant_history(
    store: &Store,
    project_id: i64,
    handle: &str,
) -> Result<Vec<ParticipantConversation>, LedgerError> {
    let participant = store.participant_by_handle(project_id, handle)?;
    let conversations = store
        .db
        .prepare("SELECT * FROM conversation WHERE participant_id = ? ORDER BY id")?
        .query_map([participant.id], conversation_view)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let own: HashSet<i64> = conversations
        .iter()
        .map(|conversation| conversation.id)
        .collect();
    let told = told(
        store,
        project_id,
        &own,
        participant.role == "chief",
        participant.agent.as_deref(),
    )?;
    conversations
        .into_iter()
        .map(|conversation| {
            let id = conversation.id;
            Ok(ParticipantConversation {
                agent: told
                    .agents
                    .get(&id)
                    .cloned()
                    .unwrap_or_else(|| participant.agent.clone()),
                ended_by: conversation
                    .ended_at
                    .as_ref()
                    .and_then(|_| told.ends.get(&id).cloned()),
                items: items_of(store, id)?,
                conversation,
            })
        })
        .collect()
}

/// What the log says of a participant's conversations, by id: the agent each
/// began on, and what ended each the last time it ended.
struct Told {
    agents: HashMap<i64, Option<String>>,
    ends: HashMap<i64, ConversationEnd>,
}

/// Reads the project's log in order for the conversations `own`. One begins
/// (`conversation.started`) or the window goes to one (`conversation.followed`),
/// and the participant's conversation before it ends: in a window that opened
/// on a fresh one, or in the window that went to another, which is one of its
/// own it had before (a `/resume`) or one it had not (`/clear`, `/new`, or a
/// `/resume` of a session the ledger never saw). A `chief.switched` ends the
/// chief's: each switch says the agent the chief ran on until it, so a
/// conversation of the `chief`'s began on the agent the next switch after it
/// was from, or on the agent it has `now`. A `conversation.ended` ends one and
/// says no more.
fn told(
    store: &Store,
    project_id: i64,
    own: &HashSet<i64>,
    chief: bool,
    now: Option<&str>,
) -> Result<Told, LedgerError> {
    let events = store
        .db
        .prepare(
            "SELECT kind, data FROM event WHERE project_id = ? AND kind IN
         ('conversation.started', 'conversation.followed', 'conversation.ended', 'chief.switched')
       ORDER BY id",
        )?
        .query_map([project_id], |row| {
            Ok((row.get::<_, String>("kind")?, row.get::<_, String>("data")?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let events: Vec<(String, Value)> = events
        .into_iter()
        .filter(|(kind, _)| chief || kind != "chief.switched")
        .map(|(kind, data)| (kind, serde_json::from_str(&data).unwrap_or(Value::Null)))
        .collect();
    let text = |value: &Value| value.as_str().map(str::to_owned);
    let switched_from: Vec<Option<String>> = events
        .iter()
        .filter(|(kind, _)| kind == "chief.switched")
        .map(|(_, data)| text(&data["from"]["agent"]))
        .collect();
    let mut told = Told {
        agents: HashMap::new(),
        ends: HashMap::new(),
    };
    let mut switches = 0;
    let mut current: Option<i64> = None;
    for (kind, data) in &events {
        if kind == "chief.switched" {
            if let Some(ended) = current.take() {
                let to = &data["to"];
                let end = ConversationEnd::Switch {
                    harness: text(&to["harness"]).unwrap_or_default(),
                    agent: text(&to["agent"]),
                };
                told.ends.insert(ended, end);
            }
            switches += 1;
            continue;
        }
        let Some(id) = data["conversation"].as_i64().filter(|id| own.contains(id)) else {
            continue;
        };
        if kind == "conversation.ended" {
            if current == Some(id) {
                current = None;
            }
            told.ends.remove(&id);
            continue;
        }
        let seen = told.agents.contains_key(&id);
        if let Some(before) = current.filter(|before| *before != id) {
            let end = if kind == "conversation.started" {
                ConversationEnd::Fresh
            } else {
                ConversationEnd::Window { resumed: seen }
            };
            told.ends.insert(before, end);
        }
        if !seen {
            let agent = match switched_from.get(switches) {
                Some(from) => from.clone(),
                None => now.map(str::to_owned),
            };
            told.agents.insert(id, agent);
        }
        current = Some(id);
    }
    Ok(told)
}

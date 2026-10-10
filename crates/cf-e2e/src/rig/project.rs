//! A project the rig opened, and the page's views of it: what the suites share
//! of reading the board, a task, a lane and an inbox, and of the staff they
//! open a project with.

use std::collections::BTreeMap;

use serde_json::{json, Value};

use super::Rig;
use crate::{files, Error, Result};

/// A project of the daemon's, by its id.
#[derive(Clone, Copy)]
pub struct Project<'a> {
    rig: &'a Rig,
    id: i64,
}

impl<'a> Project<'a> {
    /// The project `id` of the rig's daemon.
    pub fn new(rig: &'a Rig, id: i64) -> Self {
        Self { rig, id }
    }

    /// Opens a project in the rig's workspace with `agent` as its chief, as the
    /// New project dialog does. `more` is added to the request (`gate`,
    /// `staff`). The daemon's answer is a refusal when it says it is not `ok`.
    pub fn open(rig: &'a Rig, agent: &str, more: Value) -> Result<Self> {
        let mut request = json!({ "directory": rig.workspace(), "agent": agent });
        if let (Some(request), Some(more)) = (request.as_object_mut(), more.as_object()) {
            request.extend(
                more.iter()
                    .map(|(name, value)| (name.clone(), value.clone())),
            );
        }
        let opened = rig.page("project.open", request)?;
        let id = refused(&opened, "project.open")?["project"]["id"]
            .as_i64()
            .ok_or_else(|| Error::Daemon(format!("project.open opened no project: {opened}")))?;
        Ok(Self { rig, id })
    }

    /// Opens a project as the New project dialog opens one: with the staff
    /// (`members`: an agent and its role each) and the approval setting
    /// (`gate`, when given) together.
    pub fn open_with_staff(
        rig: &'a Rig,
        chief: &str,
        gate: Option<bool>,
        members: &[(&str, &str)],
    ) -> Result<Self> {
        let staff: Vec<Value> = members
            .iter()
            .map(|(agent, role)| json!({ "agent": agent, "roles": [role] }))
            .collect();
        let mut more = serde_json::Map::new();
        if let Some(gate) = gate {
            more.insert("gate".to_owned(), json!(gate));
        }
        more.insert("staff".to_owned(), Value::from(staff));
        Self::open(rig, chief, Value::Object(more))
    }

    /// Its id.
    pub fn id(&self) -> i64 {
        self.id
    }

    /// Adds the agent `agent` to the staff, as the human does: the daemon's
    /// answer, which holds the `member` added.
    pub fn add_member(&self, agent: &str) -> Result<Value> {
        let added = self
            .rig
            .page("member.add", json!({ "project": self.id, "agent": agent }))?;
        refused(&added, "member.add").cloned()
    }

    /// The board, as the page draws it.
    pub fn board(&self) -> Result<Value> {
        let answer = self.rig.page("board.get", json!({ "project": self.id }))?;
        Ok(answer["board"].clone())
    }

    /// The task `number`, or null when there is none.
    pub fn task(&self, number: i64) -> Result<Value> {
        let answer = self
            .rig
            .page("task.get", json!({ "project": self.id, "task": number }))?;
        Ok(answer["task"].clone())
    }

    /// The lane of the member or session called `handle`: the newest one, when
    /// a member's work has run in several sessions (a member's work runs in a
    /// session of its own, and its lane is the session's). Null when there is
    /// none.
    pub fn lane(&self, handle: &str) -> Result<Value> {
        let board = self.board()?;
        Ok(board["lanes"]
            .as_array()
            .and_then(|lanes| {
                lanes.iter().rev().find(|lane| {
                    lane["participant"]["member"] == handle
                        || lane["participant"]["handle"] == handle
                })
            })
            .cloned()
            .unwrap_or(Value::Null))
    }

    /// The messages in the inbox of `participant`, in the order they were made.
    pub fn inbox(&self, participant: &str) -> Result<Vec<Value>> {
        let answer = self.rig.page(
            "inbox.get",
            json!({ "project": self.id, "participant": participant }),
        )?;
        Ok(answer["messages"].as_array().cloned().unwrap_or_default())
    }

    /// The tier of each agent of the staff, by the agent's id, as the board
    /// shows them.
    pub fn tiers(&self) -> Result<BTreeMap<String, String>> {
        let board = self.board()?;
        Ok(board["lanes"]
            .as_array()
            .map(|lanes| {
                lanes
                    .iter()
                    .filter_map(|lane| {
                        Some((
                            lane["participant"]["agent"].as_str()?.to_owned(),
                            lane["participant"]["tier"].as_str()?.to_owned(),
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default())
    }
}

/// Writes the roster of four agents of the stand-in `claude`, as the tiered
/// cases use: the chief, two workers on one model and a reviewer on another.
pub fn write_staff(rig: &Rig) -> Result<()> {
    let agent = |id: &str, model: &str| json!({ "id": id, "kind": "claude-code", "model": model });
    let roster = json!({
        "schemaVersion": 1,
        "agents": [
            agent("chief", "fake-chief"),
            agent("worker", "fake"),
            agent("worker2", "fake"),
            agent("checker", "fake-2"),
        ],
    });
    files::write(&rig.home().join("agents.json"), format!("{roster}\n"))
}

/// The answer, if it says `ok`, and the refusal it is if it does not.
fn refused<'v>(answer: &'v Value, op: &str) -> Result<&'v Value> {
    if answer["ok"] == true {
        Ok(answer)
    } else {
        Err(Error::Daemon(format!("{op} was refused: {answer}")))
    }
}

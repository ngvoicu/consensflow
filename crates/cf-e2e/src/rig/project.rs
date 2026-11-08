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

    /// Opens a project in the rig's workspace with its chief on the harness and the
    /// model of the stand-in `agent` ([`chief_of`]), as the New project dialog picks
    /// them. `more` is added to the request (`gate`, `staff`, each member of the staff a
    /// stand-in by its name, `{agent, roles}`, sent as its harness, model, effort and
    /// tier: [`member_body`]). The daemon's answer is a refusal when it says it is not `ok`.
    pub fn open(rig: &'a Rig, agent: &str, more: Value) -> Result<Self> {
        let (harness, model) = chief_of(rig, agent)?;
        let mut request =
            json!({ "directory": rig.workspace(), "harness": harness, "model": model });
        if let (Some(request), Some(more)) = (request.as_object_mut(), more.as_object()) {
            request.extend(
                more.iter()
                    .map(|(name, value)| (name.clone(), value.clone())),
            );
        }
        if let Some(Value::Array(staff)) = request.get_mut("staff") {
            for member in staff.iter_mut() {
                if let Some(name) = member["agent"].as_str().map(str::to_owned) {
                    *member = member_body(rig, &name, member.get("roles").cloned())?;
                }
            }
        }
        let opened = rig.page("project.open", request)?;
        let id = refused(&opened, "project.open")?["project"]["id"]
            .as_i64()
            .ok_or_else(|| Error::Daemon(format!("project.open opened no project: {opened}")))?;
        Ok(Self { rig, id })
    }

    /// Opens a project as the New project dialog opens one: with the staff
    /// (`members`: a stand-in and its role each) and the approval setting
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

    /// Adds the stand-in `agent` to the staff, as the human does in Add member: its
    /// harness, model, effort and tier ([`member_body`]). The daemon's answer, which holds
    /// the `member` added.
    pub fn add_member(&self, agent: &str) -> Result<Value> {
        let mut body = member_body(self.rig, agent, None)?;
        body["project"] = json!(self.id);
        let added = self.rig.page("member.add", body)?;
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

    /// The tier of each member of the staff, by its handle, as the board shows them.
    pub fn tiers(&self) -> Result<BTreeMap<String, String>> {
        let board = self.board()?;
        Ok(board["lanes"]
            .as_array()
            .map(|lanes| {
                lanes
                    .iter()
                    .filter(|lane| lane["participant"]["member"].is_null())
                    .filter_map(|lane| {
                        Some((
                            lane["participant"]["handle"].as_str()?.to_owned(),
                            lane["participant"]["tier"].as_str()?.to_owned(),
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default())
    }
}

/// Writes the stand-ins of the stand-in `claude` the tiered cases use: the chief, two
/// workers on one model and a checker on another (a reviewer, or a tester, as the case
/// staffs it).
pub fn write_staff(rig: &Rig) -> Result<()> {
    let agent = |id: &str, model: &str| json!({ "id": id, "kind": "claude-code", "model": model });
    rig.stand_ins(&json!({
        "agents": [
            agent("chief", "fake-chief"),
            agent("worker", "fake"),
            agent("worker2", "fake"),
            agent("checker", "fake-2"),
        ],
    }))
}

/// The stand-in a case named `agent`, as the rig's stand-ins have it ([`Rig::stand_ins`]),
/// else one of the few the cases name without writing them: `maia` (Codex, GPT-6 Astra at
/// medium) and `pygmalion` (an image designer on Codex's default).
fn stand_in(rig: &Rig, agent: &str) -> Result<Value> {
    let text = files::read_string(&rig.stand_ins_file()).unwrap_or_default();
    let written: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    if let Some(row) = written["agents"].as_array().and_then(|rows| {
        rows.iter()
            .find(|row| row["id"] == agent || row["name"] == agent)
    }) {
        return Ok(row.clone());
    }
    match agent {
        "maia" => {
            Ok(json!({ "id": "maia", "kind": "codex", "model": "gpt-6-astra", "effort": "medium" }))
        }
        "pygmalion" => {
            Ok(json!({ "id": "pygmalion", "kind": "codex", "designer": true, "workTier": "light" }))
        }
        _ => Err(Error::Daemon(format!(
            "no stand-in {agent} in the rig's stand-ins"
        ))),
    }
}

/// The harness and the model of the stand-in `agent` ([`stand_in`]): what a case picks a
/// chief by, as the New project and Switch chief dialogs pick a harness and a model
/// (CHF-07).
pub fn chief_of(rig: &Rig, agent: &str) -> Result<(String, String)> {
    let row = stand_in(rig, agent)?;
    let harness = row["kind"]
        .as_str()
        .ok_or_else(|| Error::Daemon(format!("{agent} names no harness")))?;
    Ok((
        harness.to_owned(),
        row["model"].as_str().unwrap_or(agent).to_owned(),
    ))
}

/// The models a stand-in agent runs on (`fake`, `fake-2`, `fake-chief`), which a real
/// harness has none of: a member on one is asked for by its own name instead.
const STAND_IN_MODELS: [&str; 3] = ["fake", "fake-2", "fake-chief"];

/// The member the stand-in `agent` is, as Add member and New project ask for one: its
/// harness, its tier (`standard` where it names none), its effort (Pi's thinking), and its
/// model. A member of the stand-in harness runs every model alike: its model is the
/// stand-in's name, which the harness, never read, takes at its word and names the member
/// by, so its handle is `agent`. A member of a real harness runs its real model, and is
/// named as its harness names that model. `roles` where they are given.
pub fn member_body(rig: &Rig, agent: &str, roles: Option<Value>) -> Result<Value> {
    let row = stand_in(rig, agent)?;
    Ok(body_of(&row, agent, roles))
}

/// [`member_body`] of the stand-in row `row`. An image designer is asked for as Add member asks
/// for one: Codex and its role, no model, effort or tier (designer P2): it draws on Codex's own
/// model, and the daemon refuses any of the three.
fn body_of(row: &Value, agent: &str, roles: Option<Value>) -> Value {
    let designer = row["designer"] == true
        || roles
            .as_ref()
            .and_then(Value::as_array)
            .is_some_and(|roles| roles.iter().any(|role| role == "designer"));
    if designer {
        return json!({ "harness": row["kind"], "roles": ["designer"] });
    }
    let model = row["model"]
        .as_str()
        .filter(|model| !STAND_IN_MODELS.contains(model))
        .unwrap_or(agent);
    let mut body = json!({
        "harness": row["kind"],
        "model": model,
        "tier": row.get("workTier").filter(|tier| !tier.is_null()).cloned().unwrap_or(json!("standard")),
    });
    if let Some(effort) = ["effort", "thinking"]
        .into_iter()
        .find_map(|key| row.get(key).filter(|effort| !effort.is_null()))
    {
        body["effort"] = effort.clone();
    }
    if let Some(roles) = roles {
        body["roles"] = roles;
    }
    body
}

/// `more` with a chief's harness and model in it: those of the stand-in `agent`
/// ([`chief_of`]), as New project and Switch chief send them.
pub fn chief_body(rig: &Rig, agent: &str, more: Value) -> Result<Value> {
    let (harness, model) = chief_of(rig, agent)?;
    let mut body = more;
    if let Some(fields) = body.as_object_mut() {
        fields.insert("harness".to_owned(), json!(harness));
        fields.insert("model".to_owned(), json!(model));
    }
    Ok(body)
}

/// The answer, if it says `ok`, and the refusal it is if it does not.
fn refused<'v>(answer: &'v Value, op: &str) -> Result<&'v Value> {
    if answer["ok"] == true {
        Ok(answer)
    } else {
        Err(Error::Daemon(format!("{op} was refused: {answer}")))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::body_of;

    #[test]
    fn an_image_designer_is_asked_for_on_codex_and_its_role_alone() {
        let designer =
            json!({ "id": "pygmalion", "kind": "codex", "designer": true, "workTier": "light" });
        let wanted = json!({ "harness": "codex", "roles": ["designer"] });
        assert_eq!(body_of(&designer, "pygmalion", None), wanted);
        assert_eq!(
            body_of(&designer, "pygmalion", Some(json!(["designer"]))),
            wanted
        );
        // A stand-in written without the flag, asked for in the role.
        let plain = json!({ "id": "osiris", "kind": "codex" });
        assert_eq!(body_of(&plain, "osiris", Some(json!(["designer"]))), wanted);
    }

    #[test]
    fn any_other_member_is_asked_for_by_its_model_its_tier_and_its_roles() {
        let worker =
            json!({ "id": "maia", "kind": "codex", "model": "gpt-6-astra", "effort": "medium" });
        assert_eq!(
            body_of(&worker, "maia", Some(json!(["reviewer"]))),
            json!({
                "harness": "codex", "model": "gpt-6-astra", "tier": "standard",
                "effort": "medium", "roles": ["reviewer"],
            })
        );
    }
}

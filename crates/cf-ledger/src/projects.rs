//! Projects: each with its human, its chief and its staff, open or
//! suspended, gated or not; brought back after a restart of the daemon,
//! logged event by event, and deleted with everything in it once closed
//! (`src/ledger/projects.js`).

use cf_base::json::from_slice_lossy;
use cf_proto::ledger::{DeletedProject, EventView, ProjectView};
use rusqlite::{params, OptionalExtension, Row};
use serde_json::{json, Value};

use crate::model::{self, LedgerError, COORDINATOR_HANDLES};
use crate::staff::{add_participant, NewParticipant};
use crate::store::Store;
use crate::views::{participant_view, ParticipantRow, PARTICIPANT_SELECT};

/// A project's row.
pub(crate) struct ProjectRow {
    pub(crate) id: i64,
    pub(crate) directory: String,
    pub(crate) name: String,
    pub(crate) state: String,
    pub(crate) resume_on_start: i64,
    pub(crate) gate: i64,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
}

impl ProjectRow {
    pub(crate) fn read(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get("id")?,
            directory: row.get("directory")?,
            name: row.get("name")?,
            state: row.get("state")?,
            resume_on_start: row.get("resume_on_start")?,
            gate: row.get("gate")?,
            created_at: row.get("created_at")?,
            updated_at: row.get("updated_at")?,
        })
    }
}

/// A new project's chief: the harness it runs on and, since chiefs are
/// always saved agents, the agent; one with none is from before then and
/// runs on its harness's default.
#[derive(Debug, Clone, PartialEq)]
pub struct NewChief {
    pub harness: String,
    pub agent: Option<String>,
}

/// A member a new project starts with.
#[derive(Debug, Clone, PartialEq)]
pub struct NewMember {
    pub agent: String,
    pub harness: String,
    pub designer: bool,
    /// One or more of worker, advisor, reviewer and designer; the first leads.
    pub roles: Vec<String>,
    pub tier: String,
}

/// A project to open: where, under what name, its chief, its staff (the
/// last project's, usually), and whether the human approves each message.
#[derive(Debug, Clone, PartialEq)]
pub struct NewProject {
    pub directory: String,
    pub name: String,
    pub chief: NewChief,
    pub staff: Vec<NewMember>,
    pub gate: bool,
}

impl NewProject {
    /// The request as JSON gives it, read in the order the Node ledger checked it.
    pub fn from_json(value: &Value) -> Result<Self, LedgerError> {
        let directory = model::parse_text(value.get("directory"), "directory", 4096)?;
        let name = model::parse_text(value.get("name"), "name", 100)?;
        let chief = value.get("chief");
        let harness = model::parse_harness(chief.and_then(|chief| chief.get("harness")))?;
        let agent = match chief.and_then(|chief| chief.get("agent")) {
            None | Some(Value::Null) => None,
            agent => Some(model::parse_agent_id(agent, &[])?),
        };
        let gate = match value.get("gate") {
            None => false,
            gate => model::parse_gate(gate)?,
        };
        let staff = match value.get("staff") {
            None => Vec::new(),
            Some(Value::Array(members)) => members
                .iter()
                .map(NewMember::from_json)
                .collect::<Result<_, _>>()?,
            Some(other) => {
                return Err(LedgerError::refused(
                    "invalid-staff",
                    format!("staff is a list of members, not {other}"),
                ))
            }
        };
        Ok(Self {
            directory,
            name,
            chief: NewChief {
                harness: harness.to_string(),
                agent,
            },
            staff,
            gate,
        })
    }
}

impl NewMember {
    /// The member as JSON gives it, read in the order the Node ledger checked it.
    pub fn from_json(value: &Value) -> Result<Self, LedgerError> {
        // Its roles, or its one role; a member given neither has none.
        let roles = match value.get("roles") {
            Some(roles) if !roles.is_null() => model::parse_roles(Some(roles))?,
            _ => {
                let given = value
                    .get("role")
                    .map_or_else(|| json!([]), |role| json!([role]));
                model::parse_roles(Some(&given))?
            }
        };
        let agent = model::parse_agent_id(value.get("agent"), &COORDINATOR_HANDLES)?;
        let harness = model::parse_harness(value.get("harness"))?;
        let designer = match value.get("designer") {
            None => false,
            Some(Value::Bool(designer)) => *designer,
            Some(_) => return Err(designer_refused()),
        };
        model::require_fitting_roles(&agent, designer, &roles)?;
        let tier = model::parse_tier(value.get("tier"))?;
        Ok(Self {
            agent,
            harness: harness.to_string(),
            designer,
            roles: roles.into_iter().map(str::to_string).collect(),
            tier: tier.to_string(),
        })
    }

    /// Checks the member as the Node ledger did: its roles, normalized.
    pub(crate) fn check(&self) -> Result<Vec<&'static str>, LedgerError> {
        let roles = model::require_roles(&self.roles, || model::printed(Some(&json!(self.roles))))?;
        model::require_agent_id(&self.agent, &COORDINATOR_HANDLES)?;
        model::require_harness(&self.harness)?;
        model::require_fitting_roles(&self.agent, self.designer, &roles)?;
        model::require_tier(&self.tier)?;
        Ok(roles)
    }
}

fn designer_refused() -> LedgerError {
    LedgerError::refused(
        "invalid-designer",
        "an agent is an image agent (true) or not (false)",
    )
}

/// A project with its chief, on the saved agent it runs on, and its staff.
pub(crate) fn create_project(
    store: &mut Store,
    request: &NewProject,
) -> Result<ProjectView, LedgerError> {
    model::require_text(&request.directory, "directory", 4096)?;
    model::require_text(&request.name, "name", 100)?;
    let chief_harness = model::require_harness(&request.chief.harness)?;
    if let Some(agent) = &request.chief.agent {
        model::require_agent_id(agent, &[])?;
    }
    let members = request
        .staff
        .iter()
        .map(|member| Ok((member, member.check()?)))
        .collect::<Result<Vec<_>, LedgerError>>()?;
    store.write(|store| {
        let at = store.at();
        store.db.execute(
            "INSERT INTO project (directory, name, state, gate, created_at, updated_at)
             VALUES (?, ?, 'open', ?, ?, ?)",
            params![
                request.directory,
                request.name,
                i64::from(request.gate),
                at,
                at
            ],
        )?;
        let id = store.db.last_insert_rowid();
        add_participant(store, id, NewParticipant::coordinator("human", None, None))?;
        add_participant(
            store,
            id,
            NewParticipant::coordinator(
                "chief",
                request.chief.agent.as_deref(),
                Some(chief_harness),
            ),
        )?;
        for (member, roles) in &members {
            add_participant(
                store,
                id,
                NewParticipant {
                    handle: &member.agent,
                    role: None,
                    roles,
                    agent: Some(&member.agent),
                    harness: Some(&member.harness),
                    designer: member.designer,
                    tier: Some(&member.tier),
                },
            )?;
        }
        store.log(
            id,
            "project.created",
            json!({ "name": request.name, "directory": request.directory }),
        )?;
        known_project(store, id)
    })
}

/// A project and the participants still in it; none for an id no project has.
pub(crate) fn project(store: &Store, id: i64) -> Result<Option<ProjectView>, LedgerError> {
    let Some(row) = store
        .db
        .query_row("SELECT * FROM project WHERE id = ?", [id], ProjectRow::read)
        .optional()?
    else {
        return Ok(None);
    };
    let mut statement = store.db.prepare(&format!(
        "{PARTICIPANT_SELECT} WHERE p.project_id = ? AND p.left_at IS NULL ORDER BY p.id"
    ))?;
    let participants = statement
        .query_map([id], ParticipantRow::read)?
        .collect::<rusqlite::Result<Vec<_>>>()?
        .iter()
        .map(participant_view)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Some(ProjectView {
        id: row.id,
        directory: row.directory,
        name: row.name,
        state: row.state,
        gate: row.gate == 1,
        resume_on_start: row.resume_on_start == 1,
        created_at: row.created_at,
        updated_at: row.updated_at,
        participants,
    }))
}

/// A project the operation has just found or made.
pub(crate) fn known_project(store: &Store, id: i64) -> Result<ProjectView, LedgerError> {
    project(store, id)?.ok_or_else(|| {
        LedgerError::refused_with("unknown-project", format!("no project {id}"), 404)
    })
}

pub(crate) fn projects(store: &Store) -> Result<Vec<ProjectView>, LedgerError> {
    let ids = store
        .db
        .prepare("SELECT id FROM project ORDER BY id")?
        .query_map([], |row| row.get::<_, i64>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    ids.into_iter().map(|id| known_project(store, id)).collect()
}

/// Open or suspend a project by hand; either way it is no longer due a resume.
pub(crate) fn set_project_state(
    store: &mut Store,
    id: i64,
    state: &str,
) -> Result<ProjectView, LedgerError> {
    if state != "open" && state != "suspended" {
        return Err(LedgerError::refused(
            "invalid-state",
            format!("a project is open or suspended, not {state}"),
        ));
    }
    store.write(|store| {
        let from = store.project_row(id)?.state;
        let at = store.at();
        store.db.execute(
            "UPDATE project SET state = ?, resume_on_start = 0, updated_at = ? WHERE id = ?",
            params![state, at, id],
        )?;
        if from != state {
            store.log(id, "project.state", json!({ "from": from, "to": state }))?;
        }
        known_project(store, id)
    })
}

/// A closed project goes for good: its participants, conversations, tasks,
/// messages and events with it (the schema cascades). An open one is
/// refused: close it first, so nothing runs while its record disappears.
pub(crate) fn delete_project(store: &mut Store, id: i64) -> Result<DeletedProject, LedgerError> {
    store.write(|store| {
        let row = store.project_row(id)?;
        if row.state != "suspended" {
            return Err(LedgerError::refused_with("project-open", format!("{} is open: close it first", row.name), 409));
        }
        let count = |sql: &str| store.db.query_row(sql, [id], |row| row.get::<_, i64>(0));
        // What goes, for the one line the trace keeps.
        let gone = DeletedProject {
            id: row.id,
            name: row.name,
            directory: row.directory,
            created_at: row.created_at,
            members: count(
                "SELECT COUNT(*) AS n FROM participant
                 WHERE project_id = ? AND agent IS NOT NULL AND member_id IS NULL AND left_at IS NULL
                   AND role != 'chief'",
            )?,
            sessions: count("SELECT COUNT(*) AS n FROM participant WHERE project_id = ? AND member_id IS NOT NULL")?,
            tasks: count("SELECT COUNT(*) AS n FROM task WHERE project_id = ?")?,
            messages: count("SELECT COUNT(*) AS n FROM message WHERE project_id = ?")?,
        };
        store.db.execute("DELETE FROM project WHERE id = ?", [id])?;
        Ok(gone)
    })
}

/// Human approval required: with the gate on, every message between two
/// agents waits for the human, who passes it on or declines it. What is
/// already gated stays so when the gate goes off; the human decides it.
pub(crate) fn set_gate(store: &mut Store, id: i64, gate: bool) -> Result<ProjectView, LedgerError> {
    store.write(|store| {
        let from = store.project_row(id)?.gate == 1;
        let at = store.at();
        store.db.execute(
            "UPDATE project SET gate = ?, updated_at = ? WHERE id = ?",
            params![i64::from(gate), at, id],
        )?;
        if from != gate {
            store.log(id, "project.gate", json!({ "from": from, "to": gate }))?;
        }
        known_project(store, id)
    })
}

/// At daemon start: the panes of every open project died with the previous
/// process, so each becomes suspended and is marked to come back by itself.
pub(crate) fn suspend_for_restart(store: &mut Store) -> Result<Vec<ProjectView>, LedgerError> {
    store.write(|store| {
        let open = store
            .db
            .prepare("SELECT id FROM project WHERE state = 'open' ORDER BY id")?
            .query_map([], |row| row.get::<_, i64>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for id in &open {
            let at = store.at();
            store.db.execute(
                "UPDATE project SET state = 'suspended', resume_on_start = 1, updated_at = ?
                 WHERE id = ?",
                params![at, id],
            )?;
            store.log(
                *id,
                "project.state",
                json!({ "from": "open", "to": "suspended", "resumeOnStart": true }),
            )?;
        }
        open.into_iter()
            .map(|id| known_project(store, id))
            .collect()
    })
}

/// A resume on start is tried once: after it, success or not, the mark goes.
pub(crate) fn forget_resume(store: &mut Store, id: i64) -> Result<(), LedgerError> {
    store.write(|store| {
        store.project_row(id)?;
        store
            .db
            .execute("UPDATE project SET resume_on_start = 0 WHERE id = ?", [id])?;
        Ok(())
    })
}

/// A project's events after the one numbered `after`, oldest first, at most `limit`.
pub(crate) fn events(
    store: &Store,
    project_id: i64,
    after: i64,
    limit: i64,
) -> Result<Vec<EventView>, LedgerError> {
    let rows = store
        .db
        .prepare("SELECT * FROM event WHERE project_id = ? AND id > ? ORDER BY id LIMIT ?")?
        .query_map(params![project_id, after, limit], |row| {
            Ok((
                row.get("id")?,
                row.get("project_id")?,
                row.get("at")?,
                row.get("kind")?,
                row.get::<_, String>("data")?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<(i64, i64, String, String, String)>>>()?;
    rows.into_iter()
        .map(|(id, project_id, at, kind, data)| {
            Ok(EventView {
                id,
                project_id,
                at,
                kind,
                data: from_slice_lossy(data.as_bytes())?,
            })
        })
        .collect()
}

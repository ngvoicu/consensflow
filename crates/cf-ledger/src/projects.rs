//! Projects: each with its human, its chief and its staff, open or suspended,
//! gated or not; every open one suspended when the daemon starts, for the human
//! to resume, logged event by event, and deleted with everything in it once
//! closed. One opens with the wake-up calls and the specs the human says for it,
//! in the write that makes it. The human may rename one and write a line under
//! its name ([`describing`]), write the rules its staff is given
//! ([`staff_rules`]) and choose the size its members' windows compact at, whatever
//! their harness ([`compaction`]).

mod advisor_tool;
mod compaction;
mod describing;
mod own_hands;
mod prompt_suggestion;
mod staff_copy;
mod staff_rules;

use cf_base::json::from_slice_lossy;
use cf_proto::ledger::{DeletedProject, EventView, ProjectView};
use cf_proto::specs::SpecsView;
use rusqlite::{params, OptionalExtension, Row};
use serde_json::{json, Value};

pub use advisor_tool::parse_advisor_tool;
pub(crate) use advisor_tool::set_advisor_tool;
pub use compaction::{
    launch_size, parse_compaction, COMPACTION_DEFAULT, COMPACTION_MAX, COMPACTION_MIN,
};
pub(crate) use compaction::{parse_new_compaction, set_compaction};
pub use describing::parse_description;
pub(crate) use describing::{describe_project, rename_project};
pub use own_hands::parse_own_hands;
pub(crate) use own_hands::set_own_hands;
pub use prompt_suggestion::parse_prompt_suggestion;
pub(crate) use prompt_suggestion::set_prompt_suggestion;
pub(crate) use staff_copy::create_project_with_staff;
pub(crate) use staff_rules::set_staff_rules;
pub use staff_rules::{parse_staff_rules, STAFF_RULES_MAX_BYTES, STAFF_RULES_MAX_LINES};

use crate::meeting::{end_every_meeting, end_meeting, Ended};
use crate::model::{self, LedgerError};
use crate::specs::{keep_specs, SpecsChange};
use crate::staff::{add_participant, NewParticipant};
use crate::store::Store;
use crate::tasks::resume_with_project;
use crate::views::{participant_view, ParticipantRow, PARTICIPANT_SELECT};
use crate::wake_ups::{keep_wake_up, withdraw_queued, NewWakeUp};

/// The most characters a project's name may have, when it is given and when
/// the human changes it.
const NAME_MAX: usize = 100;

/// `value` read as a project's name: text that says something and fits.
pub fn parse_name(value: Option<&Value>) -> Result<String, LedgerError> {
    model::parse_text(value, "name", NAME_MAX)
}

/// A name a project is given or changed to: text that says something and fits.
fn require_name(name: &str) -> Result<(), LedgerError> {
    model::require_text(name, "name", NAME_MAX)
}

/// A project's row.
pub(crate) struct ProjectRow {
    pub(crate) id: i64,
    pub(crate) directory: String,
    pub(crate) name: String,
    pub(crate) state: String,
    pub(crate) gate: i64,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
    /// What the project says of specs, each part null where it says none.
    pub(crate) specs: Option<i64>,
    pub(crate) specs_folder: Option<String>,
    pub(crate) specs_variant: Option<String>,
    /// The line the human wrote under the name; null where there is none.
    pub(crate) description: Option<String>,
    /// When the human began a meeting with the chief; null while none is on.
    pub(crate) meeting_at: Option<String>,
    /// The rules the human wrote for the staff; null where there are none.
    pub(crate) staff_rules: Option<String>,
    /// The size in tokens the human chose for the members' windows to compact at,
    /// whatever their harness; null for each harness's own point.
    pub(crate) compaction: Option<u32>,
    /// 1 where the human has the members run with Claude Code's own advisor tool; 0 (every
    /// project's until it is ticked) where they run without it.
    pub(crate) advisor_tool: i64,
    /// 1 where the human leaves Claude Code's prompt suggestion on in the project's windows;
    /// 0 (every project's until it is ticked) where they are launched with it switched off.
    pub(crate) prompt_suggestion: i64,
    pub(crate) own_hands: i64,
}

impl ProjectRow {
    pub(crate) fn read(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get("id")?,
            directory: row.get("directory")?,
            name: row.get("name")?,
            state: row.get("state")?,
            gate: row.get("gate")?,
            created_at: row.get("created_at")?,
            updated_at: row.get("updated_at")?,
            specs: row.get("specs")?,
            specs_folder: row.get("specs_folder")?,
            specs_variant: row.get("specs_variant")?,
            description: row.get("description")?,
            meeting_at: row.get("meeting_at")?,
            staff_rules: row.get("staff_rules")?,
            compaction: row.get("compaction")?,
            advisor_tool: row.get("advisor_tool")?,
            prompt_suggestion: row.get("prompt_suggestion")?,
            own_hands: row.get("own_hands")?,
        })
    }

    /// What the project says of specs: none where it says nothing.
    pub(crate) fn specs_view(&self) -> Option<SpecsView> {
        SpecsView {
            on: self.specs.map(|on| on == 1),
            folder: self.specs_folder.clone(),
            variant: self.specs_variant.clone(),
        }
        .said()
    }
}

/// A new project's chief: the harness it runs on and the model of that harness the human
/// picked (CHF-04); one with none runs on its harness's own default.
#[derive(Debug, Clone, PartialEq)]
pub struct NewChief {
    pub harness: String,
    pub model: Option<String>,
    /// The effort it runs its model at; none for its harness's own.
    pub effort: Option<String>,
    /// What its harness's list calls the model, its id (Devin's `swe-2` for the `swe-2-high` its
    /// launch is given), which its card says; none where the list was never read. Held with
    /// the model only: a chief on its harness's default has none.
    pub model_name: Option<String>,
}

/// A member a new project starts with.
#[derive(Debug, Clone, PartialEq)]
pub struct NewMember {
    /// The harness by its kind (`claude-code`).
    pub harness: String,
    /// The model as its harness takes it (Pi's `provider/id`, Devin's exact `model_uid`);
    /// none for its harness's default.
    pub model: Option<String>,
    /// The effort it runs the model at; none where it names none (a Devin `model_uid`
    /// holds its own, which is said here too).
    pub effort: Option<String>,
    /// What its harness's list calls the model; none where the list was never read.
    pub model_name: Option<String>,
    pub designer: bool,
    /// One or more of worker, advisor, reviewer, tester and designer; the first leads.
    pub roles: Vec<String>,
    pub tier: String,
}

/// A project to open: where, under what name, its chief, its staff (the
/// last project's, usually), whether the human approves each message, and
/// what the human says of its wake-up calls and its specs, kept with it.
#[derive(Debug, Clone, PartialEq)]
pub struct NewProject {
    pub directory: String,
    pub name: String,
    pub chief: NewChief,
    pub staff: Vec<NewMember>,
    pub gate: bool,
    /// The wake-up calls it starts with; none leaves them as a project has them
    /// that never set them: off, an hour apart, with the default words.
    pub wake_up: Option<NewWakeUp>,
    /// What it says of specs; none says nothing, and it follows the app's setting.
    pub specs: Option<SpecsChange>,
    /// The rules the human wrote for its staff; none starts it with none.
    pub staff_rules: Option<String>,
    /// The size, in tokens, the human chose for its members' windows to compact
    /// at; none starts it on each harness's own point. A request in JSON that says
    /// nothing of it is read as [`COMPACTION_DEFAULT`] (`from_json`), and one that says
    /// `null` as none; a request built by hand says it either way.
    pub compaction: Option<u32>,
    /// Whether its members run with Claude Code's own advisor tool; false (off) is what
    /// every project starts with.
    pub advisor_tool: bool,
    /// Whether Claude Code's prompt suggestion is left on in its windows, the chief's and the
    /// members'; false (off) is what every project starts with.
    pub prompt_suggestion: bool,
}

impl NewProject {
    /// The request as JSON gives it, read in the order the Node ledger checked it;
    /// what this build added, the calls (`wakeUp`), the specs, the rules for the staff
    /// (`rules`) and the compaction (`compaction`), after it.
    pub fn from_json(value: &Value) -> Result<Self, LedgerError> {
        let directory = model::parse_text(value.get("directory"), "directory", 4096)?;
        let name = parse_name(value.get("name"))?;
        let chief = value.get("chief");
        let harness = model::parse_harness(chief.and_then(|chief| chief.get("harness")))?;
        let chief_model = match chief.and_then(|chief| chief.get("model")) {
            None | Some(Value::Null) => None,
            given => Some(model::parse_model_id(given)?),
        };
        let chief_effort = model::parse_effort(chief.and_then(|chief| chief.get("effort")))?;
        let chief_model_name =
            model::parse_model_name(chief.and_then(|chief| chief.get("modelName")))?;
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
                model: chief_model,
                effort: chief_effort,
                model_name: chief_model_name,
            },
            staff,
            gate,
            wake_up: NewWakeUp::from_opening(value.get("wakeUp"))?,
            specs: SpecsChange::from_opening(value.get("specs"))?,
            staff_rules: parse_staff_rules(value.get("rules"))?,
            compaction: parse_new_compaction(value.get("compaction"))?,
            advisor_tool: match value.get("advisorTool") {
                None => false,
                said => parse_advisor_tool(said)?,
            },
            prompt_suggestion: match value.get("promptSuggestion") {
                None => false,
                said => parse_prompt_suggestion(said)?,
            },
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
        let harness = model::parse_harness(value.get("harness"))?;
        let model = match value.get("model") {
            None | Some(Value::Null) => None,
            given => Some(model::parse_model_id(given)?),
        };
        let effort = model::parse_effort(value.get("effort"))?;
        let model_name = model::parse_model_name(value.get("modelName"))?;
        let designer = match value.get("designer") {
            None => false,
            Some(Value::Bool(designer)) => *designer,
            Some(_) => return Err(designer_refused()),
        };
        let shown =
            crate::handles::model_shown(model_name.as_deref(), model.as_deref(), Some(harness));
        model::require_fitting_roles(&shown, designer, &roles)?;
        // A member whose roles have no tier is given none, and kept in the lowest.
        let tier = match value.get("tier") {
            None | Some(Value::Null) if crate::roles::takes_no_tier(&roles) => {
                crate::roles::NO_TIER
            }
            given => model::parse_tier(given)?,
        };
        Ok(Self {
            harness: harness.to_string(),
            model,
            effort,
            model_name,
            designer,
            roles: roles.into_iter().map(str::to_string).collect(),
            tier: tier.to_string(),
        })
    }

    /// Checks the member: its roles, normalized, its harness, its model and effort, and
    /// its tier, which the human must have picked.
    pub(crate) fn check(&self) -> Result<Vec<&'static str>, LedgerError> {
        let roles = model::require_roles(&self.roles, || model::printed(Some(&json!(self.roles))))?;
        model::require_harness(&self.harness)?;
        if let Some(model) = &self.model {
            model::require_model_id(model)?;
        }
        model::parse_effort(self.effort.as_ref().map(|effort| json!(effort)).as_ref())?;
        model::parse_model_name(self.model_name.as_ref().map(|name| json!(name)).as_ref())?;
        model::require_fitting_roles(&self.shown(), self.designer, &roles)?;
        model::require_tier(&self.tier)?;
        Ok(roles)
    }

    /// What the member's model is called ([`crate::handles::model_shown`]).
    pub fn shown(&self) -> String {
        crate::handles::model_shown(
            self.model_name.as_deref(),
            self.model.as_deref(),
            Some(&self.harness),
        )
    }

    /// The handle the member joins `project` under: made from its model and effort,
    /// unique in the project ([`crate::handles::member_handle`]).
    pub(crate) fn handle(&self, store: &Store, project: i64) -> Result<String, LedgerError> {
        let mut failed = None;
        let handle = crate::handles::model_handle(
            &self.shown(),
            self.effort.as_deref(),
            &self.harness,
            |handle| {
                crate::handles::handle_taken(&store.db, project, handle).unwrap_or_else(|cause| {
                    failed = Some(cause);
                    true
                })
            },
        );
        match failed {
            Some(cause) => Err(cause.into()),
            None => Ok(handle),
        }
    }
}

fn designer_refused() -> LedgerError {
    LedgerError::refused(
        "invalid-designer",
        "an agent is an image agent (true) or not (false)",
    )
}

/// A project with its chief, on the harness and model it runs on, and its staff, and
/// the calls, specs, rules for the staff and compaction the human said for it, kept in
/// the same write: the chief's first launch reads the specs and the rules, and the
/// project is never seen without them. Everything the ledger refuses is refused
/// before the write begins.
pub(crate) fn create_project(
    store: &mut Store,
    request: &NewProject,
) -> Result<ProjectView, LedgerError> {
    model::require_text(&request.directory, "directory", 4096)?;
    require_name(&request.name)?;
    let chief_harness = model::require_harness(&request.chief.harness)?;
    if let Some(chief_model) = &request.chief.model {
        model::require_model_id(chief_model)?;
    }
    model::parse_model_name(
        request
            .chief
            .model_name
            .as_ref()
            .map(|name| json!(name))
            .as_ref(),
    )?;
    let members = request
        .staff
        .iter()
        .map(|member| Ok((member, member.check()?)))
        .collect::<Result<Vec<_>, LedgerError>>()?;
    let wake_up = request
        .wake_up
        .as_ref()
        .map(NewWakeUp::setting)
        .transpose()?;
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
        add_participant(store, id, NewParticipant::coordinator("human", None))?;
        let chief = add_participant(
            store,
            id,
            NewParticipant::coordinator("chief", Some(chief_harness)),
        )?;
        store.db.execute(
            "UPDATE participant SET model = ?, effort = ?, model_name = ? WHERE id = ?",
            params![
                request.chief.model,
                request.chief.effort,
                // A name goes with a model: none for the harness's own default.
                request
                    .chief
                    .model
                    .as_ref()
                    .and(request.chief.model_name.as_ref()),
                chief.id
            ],
        )?;
        for (member, roles) in &members {
            let handle = member.handle(store, id)?;
            add_participant(store, id, NewParticipant::member(&handle, roles, member))?;
        }
        store.log(
            id,
            "project.created",
            json!({ "name": request.name, "directory": request.directory }),
        )?;
        // Said as the project opens, kept and logged as if the human had said
        // them the moment after: a setting that is the default changes nothing.
        if let Some(wake_up) = wake_up {
            keep_wake_up(store, id, wake_up)?;
        }
        if let Some(specs) = &request.specs {
            keep_specs(store, id, specs)?;
        }
        // The setting's own writes, which join this one, so each is checked as it is when
        // the human says it right after, and logged the same.
        if let Some(rules) = &request.staff_rules {
            set_staff_rules(store, id, Some(rules))?;
        }
        if let Some(size) = request.compaction {
            set_compaction(store, id, Some(size))?;
        }
        if request.advisor_tool {
            set_advisor_tool(store, id, true)?;
        }
        if request.prompt_suggestion {
            set_prompt_suggestion(store, id, true)?;
        }
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
    let specs = row.specs_view();
    Ok(Some(ProjectView {
        id: row.id,
        directory: row.directory,
        name: row.name,
        state: row.state,
        gate: row.gate == 1,
        created_at: row.created_at,
        updated_at: row.updated_at,
        participants,
        specs,
        description: row.description,
        meeting: row.meeting_at,
        staff_rules: row.staff_rules,
        compaction: row.compaction,
        advisor_tool: row.advisor_tool == 1,
        prompt_suggestion: row.prompt_suggestion == 1,
        own_hands: row.own_hands == 1,
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

/// Open or suspend a project. A suspended one that opens (the human's Resume)
/// has the tasks its suspension paused at work again, in the same write
/// (`resume_with_project`): its windows come back after the write, and the
/// first of them, the chief's, is never given the news that they are paused.
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
            "UPDATE project SET state = ?, updated_at = ? WHERE id = ?",
            params![state, at, id],
        )?;
        if from != state {
            store.log(id, "project.state", json!({ "from": from, "to": state }))?;
            // A wake-up that waited through a Close is stale when the project
            // comes back: its clock starts again at the Resume.
            let why = if state == "open" {
                "the project was resumed"
            } else {
                "the project was closed"
            };
            withdraw_queued(store, id, why)?;
            if state == "open" {
                resume_with_project(store, id)?;
            } else {
                // A Close ends a meeting: what it held goes in at the Resume.
                end_meeting(store, id, Ended::Closed)?;
            }
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
                 WHERE project_id = ? AND role NOT IN ('human', 'chief') AND member_id IS NULL
                   AND left_at IS NULL",
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

/// End the shown notes of dialogs whose windows died with the daemon. Other
/// asides, including the task's earlier background hold, stay as they were.
fn end_restart_dialogs(store: &mut Store, at: &str) -> Result<(), LedgerError> {
    store.db.execute(
        "UPDATE message SET receipt = json_set(receipt, '$.over', ?1)
         WHERE state = 'read' AND sender_id IS NULL AND kind = 'note'
           AND json_valid(receipt) AND json_extract(receipt, '$.over') IS NULL
           AND json_extract(receipt, '$.shown') LIKE 'dialog reported in @%''s window'",
        [at],
    )?;
    Ok(())
}

/// An already suspended project can still have had a window that refused to close.
fn restart_has_dialogs(store: &Store) -> Result<bool, LedgerError> {
    Ok(store
        .db
        .prepare(
            "SELECT 1 FROM message WHERE kind = 'note' AND state = 'read' AND sender_id IS NULL
               AND json_valid(receipt) AND json_extract(receipt, '$.over') IS NULL
               AND json_extract(receipt, '$.shown') LIKE 'dialog reported in @%''s window'",
        )?
        .exists([])?)
}

/// At daemon start: the panes of every open project died with the previous
/// process, so each becomes suspended, as the human's Close leaves it, and
/// stays so until the human resumes it. Its tasks and other messages are as they
/// were: the board shows them so, and nothing is delivered meanwhile. A dialog
/// of a window that died is over, even though the engine's watch died with it.
pub(crate) fn suspend_for_restart(store: &mut Store) -> Result<Vec<ProjectView>, LedgerError> {
    store.write(|store| {
        let open = store
            .db
            .prepare("SELECT id FROM project WHERE state = 'open' ORDER BY id")?
            .query_map([], |row| row.get::<_, i64>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for (index, id) in open.iter().enumerate() {
            let at = store.at();
            if index == 0 {
                end_restart_dialogs(store, &at)?;
            }
            store.db.execute(
                "UPDATE project SET state = 'suspended', updated_at = ? WHERE id = ?",
                params![at, id],
            )?;
            store.log(
                *id,
                "project.state",
                json!({ "from": "open", "to": "suspended" }),
            )?;
            withdraw_queued(store, *id, "ConsensFlow stopped")?;
        }
        if open.is_empty() && restart_has_dialogs(store)? {
            let at = store.at();
            end_restart_dialogs(store, &at)?;
        }
        // The app's quit ended every meeting: what each held goes in at the Resume.
        end_every_meeting(store)?;
        open.into_iter()
            .map(|id| known_project(store, id))
            .collect()
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

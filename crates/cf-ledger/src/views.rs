//! How the ledger's rows read: the SELECTs that join a row to the handles it
//! refers to, and the views every operation hands back (`src/ledger/views.js`).

use cf_base::json::from_slice_lossy;
use cf_proto::ledger::{ConversationView, MessageView, Need, ParticipantView, TaskView};
use rusqlite::types::Type;
use rusqlite::Row;
use serde_json::Value;

use crate::model::LedgerError;

/// A message with its recipient's handle and role, its sender's handle and its task's number.
pub(crate) const MESSAGE_SELECT: &str = "
  SELECT m.*, r.handle AS recipient, r.role AS recipient_role, s.handle AS sender,
         t.number AS task_number
  FROM message m
  JOIN participant r ON r.id = m.recipient_id
  LEFT JOIN participant s ON s.id = m.sender_id
  LEFT JOIN task t ON t.id = m.task_id";

/// A task with its requester's and assignee's handles, the assignee's member
/// when it is a session, and the tasks it needs with their states.
pub(crate) const TASK_SELECT: &str = "
  SELECT t.*, q.handle AS requester, a.handle AS assignee,
         am.handle AS assignee_member, a.left_at AS assignee_left_at,
         (SELECT json_group_array(json_object('number', d.number, 'state', d.state))
            FROM (SELECT d.number, d.state FROM task_need n JOIN task d ON d.id = n.needs_id
                  WHERE n.task_id = t.id ORDER BY d.number) d) AS needs
  FROM task t
  JOIN participant q ON q.id = t.requester_id
  LEFT JOIN participant a ON a.id = t.assignee_id
  LEFT JOIN participant am ON am.id = a.member_id";

/// A participant with its member's handle, when it is a member's session.
pub(crate) const PARTICIPANT_SELECT: &str = "
  SELECT p.*, m.handle AS member_handle
  FROM participant p
  LEFT JOIN participant m ON m.id = p.member_id";

/// A participant's row, as `PARTICIPANT_SELECT` reads it.
pub(crate) struct ParticipantRow {
    pub(crate) id: i64,
    pub(crate) project_id: i64,
    pub(crate) handle: String,
    pub(crate) role: String,
    pub(crate) roles: String,
    pub(crate) agent: Option<String>,
    pub(crate) harness: Option<String>,
    pub(crate) tier: Option<String>,
    pub(crate) member_id: Option<i64>,
    pub(crate) out_until: Option<String>,
    pub(crate) out_since: Option<String>,
    pub(crate) created_at: String,
    pub(crate) left_at: Option<String>,
    pub(crate) switched_from_harness: Option<String>,
    pub(crate) switched_from_agent: Option<String>,
    pub(crate) switched_from_cut: i64,
    pub(crate) designer: i64,
    pub(crate) member_handle: Option<String>,
}

impl ParticipantRow {
    pub(crate) fn read(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get("id")?,
            project_id: row.get("project_id")?,
            handle: row.get("handle")?,
            role: row.get("role")?,
            roles: row.get("roles")?,
            agent: row.get("agent")?,
            harness: row.get("harness")?,
            tier: row.get("tier")?,
            member_id: row.get("member_id")?,
            out_until: row.get("out_until")?,
            out_since: row.get("out_since")?,
            created_at: row.get("created_at")?,
            left_at: row.get("left_at")?,
            switched_from_harness: row.get("switched_from_harness")?,
            switched_from_agent: row.get("switched_from_agent")?,
            switched_from_cut: row.get("switched_from_cut")?,
            designer: row.get("designer")?,
            member_handle: row.get("member_handle")?,
        })
    }
}

pub(crate) fn participant_view(row: &ParticipantRow) -> Result<ParticipantView, LedgerError> {
    // A member handle that says something makes this a session: its own name
    // is what follows the member's handle and its hyphen.
    let session = row
        .member_handle
        .as_ref()
        .filter(|handle| !handle.is_empty())
        .map(|handle| {
            row.handle
                .chars()
                .skip(handle.chars().count() + 1)
                .collect()
        });
    Ok(ParticipantView {
        id: row.id,
        project_id: row.project_id,
        handle: row.handle.clone(),
        role: row.role.clone(),
        agent: row.agent.clone(),
        harness: row.harness.clone(),
        designer: row.designer == 1,
        created_at: row.created_at.clone(),
        left_at: row.left_at.clone(),
        tier: row.tier.clone(),
        roles: serde_json::from_str(&row.roles)?,
        out_until: row.out_until.clone(),
        out_since: row.out_since.clone(),
        member_id: row.member_id,
        member: row.member_handle.clone(),
        session,
    })
}

/// A task's row, as `SELECT * FROM task` reads it: what the operations go by.
pub(crate) struct TaskRow {
    pub(crate) id: i64,
    pub(crate) project_id: i64,
    pub(crate) number: i64,
    pub(crate) assignee_id: Option<i64>,
    pub(crate) state: String,
    pub(crate) pool: Option<String>,
    pub(crate) tier: Option<String>,
    pub(crate) taken_from_id: Option<i64>,
}

impl TaskRow {
    pub(crate) fn read(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get("id")?,
            project_id: row.get("project_id")?,
            number: row.get("number")?,
            assignee_id: row.get("assignee_id")?,
            state: row.get("state")?,
            pool: row.get("pool")?,
            tier: row.get("tier")?,
            taken_from_id: row.get("taken_from_id")?,
        })
    }
}

/// A conversation's row.
pub(crate) fn conversation_view(row: &Row<'_>) -> rusqlite::Result<ConversationView> {
    Ok(ConversationView {
        id: row.get("id")?,
        participant_id: row.get("participant_id")?,
        harness: row.get("harness")?,
        native_session: row.get("native_session")?,
        started_at: row.get("started_at")?,
        ended_at: row.get("ended_at")?,
    })
}

/// A row `TASK_SELECT` read.
pub(crate) fn task_view(row: &Row<'_>) -> rusqlite::Result<TaskView> {
    let needs: Vec<Need> = serde_json::from_value(stored_json(row, "needs")?)
        .map_err(|cause| unreadable(row, "needs", cause))?;
    let assignee: Option<String> = row.get("assignee")?;
    // A session is the assignee whose member's handle says something.
    let member: Option<String> = row.get("assignee_member")?;
    Ok(TaskView {
        id: row.get("id")?,
        project_id: row.get("project_id")?,
        number: row.get("number")?,
        title: row.get("title")?,
        body: row.get("body")?,
        state: row.get("state")?,
        requester: row.get("requester")?,
        session: member
            .filter(|member| !member.is_empty())
            .and(assignee.clone()),
        assignee,
        pool: row.get("pool")?,
        tier: row.get("tier")?,
        purpose: row.get("purpose")?,
        blocked_by: needs
            .iter()
            .filter(|need| need.state != "accepted")
            .map(|need| need.number)
            .collect(),
        needs,
        held_until: row.get("held_until")?,
        paused_at: row.get("paused_at")?,
        deleted_at: row.get("deleted_at")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

/// A row `MESSAGE_SELECT` read.
pub(crate) fn message_view(row: &Row<'_>) -> rusqlite::Result<MessageView> {
    // What a column holds as JSON text; none (SQL NULL) is null.
    let json = |column: &str| -> rusqlite::Result<Value> {
        match row.get::<_, Option<String>>(column)? {
            None => Ok(Value::Null),
            Some(_) => stored_json(row, column),
        }
    };
    Ok(MessageView {
        id: row.get("id")?,
        project_id: row.get("project_id")?,
        recipient: row.get("recipient")?,
        recipient_id: row.get("recipient_id")?,
        recipient_role: row.get("recipient_role")?,
        sender: row.get("sender")?,
        kind: row.get("kind")?,
        task_number: row.get("task_number")?,
        reply_to: row.get("reply_to")?,
        body: row.get("body")?,
        state: row.get("state")?,
        attempts: row.get("attempts")?,
        reason: row.get("reason")?,
        receipt: json("receipt")?,
        questions: json("questions")?,
        choices: json("choices")?,
        urgent: row.get::<_, i64>("urgent")? == 1,
        created_at: row.get("created_at")?,
        delivered_at: row.get("delivered_at")?,
    })
}

/// A column's JSON text, read as the ledger wrote it: a lone surrogate
/// `JSON.stringify` escaped reads as U+FFFD.
fn stored_json(row: &Row<'_>, column: &str) -> rusqlite::Result<Value> {
    let text: String = row.get(column)?;
    from_slice_lossy(text.as_bytes()).map_err(|cause| unreadable(row, column, cause))
}

/// A column whose JSON does not read as the view needs it.
fn unreadable(row: &Row<'_>, column: &str, cause: serde_json::Error) -> rusqlite::Error {
    let index = row.as_ref().column_index(column).unwrap_or_default();
    rusqlite::Error::FromSqlConversionFailure(index, Type::Text, Box::new(cause))
}

//! What the ledger answers, as it crosses to the API and the page: each
//! view with its fields in the order the Node ledger wrote them
//! (`src/ledger/views.js`, `projects.js`), so its JSON reads as it did.

use serde::Serialize;
use serde_json::Value;

/// A participant of a project: the human, the chief, a member, or a member's session.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParticipantView {
    pub id: i64,
    pub project_id: i64,
    pub handle: String,
    pub role: String,
    pub agent: Option<String>,
    pub harness: Option<String>,
    pub designer: bool,
    pub created_at: String,
    pub left_at: Option<String>,
    pub tier: Option<String>,
    pub roles: Vec<String>,
    pub out_until: Option<String>,
    pub out_since: Option<String>,
    pub member_id: Option<i64>,
    /// The member a session belongs to.
    pub member: Option<String>,
    /// A session's own name, after its member's handle.
    pub session: Option<String>,
}

/// A project with the participants still in it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectView {
    pub id: i64,
    pub directory: String,
    pub name: String,
    pub state: String,
    pub gate: bool,
    pub resume_on_start: bool,
    pub created_at: String,
    pub updated_at: String,
    pub participants: Vec<ParticipantView>,
}

/// What a deleted project took with it, for the one line the trace keeps.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeletedProject {
    pub id: i64,
    pub name: String,
    pub directory: String,
    pub created_at: String,
    pub members: i64,
    pub sessions: i64,
    pub tasks: i64,
    pub messages: i64,
}

/// One event of a project's log.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EventView {
    pub id: i64,
    pub project_id: i64,
    pub at: String,
    pub kind: String,
    pub data: Value,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_a_participant_with_its_fields_in_the_node_ledger_s_order() {
        let view = ParticipantView {
            id: 2,
            project_id: 1,
            handle: "zeus-amber-pine".into(),
            role: "worker".into(),
            agent: Some("zeus".into()),
            harness: Some("pi".into()),
            designer: false,
            created_at: "2026-09-19T10:00:01.000Z".into(),
            left_at: None,
            tier: Some("standard".into()),
            roles: vec!["worker".into()],
            out_until: None,
            out_since: None,
            member_id: Some(1),
            member: Some("zeus".into()),
            session: Some("amber-pine".into()),
        };
        assert_eq!(
            serde_json::to_string(&view).unwrap(),
            r#"{"id":2,"projectId":1,"handle":"zeus-amber-pine","role":"worker","agent":"zeus","harness":"pi","designer":false,"createdAt":"2026-09-19T10:00:01.000Z","leftAt":null,"tier":"standard","roles":["worker"],"outUntil":null,"outSince":null,"memberId":1,"member":"zeus","session":"amber-pine"}"#
        );
    }
}

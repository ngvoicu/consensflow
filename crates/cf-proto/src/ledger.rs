//! What the ledger answers, as it crosses to the API and the page: each
//! view with its fields in the order the Node ledger wrote them
//! (`src/ledger/views.js`, `projects.js`), so its JSON reads as it did.

use serde::{Deserialize, Serialize};
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

/// What a member's tier became when the roster's catalog moved its model.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TierChange {
    pub project: i64,
    pub handle: String,
    pub from: Option<String>,
    pub to: String,
}

/// A member who left the staff, and the tasks cancelled with it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RemovedMember {
    pub member: ParticipantView,
    pub cancelled: Vec<i64>,
}

/// A member of the newest project's staff, as a new project starts from it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StaffMember {
    pub agent: Option<String>,
    pub harness: Option<String>,
    pub role: String,
    pub roles: Vec<String>,
}

/// An active member of one role, with what the daemon ranks it by.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemberView {
    pub id: i64,
    pub handle: String,
    pub agent: Option<String>,
    pub harness: Option<String>,
    pub tier: Option<String>,
    pub roles: Vec<String>,
    /// How many tasks it and its sessions have taken.
    pub taken: i64,
    /// How many of its windows have a task on their hands now.
    pub sessions: i64,
    pub out_until: Option<String>,
}

/// A member an open task may go to, and whether the task was taken from it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    #[serde(flatten)]
    pub member: MemberView,
    pub had_it: bool,
}

/// A participant's native conversation: the harness it runs on, the
/// harness's own session once it is known, and when it started and ended.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationView {
    pub id: i64,
    pub participant_id: i64,
    pub harness: String,
    pub native_session: Option<String>,
    pub started_at: String,
    pub ended_at: Option<String>,
}

/// One item of the copy ConsensFlow keeps of a conversation.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HistoryItem {
    pub id: String,
    pub role: String,
    pub text: String,
    pub complete: bool,
    pub at: Option<String>,
}

/// An earlier conversation of the chief, with its copied items in order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ChiefConversation {
    #[serde(flatten)]
    pub conversation: ConversationView,
    pub items: Vec<HistoryItem>,
}

/// One item a task's windows wrote, and the conversation it is from.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TranscriptItem {
    pub id: String,
    pub conversation: i64,
    pub role: String,
    pub text: String,
    pub complete: bool,
    pub at: Option<String>,
}

/// What a task's windows wrote: how many items in all, and the last ones asked for.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TaskTranscript {
    pub total: usize,
    pub items: Vec<TranscriptItem>,
}

/// What the chief ran on before the latest Switch chief.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SwitchedFrom {
    pub harness: String,
    pub agent: Option<String>,
}

/// The project's latest Switch chief: what the chief was switched from, and
/// whether its turn was cut.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LastSwitch {
    pub from: SwitchedFrom,
    pub cut: bool,
}

/// A task another task needs, with its state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Need {
    pub number: i64,
    pub state: String,
}

/// A task on the board.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskView {
    pub id: i64,
    pub project_id: i64,
    pub number: i64,
    pub title: String,
    pub body: String,
    pub state: String,
    pub requester: String,
    pub assignee: Option<String>,
    pub pool: Option<String>,
    pub tier: Option<String>,
    pub purpose: Option<String>,
    /// The assignee, when it is a member's session.
    pub session: Option<String>,
    pub needs: Vec<Need>,
    /// The numbers of the tasks it needs that are not accepted yet.
    pub blocked_by: Vec<i64>,
    pub held_until: Option<String>,
    pub paused_at: Option<String>,
    pub deleted_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// A message in an inbox.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageView {
    pub id: i64,
    pub project_id: i64,
    pub recipient: String,
    pub recipient_id: i64,
    pub recipient_role: String,
    pub sender: Option<String>,
    pub kind: String,
    pub task_number: Option<i64>,
    pub reply_to: Option<i64>,
    pub body: String,
    pub state: String,
    pub attempts: i64,
    pub reason: Option<String>,
    /// What the delivery left as proof, as it was stored.
    pub receipt: Value,
    pub questions: Value,
    pub choices: Value,
    pub urgent: bool,
    pub created_at: String,
    pub delivered_at: Option<String>,
}

/// What waits on the chief, for a chief that takes over: members' questions
/// to it without an answer, results it has not decided on, and its own
/// unfinished tasks.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ChiefOpenWork {
    pub questions: Vec<MessageView>,
    pub results: Vec<TaskView>,
    pub own: Vec<TaskView>,
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

    #[test]
    fn writes_a_candidate_as_its_member_with_had_it_last() {
        let candidate = Candidate {
            member: MemberView {
                id: 3,
                handle: "zeus".into(),
                agent: Some("zeus".into()),
                harness: Some("pi".into()),
                tier: Some("standard".into()),
                roles: vec!["worker".into()],
                taken: 2,
                sessions: 1,
                out_until: None,
            },
            had_it: true,
        };
        assert_eq!(
            serde_json::to_string(&candidate).unwrap(),
            r#"{"id":3,"handle":"zeus","agent":"zeus","harness":"pi","tier":"standard","roles":["worker"],"taken":2,"sessions":1,"outUntil":null,"hadIt":true}"#
        );
    }

    #[test]
    fn writes_a_chief_conversation_as_its_conversation_with_its_items_last() {
        let conversation = ChiefConversation {
            conversation: ConversationView {
                id: 4,
                participant_id: 2,
                harness: "codex".into(),
                native_session: Some("s-1".into()),
                started_at: "2026-09-19T10:00:01.000Z".into(),
                ended_at: Some("2026-09-19T10:00:02.000Z".into()),
            },
            items: vec![HistoryItem {
                id: "i-1".into(),
                role: "user".into(),
                text: "hello".into(),
                complete: true,
                at: None,
            }],
        };
        assert_eq!(
            serde_json::to_string(&conversation).unwrap(),
            r#"{"id":4,"participantId":2,"harness":"codex","nativeSession":"s-1","startedAt":"2026-09-19T10:00:01.000Z","endedAt":"2026-09-19T10:00:02.000Z","items":[{"id":"i-1","role":"user","text":"hello","complete":true,"at":null}]}"#
        );
    }
}

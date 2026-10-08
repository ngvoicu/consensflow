//! What the ledger answers, as it crosses to the API and the page: each view
//! with its fields in the order the Node ledger wrote them, so its JSON reads
//! as it did.

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

/// A task with its whole thread, oldest first.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TaskThread {
    #[serde(flatten)]
    pub task: TaskView,
    pub messages: Vec<MessageView>,
}

/// A new task, the message that took it to its window (none while it waits
/// on the board), and the tier asked when the task went to the nearest one
/// somebody on the staff holds.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TaskCreated {
    pub task: TaskView,
    pub message: Option<MessageView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asked: Option<String>,
}

/// A task that moved, and the message that moves it on (none when it went
/// back to the board).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TaskMoved {
    pub task: TaskView,
    pub message: Option<MessageView>,
}

/// A task taken back to the board.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TaskReleased {
    pub task: TaskView,
}

/// A held task whose time has come.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HeldTask {
    pub project_id: i64,
    pub number: i64,
    pub assignee_id: Option<i64>,
}

/// A message whose delivery has begun, and the rows that ride in its paste
/// (the ones it carries, still waiting, by id). The ledger never serialises
/// it: the engine writes the paste from it.
#[derive(Debug, Clone, PartialEq)]
pub struct Begun {
    pub message: MessageView,
    pub carried: Vec<MessageView>,
}

/// What a door finds when it asks for the answer to its question: the
/// answer, now claimed for it (boxed: it is the one variant that holds a
/// row); none yet; or the door is shut.
#[derive(Debug, Clone, PartialEq)]
pub enum Claim {
    Answered(Box<MessageView>),
    Waiting,
    Closed,
}

/// The stops asked of a window's task: how many its pauses have asked so
/// far, which is the identity of the last. Two pauses in one millisecond are
/// two stops.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stop {
    pub task_id: i64,
    pub number: i64,
    pub seq: i64,
}

/// One option of a question, as a harness's own question tool offers it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuestionOption {
    pub label: String,
    pub description: Option<String>,
}

/// A question with options, as a harness's own question tool asks it: its
/// text, a short header, its options, and whether several may be picked.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Question {
    pub question: String,
    pub header: String,
    pub options: Vec<QuestionOption>,
    pub multiple: bool,
}

/// A task as its card on the board shows it: the task without its brief
/// (the drawer reads that with the task), and the first line of its latest
/// result, none before there is one.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskCard {
    pub id: i64,
    pub project_id: i64,
    pub number: i64,
    pub title: String,
    pub state: String,
    pub requester: String,
    pub assignee: Option<String>,
    pub pool: Option<String>,
    pub tier: Option<String>,
    pub purpose: Option<String>,
    pub session: Option<String>,
    pub needs: Vec<Need>,
    pub blocked_by: Vec<i64>,
    pub held_until: Option<String>,
    pub paused_at: Option<String>,
    pub deleted_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub result: Option<String>,
}

impl TaskCard {
    /// `task`'s card, `result` the first line of its latest result.
    pub fn of(task: TaskView, result: Option<String>) -> Self {
        Self {
            id: task.id,
            project_id: task.project_id,
            number: task.number,
            title: task.title,
            state: task.state,
            requester: task.requester,
            assignee: task.assignee,
            pool: task.pool,
            tier: task.tier,
            purpose: task.purpose,
            session: task.session,
            needs: task.needs,
            blocked_by: task.blocked_by,
            held_until: task.held_until,
            paused_at: task.paused_at,
            deleted_at: task.deleted_at,
            created_at: task.created_at,
            updated_at: task.updated_at,
            result,
        }
    }
}

/// One participant's lane on the board, with the tasks on it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Lane {
    pub participant: ParticipantView,
    pub tasks: Vec<TaskCard>,
}

/// The board as the page reads it: the project, the tasks no lane has, each
/// participant's lane, and what waits for the human's approval.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Board {
    pub project: ProjectView,
    /// Every task no lane has, whatever its state: one waiting for a member,
    /// one paused, called off or failed before any member had it, and one of a
    /// member who left the staff. A task the human deleted is on no list.
    pub open: Vec<TaskCard>,
    pub lanes: Vec<Lane>,
    pub gated: Vec<MessageView>,
}

fn is_false(value: &bool) -> bool {
    !value
}

/// A view whose body may have been cut to fit a frame, and says so.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Cut<T> {
    #[serde(flatten)]
    pub view: T,
    #[serde(rename = "bodyCut", skip_serializing_if = "is_false")]
    pub body_cut: bool,
}

/// A task with its thread as the page reads it in one frame: bodies cut
/// to fit, and how many of its earliest messages were left out, if any.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskThatFits {
    #[serde(flatten)]
    pub task: Cut<TaskView>,
    pub messages: Vec<Cut<MessageView>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub messages_left_out: Option<usize>,
}

/// The newest of a window's transcript that fit in a frame, in order: how
/// many items there are, and how many came.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LatestTranscript {
    pub items: Vec<TranscriptItem>,
    pub total: usize,
    pub shown: usize,
}

/// A participant's newest messages that fit in a frame, newest first: how
/// many there are, and how many came.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LatestMessages {
    pub messages: Vec<MessageView>,
    pub total: i64,
    pub shown: usize,
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

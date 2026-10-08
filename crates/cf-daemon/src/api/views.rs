//! The agents' API's wire views: what a route says of a row, in the fields and
//! the order Node said them, because `cf --json` prints an answer with its keys
//! as they came. The views a route needs are added here as the routes land.

use cf_base::text::utf16_prefix;
use cf_ledger::{MessageView, TaskCard, TaskView};
use cf_proto::ledger::Need;
use serde::Serialize;
use serde_json::Value;

use super::answer::Failure;

/// How much of a message's first line its summary shows, in UTF-16 units.
const PREVIEW_UNITS: usize = 160;

/// A view as the JSON value an answer carries, its fields in the order they
/// are declared. A view that would not serialize is an internal failure,
/// which none does.
pub fn value<T: Serialize>(view: &T) -> Result<Value, Failure> {
    serde_json::to_value(view).map_err(|failed| Failure::Internal(failed.to_string()))
}

/// A task, summarised (`summary`): what a window needs to tell its tasks
/// apart, and not the brief. Node's also read `task.kind`, which no task has,
/// so `JSON.stringify` left it out: it is not here.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskSummary {
    pub number: i64,
    pub title: String,
    pub state: String,
    pub requester: String,
    pub assignee: Option<String>,
    pub pool: Option<String>,
    pub tier: Option<String>,
    pub needs: Vec<Need>,
    pub blocked_by: Vec<i64>,
    pub updated_at: String,
}

impl From<&TaskView> for TaskSummary {
    fn from(task: &TaskView) -> Self {
        Self {
            number: task.number,
            title: task.title.clone(),
            state: task.state.clone(),
            requester: task.requester.clone(),
            assignee: task.assignee.clone(),
            pool: task.pool.clone(),
            tier: task.tier.clone(),
            needs: task.needs.clone(),
            blocked_by: task.blocked_by.clone(),
            updated_at: task.updated_at.clone(),
        }
    }
}

/// A card on the board says as much as a task does.
impl From<&TaskCard> for TaskSummary {
    fn from(card: &TaskCard) -> Self {
        Self {
            number: card.number,
            title: card.title.clone(),
            state: card.state.clone(),
            requester: card.requester.clone(),
            assignee: card.assignee.clone(),
            pool: card.pool.clone(),
            tier: card.tier.clone(),
            needs: card.needs.clone(),
            blocked_by: card.blocked_by.clone(),
            updated_at: card.updated_at.clone(),
        }
    }
}

/// A message, summarised (`messageSummary`): its first line, cut at 160
/// units, stands for its body.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageSummary {
    pub id: i64,
    pub kind: String,
    pub state: String,
    pub sender: Option<String>,
    pub recipient: String,
    pub task: Option<i64>,
    pub preview: String,
    pub questions: Value,
    pub choices: Value,
    pub created_at: String,
}

impl From<&MessageView> for MessageSummary {
    fn from(message: &MessageView) -> Self {
        let first = message.body.split('\n').next().unwrap_or_default();
        Self {
            id: message.id,
            kind: message.kind.clone(),
            state: message.state.clone(),
            sender: message.sender.clone(),
            recipient: message.recipient.clone(),
            task: message.task_number,
            preview: utf16_prefix(first, PREVIEW_UNITS).into_owned(),
            questions: message.questions.clone(),
            choices: message.choices.clone(),
            created_at: message.created_at.clone(),
        }
    }
}

/// The answer to a question, as a door reads it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Answered {
    pub id: i64,
    pub from: Option<String>,
    pub body: String,
    pub choices: Value,
}

impl From<&MessageView> for Answered {
    fn from(answer: &MessageView) -> Self {
        Self {
            id: answer.id,
            from: answer.sender.clone(),
            body: answer.body.clone(),
            choices: answer.choices.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn message(body: &str) -> MessageView {
        MessageView {
            id: 12,
            project_id: 1,
            recipient: "chief".to_owned(),
            recipient_id: 3,
            recipient_role: "chief".to_owned(),
            sender: Some("zeus".to_owned()),
            kind: "question".to_owned(),
            task_number: Some(4),
            reply_to: None,
            body: body.to_owned(),
            state: "delivered".to_owned(),
            attempts: 1,
            reason: None,
            receipt: Value::Null,
            questions: json!([{ "question": "Which?" }]),
            choices: Value::Null,
            urgent: false,
            created_at: "2026-10-05T09:00:00.000Z".to_owned(),
            delivered_at: None,
        }
    }

    fn task() -> TaskView {
        TaskView {
            id: 9,
            project_id: 1,
            number: 4,
            title: "Parser".to_owned(),
            body: "The whole brief".to_owned(),
            state: "waiting".to_owned(),
            requester: "chief".to_owned(),
            assignee: Some("zeus".to_owned()),
            pool: Some("worker".to_owned()),
            tier: Some("standard".to_owned()),
            purpose: Some("hard-problem".to_owned()),
            session: Some("amber-pine".to_owned()),
            needs: vec![Need {
                number: 2,
                state: "done".to_owned(),
            }],
            blocked_by: vec![3],
            held_until: None,
            paused_at: None,
            deleted_at: None,
            created_at: "2026-10-05T08:00:00.000Z".to_owned(),
            updated_at: "2026-10-05T09:00:00.000Z".to_owned(),
        }
    }

    const SUMMARY: &str = r#"{"number":4,"title":"Parser","state":"waiting","requester":"chief","assignee":"zeus","pool":"worker","tier":"standard","needs":[{"number":2,"state":"done"}],"blockedBy":[3],"updatedAt":"2026-10-05T09:00:00.000Z"}"#;

    #[test]
    fn a_task_is_summarised_in_the_order_node_said_it_with_no_brief_and_no_kind() {
        // Node's `summary` read `task.kind` too, which a task has none of, so
        // `JSON.stringify` left it out.
        assert_eq!(
            serde_json::to_string(&TaskSummary::from(&task())).unwrap(),
            SUMMARY
        );
    }

    #[test]
    fn a_card_on_the_board_is_summarised_as_its_task_is() {
        let card = TaskCard::of(task(), Some("Done.".to_owned()));
        assert_eq!(
            serde_json::to_string(&TaskSummary::from(&card)).unwrap(),
            SUMMARY
        );
    }

    #[test]
    fn a_summary_is_said_in_the_order_node_said_it() {
        let written =
            serde_json::to_string(&MessageSummary::from(&message("Which?\nmore"))).unwrap();
        assert_eq!(
            written,
            r#"{"id":12,"kind":"question","state":"delivered","sender":"zeus","recipient":"chief","task":4,"preview":"Which?","questions":[{"question":"Which?"}],"choices":null,"createdAt":"2026-10-05T09:00:00.000Z"}"#
        );
    }

    #[test]
    fn the_preview_is_the_first_line_cut_at_160_utf16_units() {
        let long = "a".repeat(200);
        assert_eq!(
            MessageSummary::from(&message(&long)).preview,
            "a".repeat(160)
        );
        assert_eq!(MessageSummary::from(&message("")).preview, "");
        assert_eq!(MessageSummary::from(&message("\nsecond")).preview, "");
        // 80 emoji are 160 units; a ninety-first would be cut through.
        let emoji = "\u{1F600}".repeat(90);
        assert_eq!(
            MessageSummary::from(&message(&emoji)).preview,
            "\u{1F600}".repeat(80)
        );
    }

    #[test]
    fn an_answer_is_its_id_its_sender_its_words_and_its_choices() {
        let mut answer = message("Yes");
        answer.choices = json!(["A"]);
        let written = serde_json::to_string(&Answered::from(&answer)).unwrap();
        assert_eq!(
            written,
            r#"{"id":12,"from":"zeus","body":"Yes","choices":["A"]}"#
        );
    }
}

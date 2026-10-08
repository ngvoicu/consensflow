//! What the tests build the ledger's rows with, and read the golden by. The
//! views have no `Deserialize` and no constructor, so each is made here, from
//! the few fields a text reads and fixed values for the rest.

use std::fs;
use std::path::Path;
use std::sync::LazyLock;

use cf_base::refusal::Refusal;
use cf_catalog::{WorkTier, WORK_TIERS};
use cf_engine::roles::StaffRow;
use cf_proto::ledger::{
    ChiefConversation, ConversationView, HistoryItem, MessageView, ParticipantView, ProjectView,
    TaskView,
};
use serde_json::Value;

/// `tests/goldens/text.json`, recorded from Node and fixed since.
pub static GOLDEN: LazyLock<Value> = LazyLock::new(|| {
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/goldens/text.json");
    serde_json::from_str(&fs::read_to_string(file).unwrap()).unwrap()
});

/// A table of the golden.
pub fn table(name: &str) -> &'static [Value] {
    GOLDEN[name].as_array().unwrap()
}

/// Fails with where `actual` and `expected` first differ, which a diff of two
/// pages would bury.
#[track_caller]
pub fn assert_same_text(actual: &str, expected: &str, what: &str) {
    if actual == expected {
        return;
    }
    let at = actual
        .char_indices()
        .zip(expected.chars())
        .find(|((_, a), e)| a != e)
        .map_or_else(|| actual.len().min(expected.len()), |((at, _), _)| at);
    let around = |text: &str| {
        let from = text.floor_char_boundary(at.saturating_sub(40));
        let to = text.ceil_char_boundary((at + 40).min(text.len()));
        format!("{:?} ({} bytes)", &text[from..to], text.len())
    };
    panic!(
        "{what} differs from the golden at byte {at}:\n  rust:   {}\n  golden: {}",
        around(actual),
        around(expected)
    );
}

/// A text a row holds: its own, or what it is a repeat of.
pub fn expand(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Object(fields) => fields["repeat"]
            .as_array()
            .unwrap()
            .iter()
            .map(|part| {
                let unit = part[0].as_str().unwrap();
                unit.repeat(usize::try_from(part[1].as_u64().unwrap()).unwrap())
            })
            .collect(),
        other => panic!("no text: {other}"),
    }
}

/// A field that is a text or none.
fn text(row: &Value, field: &str) -> Option<String> {
    row.get(field).and_then(Value::as_str).map(str::to_owned)
}

fn texts(row: &Value, field: &str) -> Vec<String> {
    row[field]
        .as_array()
        .unwrap()
        .iter()
        .map(|each| each.as_str().unwrap().to_owned())
        .collect()
}

/// A message in an inbox, the way a test says it: the fields a text reads.
pub fn message(
    id: i64,
    kind: &str,
    sender: Option<&str>,
    task_number: Option<i64>,
    body: &str,
) -> MessageView {
    MessageView {
        id,
        project_id: 1,
        recipient: "chief".to_owned(),
        recipient_id: 1,
        recipient_role: "chief".to_owned(),
        sender: sender.map(str::to_owned),
        kind: kind.to_owned(),
        task_number,
        reply_to: None,
        body: body.to_owned(),
        state: "delivered".to_owned(),
        attempts: 1,
        reason: None,
        receipt: Value::Null,
        questions: Value::Null,
        choices: Value::Null,
        urgent: false,
        created_at: "2026-10-01T09:00:00.000Z".to_owned(),
        delivered_at: None,
    }
}

/// A message as a golden row holds it.
pub fn message_of(row: &Value) -> MessageView {
    MessageView {
        questions: row.get("questions").cloned().unwrap_or(Value::Null),
        urgent: row.get("urgent").and_then(Value::as_bool).unwrap_or(false),
        ..message(
            row["id"].as_i64().unwrap(),
            row["kind"].as_str().unwrap(),
            row["sender"].as_str(),
            row.get("taskNumber").and_then(Value::as_i64),
            &expand(&row["body"]),
        )
    }
}

/// A task on the board, the way a test says it.
pub fn task(number: i64, title: &str, assignee: Option<&str>, state: &str) -> TaskView {
    TaskView {
        id: number,
        project_id: 1,
        number,
        title: title.to_owned(),
        body: String::new(),
        state: state.to_owned(),
        requester: "chief".to_owned(),
        assignee: assignee.map(str::to_owned),
        pool: None,
        tier: None,
        purpose: None,
        session: None,
        needs: Vec::new(),
        blocked_by: Vec::new(),
        held_until: None,
        paused_at: None,
        deleted_at: None,
        created_at: "2026-10-01T09:00:00.000Z".to_owned(),
        updated_at: "2026-10-01T09:00:00.000Z".to_owned(),
    }
}

/// A task as a golden row holds it.
pub fn task_of(row: &Value) -> TaskView {
    task(
        row["number"].as_i64().unwrap(),
        row["title"].as_str().unwrap(),
        row["assignee"].as_str(),
        row["state"].as_str().unwrap(),
    )
}

/// A participant, the way a test says it: a chief on a harness, or none.
pub fn seat(harness: Option<&str>, agent: Option<&str>) -> ParticipantView {
    ParticipantView {
        id: 2,
        project_id: 1,
        handle: "chief".to_owned(),
        role: "chief".to_owned(),
        agent: agent.map(str::to_owned),
        harness: harness.map(str::to_owned),
        designer: false,
        created_at: "2026-09-19T10:00:01.000Z".to_owned(),
        left_at: None,
        tier: None,
        roles: Vec::new(),
        out_until: None,
        out_since: None,
        member_id: None,
        member: None,
        session: None,
    }
}

/// A participant as a golden row holds it, whole.
pub fn participant_of(row: &Value) -> ParticipantView {
    ParticipantView {
        id: row["id"].as_i64().unwrap(),
        project_id: row["projectId"].as_i64().unwrap(),
        handle: row["handle"].as_str().unwrap().to_owned(),
        role: row["role"].as_str().unwrap().to_owned(),
        agent: text(row, "agent"),
        harness: text(row, "harness"),
        designer: row["designer"].as_bool().unwrap(),
        created_at: row["createdAt"].as_str().unwrap().to_owned(),
        left_at: text(row, "leftAt"),
        tier: text(row, "tier"),
        roles: texts(row, "roles"),
        out_until: text(row, "outUntil"),
        out_since: text(row, "outSince"),
        member_id: row["memberId"].as_i64(),
        member: text(row, "member"),
        session: text(row, "session"),
    }
}

/// A project with these participants.
pub fn project_of(participants: Vec<ParticipantView>) -> ProjectView {
    ProjectView {
        id: 1,
        directory: "/work".to_owned(),
        name: "work".to_owned(),
        state: "open".to_owned(),
        gate: false,
        resume_on_start: false,
        created_at: "2026-09-19T10:00:01.000Z".to_owned(),
        updated_at: "2026-09-19T10:00:01.000Z".to_owned(),
        participants,
    }
}

/// One conversation of the chief that ended, with its items: each a role and
/// its text, complete.
pub fn conversation<R: AsRef<str>, T: AsRef<str>>(
    n: i64,
    harness: &str,
    items: impl IntoIterator<Item = (R, T)>,
) -> ChiefConversation {
    ChiefConversation {
        conversation: ConversationView {
            id: n,
            participant_id: 2,
            harness: harness.to_owned(),
            native_session: Some(format!("s-{n}")),
            started_at: format!("2026-10-0{n}T09:00:00.000Z"),
            ended_at: Some(format!("2026-10-0{n}T17:00:00.000Z")),
        },
        items: items
            .into_iter()
            .enumerate()
            .map(|(index, (role, text))| HistoryItem {
                id: format!("{n}-{index}"),
                role: role.as_ref().to_owned(),
                text: text.as_ref().to_owned(),
                complete: true,
                at: None,
            })
            .collect(),
    }
}

/// A conversation as a golden row holds it.
pub fn conversation_of(row: &Value) -> ChiefConversation {
    ChiefConversation {
        conversation: ConversationView {
            id: row["id"].as_i64().unwrap(),
            participant_id: row["participantId"].as_i64().unwrap(),
            harness: row["harness"].as_str().unwrap().to_owned(),
            native_session: text(row, "nativeSession"),
            started_at: row["startedAt"].as_str().unwrap().to_owned(),
            ended_at: text(row, "endedAt"),
        },
        items: row["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| HistoryItem {
                id: item["id"].as_str().unwrap().to_owned(),
                role: item["role"].as_str().unwrap().to_owned(),
                text: expand(&item["text"]),
                complete: item["complete"].as_bool().unwrap(),
                at: text(item, "at"),
            })
            .collect(),
    }
}

/// The conversations a row names: one of the golden's histories, or its own.
pub fn conversations_of(row: &Value) -> Vec<ChiefConversation> {
    let rows = match row.get("history") {
        Some(name) => &GOLDEN["histories"][name.as_str().unwrap()],
        None => &row["conversations"],
    };
    rows.as_array()
        .unwrap()
        .iter()
        .map(conversation_of)
        .collect()
}

/// The lookup a page names a delivery by: the golden's messages by their id.
pub fn known(id: i64) -> Result<Option<MessageView>, Refusal> {
    Ok(table("messages")
        .iter()
        .find(|row| row["id"].as_i64() == Some(id))
        .map(message_of))
}

/// A lookup that knows no message.
pub fn unknown(_: i64) -> Result<Option<MessageView>, Refusal> {
    Ok(None)
}

/// A member of the staff as a golden row holds it.
pub fn staff_row_of(row: &Value) -> StaffRow {
    StaffRow {
        name: row["name"].as_str().unwrap().to_owned(),
        roles: texts(row, "roles"),
        work_tier: row
            .get("workTier")
            .and_then(Value::as_str)
            .and_then(|word| WORK_TIERS.into_iter().find(|tier| tier.as_str() == word)),
    }
}

/// A member of the staff, the way a test says it.
pub fn staff_row(name: &str, roles: &[&str], work_tier: WorkTier) -> StaffRow {
    StaffRow {
        name: name.to_owned(),
        roles: roles.iter().map(|role| (*role).to_owned()).collect(),
        work_tier: Some(work_tier),
    }
}

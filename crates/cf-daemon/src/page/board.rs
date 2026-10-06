//! The board as the page draws it, and the human's inbox beside it.

use serde_json::{json, Value};

use super::agents::{agent_gone, saved};
use super::body::{merged, one, Body, Fields, Said};
use super::Page;

/// `board.get`: the ledger's board, each lane with what the dispatcher knows of
/// its window: after the lane's own fields, in this order, `agentMissing`,
/// `activity`, `pane`, `hidden`, `switching` and `holding`, and, only while a
/// stop of its task is ignored in every round, `unstopped`.
pub(super) async fn get(page: &Page, body: Body<'_>) -> Result<Fields, Said> {
    let board = page.ledger.borrow().board(body.whole("project")?)?;
    let agents = saved(&page.env)?;
    let roster = agents.roster();
    let mut shown = serde_json::to_value(&board)?;
    let lanes = shown
        .get_mut("lanes")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| Said::from("the board has no lanes"))?;
    for (lane, drawn) in board.lanes.iter().zip(lanes) {
        let Some(fields) = drawn.as_object_mut() else {
            continue;
        };
        let participant = &lane.participant;
        let id = participant.id;
        // A member whose agent is gone (a release dropped the entry, or the
        // human removed their own) sits, and the board says why: read from the
        // file at each lane, as it is now.
        let missing = match &participant.agent {
            Some(agent) if participant.member.is_none() => agent_gone(&roster, agent),
            _ => false,
        };
        fields.insert("agentMissing".to_owned(), json!(missing));
        fields.insert("activity".to_owned(), page.engine.activity(id));
        fields.insert(
            "pane".to_owned(),
            page.engine.pane(id).unwrap_or(Value::Null),
        );
        // A window the human hid that has not closed yet: Show makes it theirs again.
        fields.insert("hidden".to_owned(), json!(page.engine.hidden(id)));
        // A Switch chief that waits for the chief's turn to end.
        fields.insert(
            "switching".to_owned(),
            page.engine.pending_switch(id).unwrap_or(Value::Null),
        );
        // A message that waits until the human sends what they typed there.
        fields.insert("holding".to_owned(), json!(page.engine.holding(id)?));
        // A window that would not stop: its task, and how often it was told to.
        if let Some(ignored) = page.engine.unstopped(id) {
            fields.insert("unstopped".to_owned(), ignored);
        }
    }
    one("board", shown)
}

/// `inbox.get`: as much as one frame carries, newest first, and how many there
/// are; `unread` is what For you lists: the human's notes not yet read.
pub(super) async fn inbox(page: &Page, body: Body<'_>) -> Result<Fields, Said> {
    // `participant = 'human'`: only a field that was not sent is the human; a
    // handle is found by `===`, so only text names one.
    let (named, handle) = match body.get("participant") {
        None => ("human".into(), Some("human")),
        given => (body.text("participant"), given.and_then(Value::as_str)),
    };
    let project = body.whole("project")?;
    let owner = page
        .ledger
        .borrow()
        .project(project)?
        .and_then(|found| {
            found
                .participants
                .into_iter()
                .find(|member| Some(member.handle.as_str()) == handle)
        })
        .ok_or_else(|| Said(format!("{named} is not in project {project}")))?;
    let unread = body.get("unread") == Some(&Value::Bool(true));
    merged(page.ledger.borrow().latest_messages(owner.id, unread)?)
}

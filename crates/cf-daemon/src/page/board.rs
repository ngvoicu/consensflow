//! The board as the page draws it, and the human's inbox beside it.

use serde_json::{json, Value};

use super::body::{merged, one, Body, Fields, Said};
use super::Page;

/// `board.get`: the ledger's board, each lane with what the dispatcher knows of
/// its window: after the lane's own fields, in this order, `activity`, `pane`, `hidden`,
/// `switching` and `holding`, and, only while a
/// stop of its task is ignored in every round and the host would not close its
/// window, `unstopped`, and, only while its task waits on a background job or
/// agent that its harness wakes its agent for, `held`, and, only while the member of the
/// lane (itself, or the one its session belongs to) is out because its model is busy for a
/// moment, `busy`: until when (T-1357; a timed hold reads as a quota by its time alone).
pub(super) async fn get(page: &Page, body: Body<'_>) -> Result<Fields, Said> {
    let board = page.ledger.borrow().board(body.whole("project")?)?;
    let mut shown = serde_json::to_value(&board)?;
    let lanes = shown
        .get_mut("lanes")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| Said::from("the board has no lanes"))?;
    let participants = page
        .ledger
        .borrow()
        .project(body.whole("project")?)?
        .map(|found| found.participants)
        .unwrap_or_default();
    for (lane, drawn) in board.lanes.iter().zip(lanes) {
        let Some(fields) = drawn.as_object_mut() else {
            continue;
        };
        let id = lane.participant.id;
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
        // A task that goes on in the background: its card says so.
        if let Some(held) = page.engine.held(id) {
            fields.insert("held".to_owned(), held);
        }
        // A member out for its busy model, which no press ends: the page says so, and offers
        // no button, for the member's lane and its sessions' alike.
        let owner = lane.participant.member_id.unwrap_or(id);
        if let Some(until) = participants
            .iter()
            .find(|found| found.id == owner)
            .and_then(|member| page.engine.model_busy_until(member))
        {
            fields.insert("busy".to_owned(), json!(until));
        }
    }
    let mut answer = one("board", shown)?;
    // The roles its cards and lanes name, in the table's order: a board the page draws is one
    // whose roles it read with it (`cf_ledger::roles`).
    answer.insert("roles".to_owned(), cf_ledger::roles::page_view());
    Ok(answer)
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

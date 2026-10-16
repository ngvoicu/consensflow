//! `history.get` and `history.part`: a participant's whole history, for the
//! viewer its window's History button opens: every conversation, the current
//! one too, a page at a time from a place that does not move, and a long
//! item's text a part at a time. They read, and write nothing.
//!
//! A read is the ledger's, on the daemon's own thread: the ledger holds its
//! file alone (`locking_mode = EXCLUSIVE`, one connection), so no other thread
//! could read it. Each read is bounded instead: a page, and at most
//! [`HISTORY_SCAN_BYTES`] of text gone through.

use cf_base::refusal::Refusal;
use cf_engine::handoff::{whole_history_page, HISTORY_ROOM};
use cf_ledger::{HistoryAsk, HistoryFrom, HistoryKey, MessageView, HISTORY_SCAN_BYTES};
use serde_json::Value;

use super::body::{one, Body, Fields, Said};
use super::Page;

/// `history.get`: a page of the history of participant `handle`: the most
/// recent, or the one before `before` or after `after` (a place a page said);
/// only the items that hold `find` (in any case) where it is given, and the
/// tools' outputs where `tools` is true. A delivery's line names a message
/// this participant was sent or sent, and no other.
pub(super) async fn get(page: &Page, body: Body<'_>) -> Result<Fields, Said> {
    let project = body.whole("project")?;
    let handle = body.text("handle");
    let from = match (place(body, "before")?, place(body, "after")?) {
        (Some(_), Some(_)) => {
            return Err(Said::from(
                "a page is before a place or after one, not both",
            ))
        }
        (Some(before), None) => HistoryFrom::Before(before),
        (None, Some(after)) => HistoryFrom::After(after),
        (None, None) => HistoryFrom::Newest,
    };
    let find = body
        .get("find")
        .and_then(Value::as_str)
        .filter(|find| !find.is_empty());
    let ask = HistoryAsk {
        from,
        find,
        tools: body.get("tools") == Some(&Value::Bool(true)),
        room: HISTORY_ROOM,
        scan: HISTORY_SCAN_BYTES,
    };
    let read = page.ledger.borrow().read_history(project, &handle, &ask)?;
    let message = |id: i64| -> Result<Option<MessageView>, Refusal> {
        let found = page
            .ledger
            .borrow()
            .message(id)
            .map_err(|failed| Refusal::with_status("internal", failed.to_string(), 500))?;
        Ok(found.filter(|found| {
            found.project_id == project
                && (found.recipient_id == read.participant
                    || found.sender.as_deref() == Some(handle.as_ref()))
        }))
    };
    one(
        "history",
        whole_history_page(&read, from, find.is_some(), &message)?,
    )
}

/// `history.part`: the part of item `item` of conversation `conversation`,
/// participant `handle`'s, from character `from` (its start where none is given).
pub(super) async fn part(page: &Page, body: Body<'_>) -> Result<Fields, Said> {
    let from = match body.get("from") {
        None | Some(Value::Null) => 0,
        Some(from) => from
            .as_u64()
            .and_then(|from| usize::try_from(from).ok())
            .ok_or_else(|| Said::from("from is a character of the item, a whole number"))?,
    };
    let part = page.ledger.borrow().read_history_part(
        body.whole("project")?,
        &body.text("handle"),
        body.whole("conversation")?,
        &body.text("item"),
        from,
    )?;
    one("part", part)
}

/// The place a page said, given as `name`: none where it is not given.
fn place(body: Body<'_>, name: &str) -> Result<Option<HistoryKey>, Said> {
    match body.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(place) => serde_json::from_value(place.clone())
            .map(Some)
            .map_err(|_| Said(format!("{name} is a place a page said, not {place}"))),
    }
}

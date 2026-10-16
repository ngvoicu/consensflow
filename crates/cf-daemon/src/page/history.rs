//! `history.get`: a participant's whole history, for the viewer its window's
//! History button opens: every conversation, the current one too, a page at a
//! time. It reads, and writes nothing.

use cf_base::refusal::Refusal;
use cf_engine::handoff::whole_history_page;
use cf_ledger::MessageView;
use serde_json::Value;

use super::body::{one, Body, Fields, Said};
use super::Page;

/// `history.get`: page `page` (the most recent, 1, where none is given) of
/// the history of participant `handle`, only the items that hold `find`, in
/// any case, where it is given, and the tools' outputs where `tools` is true.
pub(super) async fn get(page: &Page, body: Body<'_>) -> Result<Fields, Said> {
    let project = body.whole("project")?;
    let history = page
        .ledger
        .borrow()
        .participant_history(project, &body.text("handle"))?;
    // A line may name any number: only this project's messages are read out.
    let message = |id: i64| -> Result<Option<MessageView>, Refusal> {
        let found = page
            .ledger
            .borrow()
            .message(id)
            .map_err(|failed| Refusal::with_status("internal", failed.to_string(), 500))?;
        Ok(found.filter(|found| found.project_id == project))
    };
    let number = match body.get("page") {
        None | Some(Value::Null) => 1.0,
        Some(asked) => asked.as_f64().unwrap_or(f64::NAN),
    };
    let find = body.get("find").and_then(Value::as_str);
    let tools = body.get("tools") == Some(&Value::Bool(true));
    one(
        "history",
        whole_history_page(&history, &message, number, find, tools)?,
    )
}

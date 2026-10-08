//! `GET /api/history`: a page of the chief's history, which only a chief reads
//! (a chief the human switched in reads what the human and the chiefs before it
//! said), and which writes down that it was read.

use cf_base::js;
use cf_base::refusal::Refusal;
use cf_engine::handoff::history_page;
use cf_ledger::{LedgerError, MessageView};

use super::{Answer, Caller, Context, Failure, Request};
use crate::api::views::value;

/// The largest whole number a page may be and still be written down as it
/// was asked for: a double holds no more exactly.
const MOST_EXACT: f64 = 9_007_199_254_740_991.0;

pub(super) async fn handle(
    context: &Context,
    caller: &Caller,
    request: Request,
) -> Result<Answer, Failure> {
    if caller.participant.role != "chief" {
        return Err(Failure::refuse(
            403,
            "not-the-chief",
            "the chief's history is the chief's to read",
        ));
    }
    // `Number(page ?? '1')`: a page that is no number reads as NaN, and is
    // no page of any history.
    let page = js::number(request.param("page").unwrap_or("1"));
    let find = request.param("find").filter(|find| !find.is_empty());
    let tools = request.param("tools") == Some("1");
    let project = caller.project.id;
    let history = context.ledger.borrow().chief_history(project)?;
    // A line may name any number: only this project's messages are read out.
    let message = |id: i64| -> Result<Option<MessageView>, Refusal> {
        let found = context.ledger.borrow().message(id).map_err(refused)?;
        Ok(found.filter(|found| found.project_id == project))
    };
    let shown = history_page(&history, &message, page, find, tools)?;
    context
        .ledger
        .borrow_mut()
        .history_read(project, written_page(page), find, tools)?;
    Ok(Answer::ok(value(&shown)?))
}

/// What a failed lookup says, as the API says any failure: a refusal as it
/// was, and anything else as 500 `internal`.
fn refused(failed: LedgerError) -> Refusal {
    match Failure::from(failed) {
        Failure::Refused(refusal) => refusal,
        Failure::Internal(words) => Refusal::with_status("internal", words, 500),
    }
}

/// The page the ledger writes down. A history with nothing in it answers
/// whatever page was asked for, and Node wrote it down as it was (`null` for
/// the page of `abc`); the ledger writes whole numbers, so a page that is none
/// is written as the page the answer was, the first that is not there: 0.
fn written_page(page: f64) -> i64 {
    if page.is_finite() && page.fract() == 0.0 && page.abs() <= MOST_EXACT {
        // A whole number the double holds exactly: the cast loses nothing.
        #[allow(clippy::cast_possible_truncation)]
        return page as i64;
    }
    0
}

#[cfg(test)]
mod tests;

//! The human's messages: the notes read, and what waits for approval sent or
//! declined.

use cf_engine::require_open;

use super::body::{one, Body, Fields, Said};
use super::Page;

/// The human, as the ledger is told who decided.
const HUMAN: &str = "human";

/// `message.read`.
pub(super) async fn read(page: &Page, body: Body<'_>) -> Result<Fields, Said> {
    let message = page.ledger.borrow_mut().mark_read(body.whole("message")?)?;
    one("message", message)
}

/// `message.approve`: nothing is passed on in a closed project.
pub(super) async fn approve(page: &Page, body: Body<'_>) -> Result<Fields, Said> {
    let id = body.whole("message")?;
    let waiting = page.ledger.borrow().message(id)?;
    if let Some(waiting) = waiting {
        if let Some(project) = page.ledger.borrow().project(waiting.project_id)? {
            require_open(&project)?;
        }
    }
    let message = page.ledger.borrow_mut().approve_message(id, HUMAN)?;
    one("message", message)
}

/// `message.decline`.
pub(super) async fn decline(page: &Page, body: Body<'_>) -> Result<Fields, Said> {
    let message = page
        .ledger
        .borrow_mut()
        .decline_message(body.whole("message")?, HUMAN)?;
    one("message", message)
}

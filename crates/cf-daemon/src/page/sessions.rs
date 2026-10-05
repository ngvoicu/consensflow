//! A session's window at the human's hand: opened, hidden, ended.

use super::body::{one, Body, Fields, Said};
use super::Page;

/// `session.open`.
pub(super) async fn open(page: &Page, body: Body<'_>) -> Result<Fields, Said> {
    let project = page
        .engine
        .open_window(body.whole("project")?, &body.text("handle"))
        .await?;
    one("project", project)
}

/// `session.hide`: the human hides a session's terminal: its window closes once
/// nothing holds it open.
pub(super) async fn hide(page: &Page, body: Body<'_>) -> Result<Fields, Said> {
    let project = page
        .engine
        .hide_window(body.whole("project")?, &body.text("handle"))
        .await?;
    one("project", project)
}

/// `session.end`: delete session: it leaves the board with its window, and
/// keeps its conversation.
pub(super) async fn end(page: &Page, body: Body<'_>) -> Result<Fields, Said> {
    let project = page
        .engine
        .end_session(body.whole("project")?, &body.text("handle"))
        .await?;
    one("project", project)
}

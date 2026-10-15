//! The human's wake-up calls: what Project settings reads of a project's, and
//! what it saves. (New project says them in `project.open`, which keeps them in
//! the write that makes the project.) Only the page asks for them: no route of
//! the agents' API reaches them, so a window's token, the chief's or a member's,
//! changes nothing of them.

use cf_ledger::NewWakeUp;

use super::body::{one, Body, Fields, Said};
use super::Page;

/// `wakeup.get`: the project's calls as the dialog shows them, off, an hour
/// apart and with the default words until the human says otherwise.
pub(super) async fn get(page: &Page, body: Body<'_>) -> Result<Fields, Said> {
    let setting = page.ledger.borrow().wake_up(body.whole("project")?)?;
    one("wakeUp", setting.view())
}

/// `wakeup.set`: the project's calls, on or off, `every` of a `unit` apart
/// (minutes, hours or days, a minute at the least) and the `text` the human
/// wrote, or none for the default's. The request is checked before the
/// project is looked for. The answer is the setting as saved, and the
/// dispatcher is woken: it applies at once.
pub(super) async fn set(page: &Page, body: Body<'_>) -> Result<Fields, Said> {
    let request = NewWakeUp::from_json(body.json())?;
    let setting = page
        .ledger
        .borrow_mut()
        .set_wake_up(body.whole("project")?, &request)?;
    one("wakeUp", setting.view())
}

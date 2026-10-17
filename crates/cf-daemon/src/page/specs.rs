//! What a project says of specs, which its chief plans big work by: whether
//! they are on, the folder they are kept in and the Specmint variant. The
//! human's alone, as the gate is: no window's token reaches it (the daemon's
//! API has no route for it, and the page is the app's). Project settings says
//! it here; New project says it in `project.open`, which keeps it in the write
//! that makes the project.

use cf_ledger::SpecsChange;
use serde_json::json;

use super::body::{one, Body, Fields, Said};
use super::Page;

/// `project.specs`: all three parts at once, each checked before the project is
/// looked for; one the page leaves out, or sends as `null`, is not said (the
/// project follows what the app says, [`super::app_specs`]). It applies at the
/// chief's next launch, which the page says.
pub(super) async fn set(page: &Page, body: Body<'_>) -> Result<Fields, Said> {
    let change = SpecsChange::from_json(&json!({
        "on": body.get("on"),
        "folder": body.get("folder"),
        "variant": body.get("variant"),
    }))?;
    let project = page
        .ledger
        .borrow_mut()
        .set_specs(body.whole("project")?, &change)?;
    one("project", project)
}

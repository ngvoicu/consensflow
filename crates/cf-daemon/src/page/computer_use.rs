//! A project's computer use, for the row Project settings shows (T-540): what the
//! system is, ConsensFlow's macOS grants (the app's, shown where they matter), and
//! for each harness with a computer use of its own what it says for the project's
//! folder, under the harness's word. Claude Code's is read from its own
//! configuration, never written. The page shows a harness's part only when the
//! project has a member of that harness, the chief included.
//!
//! `computer_use.open` opens one of the two System Settings panes where the human
//! gives a grant, by the pane's name, and nothing else.

use std::rc::Rc;

use cf_engine::seams::ComputerUse;
use serde_json::{json, Value};

use super::body::{merged, one, Body, Fields, Said};
use super::Page;
use crate::computer_use::{Opens, Pane};

/// What the page asks of the machine's computer use.
pub struct ComputerUsePage {
    pub machine: Rc<dyn ComputerUse>,
    pub open: Opens,
    /// The app the daemon's grants are another's of, where they are not ConsensFlow's
    /// own ([`crate::computer_use::grants_of`]).
    pub grants_of: Option<String>,
}

/// The system, by the page's word for it.
fn system() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(windows) {
        "windows"
    } else {
        "other"
    }
}

/// `computer_use.get`: `{system, permissions, grantsOf, harnesses}` for the project the
/// body names; `permissions` is null where the system keeps no grants, and `grantsOf`
/// names the app they are of where it is not ConsensFlow (null where it is).
pub(super) async fn get(page: &Page, body: Body<'_>) -> Result<Fields, Said> {
    let id = body.whole("project")?;
    let project = page
        .ledger
        .borrow()
        .project(id)?
        .ok_or_else(|| Said(format!("no project {id}")))?;
    let machine = &page.computer_use.machine;
    let permissions = machine.permissions().map_or(Value::Null, |granted| {
        json!({
            "accessibility": granted.accessibility,
            "screenRecording": granted.screen_recording,
        })
    });
    let claude = machine.claude(&project.directory);
    merged(json!({
        "system": system(),
        "permissions": permissions,
        "grantsOf": page.computer_use.grants_of,
        "harnesses": {
            "claude-code": { "on": claude.on, "folder": claude.folder },
        },
    }))
}

/// `computer_use.open`: the System Settings pane the body names (`accessibility`
/// or `screen-recording`).
pub(super) async fn open(page: &Page, body: Body<'_>) -> Result<Fields, Said> {
    let pane = Pane::named(&body.text("pane")).ok_or_else(|| {
        Said(format!(
            "pane names accessibility or screen-recording, not {}",
            body.text("pane")
        ))
    })?;
    (page.computer_use.open)(pane).await.map_err(Said)?;
    one("opened", body.text("pane"))
}

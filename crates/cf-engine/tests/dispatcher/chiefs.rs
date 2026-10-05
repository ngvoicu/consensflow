//! What the suites about a project's chief share (the helpers of
//! `core-dispatcher.test.mjs` between `a member whose saved agent is gone`
//! and `switching the chief to another agent`): Codex's own fake, the chief
//! of a project, the handoffs it was written, and a project replaced while
//! something of it waits.

use cf_engine::seams::EngineError;
use cf_engine::testing::{Context, Driver, Made, Pending};
use cf_engine::SwitchTo;
use cf_ledger::{DeletedProject, MessageView, ParticipantView, ProjectView};
use regex::Regex;
use serde_json::json;

/// A test's engine where Codex has a fake of its own, so a test sees which
/// harness a window opened on (`withCodex`).
pub fn with_codex() -> Context {
    Context::made(Made {
        codex: true,
        ..Made::default()
    })
}

/// The chief a switch is to.
pub fn to(harness: &str, agent: &str) -> SwitchTo {
    SwitchTo {
        harness: harness.to_owned(),
        agent: agent.to_owned(),
    }
}

/// The project's chief as the ledger has it now (`chiefOf`).
pub fn chief_of(context: &Context, project: i64) -> ParticipantView {
    context
        .project(project)
        .participants
        .into_iter()
        .find(|participant| participant.handle == "chief")
        .expect("a chief")
}

/// The handoffs the chief was written, newest first (`handoffsOf`).
pub fn handoffs_of(context: &Context, project: i64) -> Vec<MessageView> {
    let chief = chief_of(context, project).id;
    let inbox = context
        .ledger
        .borrow()
        .inbox(chief, 100)
        .expect("the chief's inbox");
    inbox
        .into_iter()
        .filter(|message| message.body.starts_with("You are the chief now"))
        .collect()
}

/// What the human heard from the project: the bodies of the notes to them.
pub fn to_human(context: &Context, project: i64) -> Vec<String> {
    let human = context.id(project, "human");
    let inbox = context
        .ledger
        .borrow()
        .inbox(human, 100)
        .expect("the human's inbox");
    inbox.into_iter().map(|message| message.body).collect()
}

/// `assert.match`: `text` holds what `pattern` says, read as JavaScript read it.
pub fn assert_match(text: &str, pattern: &str) {
    let regex = Regex::new(pattern).expect("a pattern");
    assert!(regex.is_match(text), "{text:?} does not match /{pattern}/");
}

/// A project replaced while something of it waits: the human closes and
/// deletes it, and opens another, whose ids are its own, as the ledger never
/// gives the deleted one's again (`replaceProject`).
pub struct Replaced {
    /// The project opened in its place.
    pub fresh: ProjectView,
    old: i64,
    closing: Pending<Result<ProjectView, EngineError>>,
    deleting: Pending<Result<DeletedProject, EngineError>>,
}

impl Replaced {
    /// The old project's close and delete ended, and failed nothing
    /// (`await gone`). A close answers the project as it is when it ends:
    /// Node answered `null` for one deleted meanwhile, where the engine's
    /// operations answer a project or say there is none.
    pub fn gone(&self) {
        let closed = self.closing.answer().expect("the close ended");
        if let Err(error) = closed {
            assert_eq!(error.to_string(), format!("no project {}", self.old));
        }
        self.deleting
            .answer()
            .expect("the delete ended")
            .expect("the delete");
    }
}

/// Closes and deletes `old` while something of it waits, and opens another
/// project, the old one's close and delete left to go on beside it.
pub fn replace_project(driver: &Driver, old: &ProjectView) -> Replaced {
    let closing = driver.begin_close_project(old.id);
    let deleting = driver.begin_delete_project(old.id);
    let fresh = driver
        .open_project(json!({
            "directory": "/work/api",
            "name": "api",
            "chief": { "harness": "claude-code", "agent": "apollo" },
            "staff": [],
        }))
        .expect("a project in its place");
    assert_ne!(fresh.id, old.id, "the ledger never gives its id again");
    Replaced {
        fresh,
        old: old.id,
        closing,
        deleting,
    }
}

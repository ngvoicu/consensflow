//! An engine that does only what a test scripts: a window that takes three
//! turns to open or hide, saying each, and a lane whose activity, read, does
//! what the test says. What a test does not script it does not call.

use cf_engine::seams::EngineError;
use cf_engine::{SwitchTo, SwitchWhen};
use cf_harness::contract::Work;
use cf_ledger::{
    DeletedProject, NewProject, ParticipantView, ProjectView, RemovedMember, TaskReleased,
};

use super::*;

/// What a test's operations do not call.
const UNUSED: &str = "the operations of this test do not call it";

pub(super) struct Scripted {
    /// What the windows did, in order.
    pub(super) steps: Rc<RefCell<Vec<String>>>,
    /// Told the participant whose activity is read.
    pub(super) reading: Box<dyn Fn(i64)>,
}

impl Scripted {
    pub(super) fn new() -> Self {
        Self {
            steps: Rc::default(),
            reading: Box::new(|_| {}),
        }
    }

    /// A piece of work that says its three steps, a turn apart, and answers no project.
    fn work(&self, what: String) -> Work<'_, Result<Option<ProjectView>, EngineError>> {
        let steps = Rc::clone(&self.steps);
        Box::pin(async move {
            for step in 0..3 {
                steps.borrow_mut().push(format!("{what} {step}"));
                next_turn().await;
            }
            Ok(None)
        })
    }
}

impl Engine for Scripted {
    fn open_window<'a>(
        &'a self,
        _project: i64,
        handle: &'a str,
    ) -> Work<'a, Result<Option<ProjectView>, EngineError>> {
        self.work(format!("open {handle}"))
    }

    fn hide_window<'a>(
        &'a self,
        _project: i64,
        handle: &'a str,
    ) -> Work<'a, Result<Option<ProjectView>, EngineError>> {
        self.work(format!("hide {handle}"))
    }

    fn activity(&self, participant: i64) -> Value {
        (self.reading)(participant);
        json!({ "state": "idle" })
    }

    fn holding(&self, _: i64) -> Result<bool, EngineError> {
        Ok(false)
    }

    fn hidden(&self, _: i64) -> bool {
        false
    }

    fn pending_switch(&self, _: i64) -> Option<Value> {
        None
    }

    fn pane(&self, _: i64) -> Option<Value> {
        None
    }

    fn unstopped(&self, _: i64) -> Option<Value> {
        None
    }

    fn open_project(&self, _: NewProject) -> Work<'_, Result<Option<ProjectView>, EngineError>> {
        unreachable!("{UNUSED}")
    }

    fn resume_project(&self, _: i64) -> Work<'_, Result<Option<ProjectView>, EngineError>> {
        unreachable!("{UNUSED}")
    }

    fn close_project(&self, _: i64) -> Work<'_, Result<Option<ProjectView>, EngineError>> {
        unreachable!("{UNUSED}")
    }

    fn delete_project(&self, _: i64) -> Work<'_, Result<DeletedProject, EngineError>> {
        unreachable!("{UNUSED}")
    }

    fn switch_chief(
        &self,
        _: i64,
        _: SwitchTo,
        _: SwitchWhen,
        _: bool,
    ) -> Work<'_, Result<Option<ProjectView>, EngineError>> {
        unreachable!("{UNUSED}")
    }

    fn end_session<'a>(&'a self, _: i64, _: &'a str) -> Work<'a, Result<ProjectView, EngineError>> {
        unreachable!("{UNUSED}")
    }

    fn remove_member<'a>(
        &'a self,
        _: i64,
        _: &'a str,
    ) -> Work<'a, Result<RemovedMember, EngineError>> {
        unreachable!("{UNUSED}")
    }

    fn reassign_task(&self, _: i64, _: i64) -> Work<'_, Result<TaskReleased, EngineError>> {
        unreachable!("{UNUSED}")
    }

    fn back_from_quota(&self, _: i64, _: &str) -> Result<ParticipantView, EngineError> {
        unreachable!("{UNUSED}")
    }

    fn require_adapter(&self, _: &str) -> Result<(), EngineError> {
        unreachable!("{UNUSED}")
    }
}

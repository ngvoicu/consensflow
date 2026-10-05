//! What the page's operations call on the engine (`dispatcher` in
//! `src/core/page.js`): the human's operations on the dispatcher, and the five
//! projections `board.get` adds to each lane. **Frozen**: the operations are
//! written against this trait, so that their tests stand a small stand-in in
//! the engine's place (`core-page.test.mjs:42-107`), and the daemon gives
//! [`cf_engine::Dispatcher`].
//!
//! An operation that waits answers a [`Work`]: nothing runs until it is
//! polled, and the handler polls it where it makes the call, so the engine's
//! work is begun in the handler's first poll, in the order the frames were
//! read. The projections and the quota's `back_from_quota` read what the
//! engine knows now and wait for nothing.

use std::rc::Rc;

use cf_engine::seams::EngineError;
use cf_engine::{Activity, Dispatcher, SwitchTo, SwitchWhen};
use cf_harness::contract::{Pane, Work};
use cf_ledger::{
    DeletedProject, NewProject, ParticipantView, ProjectView, RemovedMember, TaskReleased,
};

/// The engine, as the page's operations use it.
pub trait Engine {
    /// A new project (`openProject`): its chief's window opens after the
    /// answer. None where the project was deleted meanwhile.
    fn open_project(
        &self,
        request: NewProject,
    ) -> Work<'_, Result<Option<ProjectView>, EngineError>>;

    /// The human's Resume (`resumeProject`).
    fn resume_project(&self, project: i64) -> Work<'_, Result<Option<ProjectView>, EngineError>>;

    /// The human's Close (`closeProject`): every window of the project goes.
    fn close_project(&self, project: i64) -> Work<'_, Result<Option<ProjectView>, EngineError>>;

    /// A closed project goes for good (`deleteProject`).
    fn delete_project(&self, project: i64) -> Work<'_, Result<DeletedProject, EngineError>>;

    /// The human's Switch chief (`switchChief`): the chief goes on in a fresh
    /// window on `to`, now or once its turn ends, with a note first if asked.
    fn switch_chief(
        &self,
        project: i64,
        to: SwitchTo,
        when: SwitchWhen,
        note: bool,
    ) -> Work<'_, Result<Option<ProjectView>, EngineError>>;

    /// A session's window opened by the human (`openWindow`).
    fn open_window<'a>(
        &'a self,
        project: i64,
        handle: &'a str,
    ) -> Work<'a, Result<Option<ProjectView>, EngineError>>;

    /// A session's window hidden by the human (`hideWindow`).
    fn hide_window<'a>(
        &'a self,
        project: i64,
        handle: &'a str,
    ) -> Work<'a, Result<Option<ProjectView>, EngineError>>;

    /// A session deleted with its window (`endSession`).
    fn end_session<'a>(
        &'a self,
        project: i64,
        handle: &'a str,
    ) -> Work<'a, Result<ProjectView, EngineError>>;

    /// A member off the staff once its step in progress is over
    /// (`removeMember`).
    fn remove_member<'a>(
        &'a self,
        project: i64,
        handle: &'a str,
    ) -> Work<'a, Result<RemovedMember, EngineError>>;

    /// A task given to another member of its tier (`reassignTask`).
    fn reassign_task(
        &self,
        project: i64,
        number: i64,
    ) -> Work<'_, Result<TaskReleased, EngineError>>;

    /// A member out of quota is back before its reset (`backFromQuota`).
    fn back_from_quota(&self, project: i64, handle: &str) -> Result<ParticipantView, EngineError>;

    /// Refuses a harness ConsensFlow has no adapter for (`requireAdapter`).
    fn require_adapter(&self, harness: &str) -> Result<(), EngineError>;

    /// What a participant's window is doing (`activity`).
    fn activity(&self, participant: i64) -> Activity;

    /// Whether a message waits for the human to send what they typed in the
    /// participant's window (`holding`).
    fn holding(&self, participant: i64) -> Result<bool, EngineError>;

    /// Whether the human hid the window and it has not closed yet (`hidden`).
    fn hidden(&self, participant: i64) -> bool;

    /// The Switch chief waiting for this chief's turn to end (`pendingSwitch`).
    fn pending_switch(&self, participant: i64) -> Option<SwitchTo>;

    /// The participant's live window (`pane`).
    fn pane(&self, participant: i64) -> Option<Pane>;
}

/// The dispatcher is the engine the daemon gives: each call is its own.
impl Engine for Rc<Dispatcher> {
    fn open_project(
        &self,
        request: NewProject,
    ) -> Work<'_, Result<Option<ProjectView>, EngineError>> {
        Box::pin(Dispatcher::open_project(self, request))
    }

    fn resume_project(&self, project: i64) -> Work<'_, Result<Option<ProjectView>, EngineError>> {
        Box::pin(Dispatcher::resume_project(self, project))
    }

    fn close_project(&self, project: i64) -> Work<'_, Result<Option<ProjectView>, EngineError>> {
        Box::pin(Dispatcher::close_project(self, project))
    }

    fn delete_project(&self, project: i64) -> Work<'_, Result<DeletedProject, EngineError>> {
        Box::pin(Dispatcher::delete_project(self, project))
    }

    fn switch_chief(
        &self,
        project: i64,
        to: SwitchTo,
        when: SwitchWhen,
        note: bool,
    ) -> Work<'_, Result<Option<ProjectView>, EngineError>> {
        Box::pin(Dispatcher::switch_chief(self, project, to, when, note))
    }

    fn open_window<'a>(
        &'a self,
        project: i64,
        handle: &'a str,
    ) -> Work<'a, Result<Option<ProjectView>, EngineError>> {
        Box::pin(Dispatcher::open_window(self, project, handle))
    }

    fn hide_window<'a>(
        &'a self,
        project: i64,
        handle: &'a str,
    ) -> Work<'a, Result<Option<ProjectView>, EngineError>> {
        Box::pin(Dispatcher::hide_window(self, project, handle))
    }

    fn end_session<'a>(
        &'a self,
        project: i64,
        handle: &'a str,
    ) -> Work<'a, Result<ProjectView, EngineError>> {
        Box::pin(Dispatcher::end_session(self, project, handle))
    }

    fn remove_member<'a>(
        &'a self,
        project: i64,
        handle: &'a str,
    ) -> Work<'a, Result<RemovedMember, EngineError>> {
        Box::pin(Dispatcher::remove_member(self, project, handle))
    }

    fn reassign_task(
        &self,
        project: i64,
        number: i64,
    ) -> Work<'_, Result<TaskReleased, EngineError>> {
        Box::pin(Dispatcher::reassign_task(self, project, number))
    }

    fn back_from_quota(&self, project: i64, handle: &str) -> Result<ParticipantView, EngineError> {
        Dispatcher::back_from_quota(self, project, handle)
    }

    fn require_adapter(&self, harness: &str) -> Result<(), EngineError> {
        Dispatcher::requires_adapter(self, harness)
    }

    fn activity(&self, participant: i64) -> Activity {
        Dispatcher::activity(self, participant)
    }

    fn holding(&self, participant: i64) -> Result<bool, EngineError> {
        Dispatcher::holding(self, participant)
    }

    fn hidden(&self, participant: i64) -> bool {
        Dispatcher::hidden(self, participant)
    }

    fn pending_switch(&self, participant: i64) -> Option<SwitchTo> {
        Dispatcher::pending_switch(self, participant)
    }

    fn pane(&self, participant: i64) -> Option<Pane> {
        Dispatcher::pane(self, participant)
    }
}

#[cfg(test)]
mod tests;

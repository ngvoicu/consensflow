//! The human's operations on the engine as a test drives them: each written
//! down where it begins, as the Node recorder marks the dispatcher's
//! operations, and the work it began run to stillness.

use std::rc::Rc;

use cf_ledger::{DeletedProject, ParticipantView, ProjectView, TaskReleased};
use serde_json::json;

use super::context::Context;
use crate::seams::EngineError;

impl Context {
    /// The human's Resume (`resumeProject`, which the engine writes down as
    /// it begins).
    pub fn resume_project(&self, project: i64) -> Result<ProjectView, EngineError> {
        let dispatcher = Rc::clone(&self.dispatcher);
        self.run(async move { dispatcher.resume_project(project).await })
    }

    /// The human's Close (`closeProject`).
    pub fn close_project(&self, project: i64) -> Result<ProjectView, EngineError> {
        self.recorder.op("closeProject", json!([project]));
        let dispatcher = Rc::clone(&self.dispatcher);
        self.run(async move { dispatcher.close_project(project).await })
    }

    /// The human deletes a closed project (`deleteProject`).
    pub fn delete_project(&self, project: i64) -> Result<DeletedProject, EngineError> {
        self.recorder.op("deleteProject", json!([project]));
        let dispatcher = Rc::clone(&self.dispatcher);
        self.run(async move { dispatcher.delete_project(project).await })
    }

    /// The human opens a session's window (`openWindow`).
    pub fn open_window(&self, project: i64, handle: &str) -> Result<ProjectView, EngineError> {
        self.recorder.op("openWindow", json!([project, handle]));
        let (dispatcher, handle) = (Rc::clone(&self.dispatcher), handle.to_owned());
        self.run(async move { dispatcher.open_window(project, &handle).await })
    }

    /// The human hides a session's terminal (`hideWindow`).
    pub fn hide_window(&self, project: i64, handle: &str) -> Result<ProjectView, EngineError> {
        self.recorder.op("hideWindow", json!([project, handle]));
        let (dispatcher, handle) = (Rc::clone(&self.dispatcher), handle.to_owned());
        self.run(async move { dispatcher.hide_window(project, &handle).await })
    }

    /// The human gives a task in a window to another member (`reassignTask`).
    pub fn reassign_task(&self, project: i64, number: i64) -> Result<TaskReleased, EngineError> {
        self.recorder.op("reassignTask", json!([project, number]));
        let dispatcher = Rc::clone(&self.dispatcher);
        self.run(async move { dispatcher.reassign_task(project, number).await })
    }

    /// The human deletes a session (`endSession`).
    pub fn end_session(&self, project: i64, handle: &str) -> Result<ProjectView, EngineError> {
        self.recorder.op("endSession", json!([project, handle]));
        let (dispatcher, handle) = (Rc::clone(&self.dispatcher), handle.to_owned());
        self.run(async move { dispatcher.end_session(project, &handle).await })
    }

    /// The last window of `handle` exits, as the pane host says it (`host.exit`).
    pub fn exit(&self, handle: &str) {
        let (host, handle) = (Rc::clone(&self.host), handle.to_owned());
        self.run(async move { host.exit(&handle).await });
    }

    /// The human says a member out of quota is back (`backFromQuota`).
    pub fn back_from_quota(
        &self,
        project: i64,
        handle: &str,
    ) -> Result<ParticipantView, EngineError> {
        self.recorder.op("backFromQuota", json!([project, handle]));
        self.dispatcher.back_from_quota(project, handle)
    }
}

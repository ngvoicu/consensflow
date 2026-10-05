//! The engine's operations as a test calls them (`dispatcher.closeProject`,
//! `deleteProject`, `resumeProject`, `removeMember`, `resumeAfterRestart`,
//! and the pane host's exits): each written down where it begins, by its
//! Node name, and run to stillness. Two calls a test made without awaiting
//! the first are begun where they were called and run after
//! ([`Context::begin_pass`]).

use std::rc::{Rc, Weak};

use cf_ledger::{DeletedProject, ProjectView, RemovedMember};
use serde_json::json;

use super::context::Context;
use super::executor::Answer;
use crate::dispatcher::{Dispatcher, Resumed};
use crate::seams::EngineError;

impl Context {
    /// A pass begun where it is called, its first wait not passed:
    /// [`Context::finish`] or [`Context::settle`] runs the rest.
    pub fn begin_pass(&self) -> Answer<Result<(), EngineError>> {
        self.recorder.op("pass", json!([]));
        let dispatcher = Rc::clone(&self.dispatcher);
        self.executor
            .start_now(async move { dispatcher.pass().await })
    }

    /// A member taken off the staff, begun where it is called.
    pub fn begin_remove_member(
        &self,
        project: i64,
        handle: &str,
    ) -> Answer<Result<RemovedMember, EngineError>> {
        self.recorder.op("removeMember", json!([project, handle]));
        let (dispatcher, handle) = (Rc::clone(&self.dispatcher), handle.to_owned());
        self.executor
            .start_now(async move { dispatcher.remove_member(project, &handle).await })
    }

    /// The human's Close.
    pub fn close_project(&self, project: i64) -> Result<ProjectView, EngineError> {
        self.recorder.op("closeProject", json!([project]));
        let dispatcher = Rc::clone(&self.dispatcher);
        self.run(async move { dispatcher.close_project(project).await })
    }

    /// The human's Resume.
    pub fn resume_project(&self, project: i64) -> Result<ProjectView, EngineError> {
        self.recorder.op("resumeProject", json!([project]));
        let dispatcher = Rc::clone(&self.dispatcher);
        self.run(async move { dispatcher.resume_project(project).await })
    }

    /// The human deletes a closed project.
    pub fn delete_project(&self, project: i64) -> Result<DeletedProject, EngineError> {
        self.recorder.op("deleteProject", json!([project]));
        let dispatcher = Rc::clone(&self.dispatcher);
        self.run(async move { dispatcher.delete_project(project).await })
    }

    /// The human takes a member off the staff.
    pub fn remove_member(&self, project: i64, handle: &str) -> Result<RemovedMember, EngineError> {
        self.recorder.op("removeMember", json!([project, handle]));
        let (dispatcher, handle) = (Rc::clone(&self.dispatcher), handle.to_owned());
        self.run(async move { dispatcher.remove_member(project, &handle).await })
    }

    /// The last window of `handle` exits, as the pane host says it.
    pub fn exit(&self, handle: &str) {
        let (host, handle) = (Rc::clone(&self.host), handle.to_owned());
        self.run(async move { host.exit(&handle).await });
    }

    /// An engine made again on the same ledger and fakes, as after a
    /// restart (`context.make()`): the pane host tells it exits too, and the
    /// context holds it until the test is closed.
    pub fn make(&self) -> Restarted<'_> {
        let engine = Dispatcher::new(self.seams.clone());
        self.host.attach(&engine);
        let made = Rc::downgrade(&engine);
        self.restarted.borrow_mut().push(engine);
        Restarted {
            context: self,
            engine: made,
        }
    }
}

/// An engine a restart made ([`Context::make`]).
pub struct Restarted<'a> {
    context: &'a Context,
    engine: Weak<Dispatcher>,
}

impl Restarted<'_> {
    fn engine(&self) -> Rc<Dispatcher> {
        self.engine.upgrade().expect("the context holds the engine")
    }

    /// One pass, everything it began run to stillness.
    pub fn pass(&self) -> Result<(), EngineError> {
        self.context.recorder.op("pass", json!([]));
        let engine = self.engine();
        self.context.run(async move { engine.pass().await })
    }

    /// What was on its way is settled, and the projects open before come back.
    pub fn resume_after_restart(&self) -> Result<Vec<Resumed>, EngineError> {
        self.context.recorder.op("resumeAfterRestart", json!([]));
        let engine = self.engine();
        self.context
            .run(async move { engine.resume_after_restart().await })
    }
}

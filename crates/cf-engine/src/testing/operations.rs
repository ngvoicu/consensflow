//! The engine's operations as a test calls them (the human's, the pane host's
//! exits, and a restart's engine): each written down where it begins, by its
//! Node name, as the Node recorder marks the dispatcher's operations a test
//! calls, and the work it began run to stillness. Calls a test made without
//! awaiting the first are begun where they were called and run after
//! ([`Context::begin_pass`]).

use std::rc::{Rc, Weak};

use cf_ledger::{
    DeletedProject, NewProject, ParticipantView, ProjectView, RemovedMember, TaskReleased,
};
use serde_json::{json, Value};

use super::context::Context;
use super::executor::Answer;
use crate::chief_switch::SwitchTo;
use crate::dispatcher::{Dispatcher, Resumed, SwitchWhen};
use crate::seams::EngineError;

/// What a Switch chief is asked, as `switchChief` is given it.
fn switch_asked(project: i64, to: &SwitchTo, when: SwitchWhen, note: bool) -> Value {
    let mut asked = json!({ "harness": to.harness, "agent": to.agent });
    if when == SwitchWhen::Turn {
        asked["when"] = json!("turn");
    }
    if note {
        asked["note"] = json!(true);
    }
    json!([project, asked])
}

impl Context {
    /// A pass begun where it is called, its first wait not passed:
    /// [`Context::finish`] or [`Context::settle`] runs the rest.
    pub fn begin_pass(&self) -> Answer<Result<(), EngineError>> {
        self.recorder.op("pass", json!([]));
        let dispatcher = Rc::clone(&self.dispatcher);
        self.executor
            .start_now(async move { dispatcher.pass().await })
    }

    /// A project opened (`openProject`), begun where it is called.
    pub fn begin_open_project(&self, request: Value) -> Answer<Result<ProjectView, EngineError>> {
        self.recorder.op("openProject", json!([request.clone()]));
        let dispatcher = Rc::clone(&self.dispatcher);
        self.executor.start_now(async move {
            let request = NewProject::from_json(&request)?;
            dispatcher.open_project(request).await
        })
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

    /// The human's Resume (`resumeProject`).
    pub fn resume_project(&self, project: i64) -> Result<ProjectView, EngineError> {
        self.recorder.op("resumeProject", json!([project]));
        let dispatcher = Rc::clone(&self.dispatcher);
        self.run(async move { dispatcher.resume_project(project).await })
    }

    /// The human's Close (`closeProject`).
    pub fn close_project(&self, project: i64) -> Result<ProjectView, EngineError> {
        self.recorder.op("closeProject", json!([project]));
        let dispatcher = Rc::clone(&self.dispatcher);
        self.run(async move { dispatcher.close_project(project).await })
    }

    /// The human's Close, begun where it is called.
    pub fn begin_close_project(&self, project: i64) -> Answer<Result<ProjectView, EngineError>> {
        self.recorder.op("closeProject", json!([project]));
        let dispatcher = Rc::clone(&self.dispatcher);
        self.executor
            .start_now(async move { dispatcher.close_project(project).await })
    }

    /// The human deletes a closed project (`deleteProject`).
    pub fn delete_project(&self, project: i64) -> Result<DeletedProject, EngineError> {
        self.recorder.op("deleteProject", json!([project]));
        let dispatcher = Rc::clone(&self.dispatcher);
        self.run(async move { dispatcher.delete_project(project).await })
    }

    /// The human deletes a closed project, begun where it is called.
    pub fn begin_delete_project(
        &self,
        project: i64,
    ) -> Answer<Result<DeletedProject, EngineError>> {
        self.recorder.op("deleteProject", json!([project]));
        let dispatcher = Rc::clone(&self.dispatcher);
        self.executor
            .start_now(async move { dispatcher.delete_project(project).await })
    }

    /// The human takes a member off the staff (`removeMember`).
    pub fn remove_member(&self, project: i64, handle: &str) -> Result<RemovedMember, EngineError> {
        self.recorder.op("removeMember", json!([project, handle]));
        let (dispatcher, handle) = (Rc::clone(&self.dispatcher), handle.to_owned());
        self.run(async move { dispatcher.remove_member(project, &handle).await })
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

    /// The human says a member out of quota is back (`backFromQuota`).
    pub fn back_from_quota(
        &self,
        project: i64,
        handle: &str,
    ) -> Result<ParticipantView, EngineError> {
        self.recorder.op("backFromQuota", json!([project, handle]));
        self.dispatcher.back_from_quota(project, handle)
    }

    /// The human's Switch chief (`switchChief`).
    pub fn switch_chief(
        &self,
        project: i64,
        to: SwitchTo,
        when: SwitchWhen,
        note: bool,
    ) -> Result<ProjectView, EngineError> {
        self.recorder
            .op("switchChief", switch_asked(project, &to, when, note));
        let dispatcher = Rc::clone(&self.dispatcher);
        self.run(async move { dispatcher.switch_chief(project, to, when, note).await })
    }

    /// The human's Switch chief, begun where it is called.
    pub fn begin_switch_chief(
        &self,
        project: i64,
        to: SwitchTo,
        when: SwitchWhen,
        note: bool,
    ) -> Answer<Result<ProjectView, EngineError>> {
        self.recorder
            .op("switchChief", switch_asked(project, &to, when, note));
        let dispatcher = Rc::clone(&self.dispatcher);
        self.executor
            .start_now(async move { dispatcher.switch_chief(project, to, when, note).await })
    }

    /// The last window of `handle` exits, as the pane host says it (`host.exit`).
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

    /// The human's Switch chief, everything it began run to stillness.
    pub fn switch_chief(
        &self,
        project: i64,
        to: SwitchTo,
        when: SwitchWhen,
        note: bool,
    ) -> Result<ProjectView, EngineError> {
        self.context
            .recorder
            .op("switchChief", switch_asked(project, &to, when, note));
        let engine = self.engine();
        self.context
            .run(async move { engine.switch_chief(project, to, when, note).await })
    }

    /// What was on its way is settled, and the projects open before come back.
    pub fn resume_after_restart(&self) -> Result<Vec<Resumed>, EngineError> {
        self.context.recorder.op("resumeAfterRestart", json!([]));
        let engine = self.engine();
        self.context
            .run(async move { engine.resume_after_restart().await })
    }
}

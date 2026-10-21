//! The engine's operations as a test calls them (the human's, the pane host's
//! exits, and a restart's engine, which the human's Resume then brings the
//! project back on): each written down where it begins, by its
//! Node name, as the Node recorder marks the dispatcher's operations a test
//! calls, with what it answered, or failed with, once it ends; and the work
//! it began run to stillness. Calls a test made without awaiting the first
//! are begun where they were called and run after ([`Context::begin_pass`]).

use std::future::Future;
use std::rc::{Rc, Weak};

use cf_harness::contract::Pane;
use cf_ledger::{
    DeletedProject, NewProject, ParticipantView, ProjectView, RemovedMember, TaskReleased,
};
use serde::Serialize;
use serde_json::{json, Value};

use super::context::Context;
use super::executor::Answer;
use crate::chief_switch::SwitchTo;
use crate::dispatcher::{Dispatcher, SwitchWhen};
use crate::seams::EngineError;
use crate::windows::Activity;

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

/// An answer as the Node recorder writes it: the JSON the ledger's views are.
pub(super) fn answer<T: Serialize>(answer: &T) -> Value {
    serde_json::to_value(answer).expect("an answer as JSON")
}

/// What an operation that answers nothing answered: JavaScript's `undefined`.
pub(super) fn nothing(_: &()) -> Value {
    json!({ "$undefined": true })
}

impl Context {
    /// Writes down an operation where it begins, and its answer, as `told`
    /// writes it, or its failure once it ends: the work, as it ran.
    pub(super) fn operation<T: 'static>(
        &self,
        name: &str,
        args: Value,
        told: fn(&T) -> Value,
        work: impl Future<Output = Result<T, EngineError>> + 'static,
    ) -> impl Future<Output = Result<T, EngineError>> + 'static {
        let (at, recorder) = (self.recorder.op(name, args), self.recorder.clone());
        async move {
            let result = work.await;
            match &result {
                Ok(done) => recorder.answered(at, told(done)),
                Err(cause) => recorder.threw(at, &cause.to_string()),
            }
            result
        }
    }

    /// A pass begun where it is called, its first wait not passed:
    /// [`Context::finish`] or [`Context::settle`] runs the rest.
    pub fn begin_pass(&self) -> Answer<Result<(), EngineError>> {
        let dispatcher = Rc::clone(&self.dispatcher);
        let pass = self.operation("pass", json!([]), nothing, async move {
            dispatcher.pass().await
        });
        self.executor.start_now(pass)
    }

    /// A project opened (`openProject`), begun where it is called.
    pub fn begin_open_project(
        &self,
        request: Value,
    ) -> Answer<Result<Option<ProjectView>, EngineError>> {
        let dispatcher = Rc::clone(&self.dispatcher);
        let asked = request.clone();
        let open = self.operation("openProject", json!([request]), answer, async move {
            let request = NewProject::from_json(&asked)?;
            dispatcher.open_project(request).await
        });
        self.executor.start_now(open)
    }

    /// A member taken off the staff, begun where it is called.
    pub fn begin_remove_member(
        &self,
        project: i64,
        handle: &str,
    ) -> Answer<Result<RemovedMember, EngineError>> {
        let (dispatcher, handle) = (Rc::clone(&self.dispatcher), handle.to_owned());
        let removal = self.operation(
            "removeMember",
            json!([project, handle]),
            answer,
            async move { dispatcher.remove_member(project, &handle).await },
        );
        self.executor.start_now(removal)
    }

    /// The human's Resume (`resumeProject`).
    pub fn resume_project(&self, project: i64) -> Result<Option<ProjectView>, EngineError> {
        let dispatcher = Rc::clone(&self.dispatcher);
        let resume = self.operation("resumeProject", json!([project]), answer, async move {
            dispatcher.resume_project(project).await
        });
        self.run(resume)
    }

    /// The human's Close (`closeProject`).
    pub fn close_project(&self, project: i64) -> Result<Option<ProjectView>, EngineError> {
        self.run(self.closing(project))
    }

    /// The human's Close, begun where it is called.
    pub fn begin_close_project(
        &self,
        project: i64,
    ) -> Answer<Result<Option<ProjectView>, EngineError>> {
        self.executor.start_now(self.closing(project))
    }

    fn closing(
        &self,
        project: i64,
    ) -> impl Future<Output = Result<Option<ProjectView>, EngineError>> + 'static {
        let dispatcher = Rc::clone(&self.dispatcher);
        self.operation("closeProject", json!([project]), answer, async move {
            dispatcher.close_project(project).await
        })
    }

    /// The human deletes a closed project (`deleteProject`).
    pub fn delete_project(&self, project: i64) -> Result<DeletedProject, EngineError> {
        self.run(self.deleting(project))
    }

    /// The human deletes a closed project, begun where it is called.
    pub fn begin_delete_project(
        &self,
        project: i64,
    ) -> Answer<Result<DeletedProject, EngineError>> {
        self.executor.start_now(self.deleting(project))
    }

    fn deleting(
        &self,
        project: i64,
    ) -> impl Future<Output = Result<DeletedProject, EngineError>> + 'static {
        let dispatcher = Rc::clone(&self.dispatcher);
        self.operation("deleteProject", json!([project]), answer, async move {
            dispatcher.delete_project(project).await
        })
    }

    /// The human takes a member off the staff (`removeMember`).
    pub fn remove_member(&self, project: i64, handle: &str) -> Result<RemovedMember, EngineError> {
        let (dispatcher, handle) = (Rc::clone(&self.dispatcher), handle.to_owned());
        let removal = self.operation(
            "removeMember",
            json!([project, handle]),
            answer,
            async move { dispatcher.remove_member(project, &handle).await },
        );
        self.run(removal)
    }

    /// The human opens a session's window (`openWindow`).
    pub fn open_window(
        &self,
        project: i64,
        handle: &str,
    ) -> Result<Option<ProjectView>, EngineError> {
        let (dispatcher, handle) = (Rc::clone(&self.dispatcher), handle.to_owned());
        let opening = self.operation("openWindow", json!([project, handle]), answer, async move {
            dispatcher.open_window(project, &handle).await
        });
        self.run(opening)
    }

    /// The human hides a session's terminal (`hideWindow`).
    pub fn hide_window(
        &self,
        project: i64,
        handle: &str,
    ) -> Result<Option<ProjectView>, EngineError> {
        let (dispatcher, handle) = (Rc::clone(&self.dispatcher), handle.to_owned());
        let hiding = self.operation("hideWindow", json!([project, handle]), answer, async move {
            dispatcher.hide_window(project, &handle).await
        });
        self.run(hiding)
    }

    /// The human gives a task in a window to another member (`reassignTask`).
    pub fn reassign_task(&self, project: i64, number: i64) -> Result<TaskReleased, EngineError> {
        let dispatcher = Rc::clone(&self.dispatcher);
        let reassigning = self.operation(
            "reassignTask",
            json!([project, number]),
            answer,
            async move { dispatcher.reassign_task(project, number).await },
        );
        self.run(reassigning)
    }

    /// The human deletes a session (`endSession`).
    pub fn end_session(&self, project: i64, handle: &str) -> Result<ProjectView, EngineError> {
        let (dispatcher, handle) = (Rc::clone(&self.dispatcher), handle.to_owned());
        let ending = self.operation("endSession", json!([project, handle]), answer, async move {
            dispatcher.end_session(project, &handle).await
        });
        self.run(ending)
    }

    /// The human says a member out of quota is back (`backFromQuota`).
    pub fn back_from_quota(
        &self,
        project: i64,
        handle: &str,
    ) -> Result<ParticipantView, EngineError> {
        let at = self.recorder.op("backFromQuota", json!([project, handle]));
        let back = self.dispatcher.back_from_quota(project, handle);
        match &back {
            Ok(member) => self.recorder.answered(at, answer(member)),
            Err(cause) => self.recorder.threw(at, &cause.to_string()),
        }
        back
    }

    /// The human's Switch chief (`switchChief`).
    pub fn switch_chief(
        &self,
        project: i64,
        to: SwitchTo,
        when: SwitchWhen,
        note: bool,
    ) -> Result<Option<ProjectView>, EngineError> {
        self.run(self.switching(project, to, when, note))
    }

    /// The human's Switch chief, begun where it is called.
    pub fn begin_switch_chief(
        &self,
        project: i64,
        to: SwitchTo,
        when: SwitchWhen,
        note: bool,
    ) -> Answer<Result<Option<ProjectView>, EngineError>> {
        self.executor
            .start_now(self.switching(project, to, when, note))
    }

    fn switching(
        &self,
        project: i64,
        to: SwitchTo,
        when: SwitchWhen,
        note: bool,
    ) -> impl Future<Output = Result<Option<ProjectView>, EngineError>> + 'static {
        let dispatcher = Rc::clone(&self.dispatcher);
        let asked = switch_asked(project, &to, when, note);
        self.operation("switchChief", asked, answer, async move {
            dispatcher.switch_chief(project, to, when, note).await
        })
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

    /// What this engine says the participant's window is doing.
    pub fn activity(&self, participant: i64) -> Activity {
        self.engine().activity(participant)
    }

    /// The pane this engine holds the participant's window in, if any.
    pub fn pane(&self, participant: i64) -> Option<Pane> {
        self.engine().pane(participant)
    }

    /// One pass, everything it began run to stillness.
    pub fn pass(&self) -> Result<(), EngineError> {
        self.context.run(self.passing())
    }

    /// One pass, begun where it is called.
    pub fn begin_pass(&self) -> Answer<Result<(), EngineError>> {
        self.context.executor.start_now(self.passing())
    }

    fn passing(&self) -> impl Future<Output = Result<(), EngineError>> + 'static {
        let engine = self.engine();
        self.context.operation(
            "pass",
            json!([]),
            nothing,
            async move { engine.pass().await },
        )
    }

    /// The human's Switch chief, everything it began run to stillness.
    pub fn switch_chief(
        &self,
        project: i64,
        to: SwitchTo,
        when: SwitchWhen,
        note: bool,
    ) -> Result<Option<ProjectView>, EngineError> {
        let engine = self.engine();
        let asked = switch_asked(project, &to, when, note);
        let switching = self
            .context
            .operation("switchChief", asked, answer, async move {
                engine.switch_chief(project, to, when, note).await
            });
        self.context.run(switching)
    }

    /// What was on its way is settled, and nothing else comes of the projects
    /// that were open: they wait for the human's Resume.
    pub fn settle_after_restart(&self) -> Result<(), EngineError> {
        self.context.run(self.settling())
    }

    /// What was on its way is settled, begun where it is called.
    pub fn begin_settle_after_restart(&self) -> Answer<Result<(), EngineError>> {
        self.context.executor.start_now(self.settling())
    }

    fn settling(&self) -> impl Future<Output = Result<(), EngineError>> + 'static {
        let engine = self.engine();
        self.context
            .operation("settleAfterRestart", json!([]), nothing, async move {
                engine.settle_after_restart().await
            })
    }

    /// The human's Resume, of a project this engine's start left suspended.
    pub fn resume_project(&self, project: i64) -> Result<Option<ProjectView>, EngineError> {
        self.context.run(self.resuming(project))
    }

    /// The human's Resume, begun where it is called.
    pub fn begin_resume_project(
        &self,
        project: i64,
    ) -> Answer<Result<Option<ProjectView>, EngineError>> {
        self.context.executor.start_now(self.resuming(project))
    }

    fn resuming(
        &self,
        project: i64,
    ) -> impl Future<Output = Result<Option<ProjectView>, EngineError>> + 'static {
        let engine = self.engine();
        self.context
            .operation("resumeProject", json!([project]), answer, async move {
                engine.resume_project(project).await
            })
    }
}

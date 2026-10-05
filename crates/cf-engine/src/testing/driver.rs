//! A dispatcher a test drives, as `settled` in `core-dispatcher.test.mjs`
//! makes one: each operation of the human's is begun where the test calls it,
//! as JavaScript ran an async call to its first wait, and written down there;
//! the test either waits for it ([`Driver::pass`]) or begins others meanwhile
//! ([`Driver::begin_pass`]) and lets everything run to stillness when it
//! says ([`Driver::settle`]), as one turn of the event loop did.

use std::future::Future;
use std::rc::Rc;

use cf_ledger::{DeletedProject, NewProject, ProjectView};
use serde_json::{json, Value};

use super::executor::{Executor, Pending};
use super::recorder::Recorder;
use crate::chief_switch::SwitchTo;
use crate::dispatcher::{Dispatcher, Resumed, SwitchWhen};
use crate::seams::EngineError;

/// A dispatcher, and the test's executor and record of what it was asked.
pub struct Driver {
    executor: Rc<Executor>,
    recorder: Recorder,
    pub dispatcher: Rc<Dispatcher>,
}

impl Driver {
    pub(crate) fn new(
        executor: Rc<Executor>,
        recorder: Recorder,
        dispatcher: Rc<Dispatcher>,
    ) -> Self {
        Self {
            executor,
            recorder,
            dispatcher,
        }
    }

    /// Runs everything begun to stillness: what waits on nothing the test
    /// holds is over.
    pub fn settle(&self) {
        self.executor.run();
    }

    /// Begins an operation: written down where it begins, its first part done here.
    fn operation<T: 'static>(
        &self,
        name: &str,
        args: Value,
        work: impl Future<Output = T> + 'static,
    ) -> Pending<T> {
        self.recorder.op(name, args);
        self.executor.begin(work)
    }

    /// The answer of an operation, once everything has run to stillness.
    fn answer<T>(&self, pending: &Pending<T>) -> T {
        self.settle();
        pending
            .answer()
            .expect("the work waits on nothing the test releases")
    }

    /// One pass, begun.
    pub fn begin_pass(&self) -> Pending<Result<(), EngineError>> {
        let dispatcher = Rc::clone(&self.dispatcher);
        self.operation("pass", json!([]), async move { dispatcher.pass().await })
    }

    /// One pass, everything it began run to stillness.
    pub fn pass(&self) -> Result<(), EngineError> {
        self.answer(&self.begin_pass())
    }

    /// A project opened (`dispatcher.openProject`), `request` as the API
    /// gives it, begun.
    pub fn begin_open_project(&self, request: Value) -> Pending<Result<ProjectView, EngineError>> {
        let dispatcher = Rc::clone(&self.dispatcher);
        let args = json!([request.clone()]);
        self.operation("openProject", args, async move {
            let request = NewProject::from_json(&request)?;
            dispatcher.open_project(request).await
        })
    }

    /// A project opened, everything it began run to stillness.
    pub fn open_project(&self, request: Value) -> Result<ProjectView, EngineError> {
        self.answer(&self.begin_open_project(request))
    }

    /// Switch chief (`dispatcher.switchChief`), begun.
    pub fn begin_switch_chief(
        &self,
        project: i64,
        to: SwitchTo,
        when: SwitchWhen,
        note: bool,
    ) -> Pending<Result<ProjectView, EngineError>> {
        let mut asked = json!({ "harness": to.harness, "agent": to.agent });
        if when == SwitchWhen::Turn {
            asked["when"] = json!("turn");
        }
        if note {
            asked["note"] = json!(true);
        }
        let dispatcher = Rc::clone(&self.dispatcher);
        self.operation("switchChief", json!([project, asked]), async move {
            dispatcher.switch_chief(project, to, when, note).await
        })
    }

    /// Switch chief, everything it began run to stillness.
    pub fn switch_chief(
        &self,
        project: i64,
        to: SwitchTo,
        when: SwitchWhen,
        note: bool,
    ) -> Result<ProjectView, EngineError> {
        self.answer(&self.begin_switch_chief(project, to, when, note))
    }

    /// The human's Close, begun.
    pub fn begin_close_project(&self, project: i64) -> Pending<Result<ProjectView, EngineError>> {
        let dispatcher = Rc::clone(&self.dispatcher);
        self.operation("closeProject", json!([project]), async move {
            dispatcher.close_project(project).await
        })
    }

    /// The human's Close, everything it began run to stillness.
    pub fn close_project(&self, project: i64) -> Result<ProjectView, EngineError> {
        self.answer(&self.begin_close_project(project))
    }

    /// The human's Resume, begun.
    pub fn begin_resume_project(&self, project: i64) -> Pending<Result<ProjectView, EngineError>> {
        let dispatcher = Rc::clone(&self.dispatcher);
        self.operation("resumeProject", json!([project]), async move {
            dispatcher.resume_project(project).await
        })
    }

    /// The human's Resume, everything it began run to stillness.
    pub fn resume_project(&self, project: i64) -> Result<ProjectView, EngineError> {
        self.answer(&self.begin_resume_project(project))
    }

    /// A closed project deleted, begun.
    pub fn begin_delete_project(
        &self,
        project: i64,
    ) -> Pending<Result<DeletedProject, EngineError>> {
        let dispatcher = Rc::clone(&self.dispatcher);
        self.operation("deleteProject", json!([project]), async move {
            dispatcher.delete_project(project).await
        })
    }

    /// A closed project deleted, everything it began run to stillness.
    pub fn delete_project(&self, project: i64) -> Result<DeletedProject, EngineError> {
        self.answer(&self.begin_delete_project(project))
    }

    /// What was on its way when the previous process ended is settled and
    /// the projects open then come back, everything run to stillness.
    pub fn resume_after_restart(&self) -> Result<Vec<Resumed>, EngineError> {
        let dispatcher = Rc::clone(&self.dispatcher);
        let pending = self.operation("resumeAfterRestart", json!([]), async move {
            dispatcher.resume_after_restart().await
        });
        self.answer(&pending)
    }
}

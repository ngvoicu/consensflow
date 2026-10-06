//! The dispatcher of the Node tests (`core-page.test.mjs:42-107`,
//! `corners-page.test.mjs:42-73`), as the engine the page is given: each call
//! the page makes on it is the next one Node's operation made, asserted by its
//! name and its arguments, and answered as Node's answered, with the ledger
//! calls its stand-in made made again on the player's ledger, in their place
//! among the ledger's own.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::future::ready;
use std::rc::Rc;

use cf_base::refusal::Refusal;
use cf_daemon::page::Engine;
use cf_engine::seams::EngineError;
use cf_engine::{SwitchTo, SwitchWhen};
use cf_harness::contract::Work;
use cf_ledger::{
    ChiefSwitch, DeletedProject, LedgerError, NewProject, ParticipantView, ProjectView,
    RemovedMember, TaskReleased,
};
use serde::Serialize;
use serde_json::{json, Value};

use crate::ledger::{differs, Rig};

/// The engine of a trace: what its operations call, as Node's stand-ins answered.
pub struct Standin {
    rig: Rc<Rig>,
    seams: RefCell<VecDeque<Value>>,
}

/// One call of the stand-in, whose answer is Node's: the ledger calls it made
/// are still to be made.
struct Seam {
    rig: Rc<Rig>,
    recorded: Value,
    calls: VecDeque<Value>,
}

impl Standin {
    pub fn new(rig: Rc<Rig>) -> Self {
        Self {
            rig,
            seams: RefCell::new(VecDeque::new()),
        }
    }

    /// What the next operation is to call, in order.
    pub fn expect(&self, seams: Vec<Value>) {
        *self.seams.borrow_mut() = seams.into();
    }

    /// After the operation: what it did not call that Node's did.
    pub fn leftover(&self) -> Vec<String> {
        self.seams
            .borrow_mut()
            .drain(..)
            .map(|seam| format!("never called {}", named(&seam)))
            .collect()
    }

    /// The call the page makes now: it is the next one Node's operation made,
    /// with these arguments.
    fn seam(&self, method: &str, args: Vec<Value>) -> Seam {
        let Some(recorded) = self.seams.borrow_mut().pop_front() else {
            self.rig
                .problem(format!("called {method} past Node's last call"));
            return Seam::none(&self.rig);
        };
        if recorded["method"] != method || recorded["seam"] != "dispatcher" {
            self.rig.problem(format!(
                "called dispatcher.{method}, Node {}",
                named(&recorded)
            ));
        }
        // The arguments are compared as values: a request's keys are in the
        // order Rust's request holds them, as Node's were in its own.
        let asked = Value::Array(args);
        let given = normal(&recorded["args"]);
        if asked != given {
            let what = format!("{method}'s arguments");
            if let Some(why) = differs(&what, &asked.to_string(), &given.to_string()) {
                self.rig.problem(why);
            }
        }
        Seam {
            rig: Rc::clone(&self.rig),
            calls: recorded["calls"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into(),
            recorded,
        }
    }

    /// A projection or a check, answered as Node's stand-in answered it.
    fn recorded(&self, method: &str, args: Vec<Value>) -> Value {
        let seam = self.seam(method, args);
        seam.expect_no_calls();
        seam.recorded["result"].clone()
    }
}

/// A recorded call by its name: `dispatcher.pane`.
fn named(seam: &Value) -> String {
    let word = |field: &str| seam[field].as_str().unwrap_or("?").to_owned();
    format!("{}.{}", word("seam"), word("method"))
}

/// The recorder's `{"$undefined": true}` for a flag that was not given: `false`.
fn normal(args: &Value) -> Value {
    match args {
        Value::Object(fields) => Value::Object(
            fields
                .iter()
                .map(|(key, value)| {
                    let said = if key == "gate" && value.get("$undefined").is_some() {
                        Value::Bool(false)
                    } else {
                        normal(value)
                    };
                    (key.clone(), said)
                })
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(normal).collect()),
        other => other.clone(),
    }
}

impl Seam {
    /// A call there was none of: the problem is noted, and the answer is empty.
    fn none(rig: &Rc<Rig>) -> Self {
        Self {
            rig: Rc::clone(rig),
            recorded: json!({}),
            calls: VecDeque::new(),
        }
    }

    /// The next ledger call Node's stand-in made, made here.
    fn call<T: Serialize>(
        &mut self,
        method: &str,
        make: impl FnOnce(&mut cf_ledger::Ledger) -> Result<T, LedgerError>,
    ) -> Result<T, LedgerError> {
        match self.calls.pop_front() {
            Some(recorded) => self.rig.call(&recorded, method, make),
            None => {
                self.rig.problem(format!(
                    "made ledger {method}, which Node's stand-in did not"
                ));
                make(&mut self.rig.ledger.borrow_mut())
            }
        }
    }

    fn expect_no_calls(&self) {
        for call in &self.calls {
            self.rig.problem(format!(
                "never made the ledger call {} that Node's stand-in made",
                call["method"]
            ));
        }
    }

    /// What the call answered, held to what Node's did: a value, or the words of a refusal.
    fn settle<T: Serialize>(self, answered: &Result<T, EngineError>) {
        self.expect_no_calls();
        let said = &self.recorded;
        match answered {
            Ok(value) => {
                if said.get("refusal").is_some() {
                    self.rig
                        .problem(format!("answered, where Node refused: {}", said["refusal"]));
                    return;
                }
                let ours = serde_json::to_value(value).unwrap_or(Value::Null);
                let theirs = if said["result"].get("$undefined").is_some() {
                    Value::Null
                } else {
                    said["result"].clone()
                };
                if let Some(why) = differs("the answer", &ours.to_string(), &theirs.to_string()) {
                    self.rig.problem(why);
                }
            }
            Err(error) => {
                if said["refusal"]["message"] != error.to_string() {
                    self.rig
                        .problem(format!("refused with {error:?}, Node {}", said["refusal"]));
                }
            }
        }
    }
}

/// What the dispatcher's own refusals (a string thrown, an `Error`) become here.
fn refused(words: impl Into<String>) -> EngineError {
    EngineError::Refused(Refusal::new("stand-in", words))
}

/// The recorded refusal of a call that made no ledger call, when there is one.
fn thrown(seam: &Seam) -> Option<EngineError> {
    seam.recorded["refusal"]["message"].as_str().map(refused)
}

/// A request as Node's stand-in saw it: `{directory, name, chief, gate, staff}`.
fn project_json(request: &NewProject) -> Value {
    let staff: Vec<Value> = request
        .staff
        .iter()
        .map(|member| {
            json!({
                "agent": member.agent,
                "harness": member.harness,
                "designer": member.designer,
                "tier": member.tier,
                "roles": member.roles,
            })
        })
        .collect();
    json!({
        "directory": request.directory,
        "name": request.name,
        "chief": { "harness": request.chief.harness, "agent": request.chief.agent },
        "gate": request.gate,
        "staff": staff,
    })
}

impl Engine for Standin {
    fn open_project(
        &self,
        request: NewProject,
    ) -> Work<'_, Result<Option<ProjectView>, EngineError>> {
        let mut seam = self.seam("openProject", vec![project_json(&request)]);
        let answered = seam
            .call("createProject", |ledger| ledger.create_project(&request))
            .map(Some)
            .map_err(EngineError::from);
        seam.settle(&answered);
        Box::pin(ready(answered))
    }

    fn resume_project(&self, project: i64) -> Work<'_, Result<Option<ProjectView>, EngineError>> {
        let mut seam = self.seam("resumeProject", vec![json!(project)]);
        let answered = seam
            .call("setProjectState", |ledger| {
                ledger.set_project_state(project, "open")
            })
            .map(Some)
            .map_err(EngineError::from);
        seam.settle(&answered);
        Box::pin(ready(answered))
    }

    fn close_project(&self, project: i64) -> Work<'_, Result<Option<ProjectView>, EngineError>> {
        let mut seam = self.seam("closeProject", vec![json!(project)]);
        let answered = seam
            .call("setProjectState", |ledger| {
                ledger.set_project_state(project, "suspended")
            })
            .map(Some)
            .map_err(EngineError::from);
        seam.settle(&answered);
        Box::pin(ready(answered))
    }

    fn delete_project(&self, project: i64) -> Work<'_, Result<DeletedProject, EngineError>> {
        let mut seam = self.seam("deleteProject", vec![json!(project)]);
        let answered = seam
            .call("deleteProject", |ledger| ledger.delete_project(project))
            .map_err(EngineError::from);
        seam.settle(&answered);
        Box::pin(ready(answered))
    }

    fn switch_chief(
        &self,
        project: i64,
        to: SwitchTo,
        when: SwitchWhen,
        note: bool,
    ) -> Work<'_, Result<Option<ProjectView>, EngineError>> {
        let when = match when {
            SwitchWhen::Now => "now",
            SwitchWhen::Turn => "turn",
        };
        let request =
            json!({ "harness": to.harness, "agent": to.agent, "when": when, "note": note });
        let mut seam = self.seam("switchChief", vec![json!(project), request]);
        let switch = ChiefSwitch {
            harness: to.harness,
            agent: to.agent,
            cut: false,
        };
        let answered = seam
            .call("switchChief", |ledger| {
                ledger.switch_chief(project, &switch)
            })
            .map(Some)
            .map_err(EngineError::from);
        seam.settle(&answered);
        Box::pin(ready(answered))
    }

    fn open_window<'a>(
        &'a self,
        project: i64,
        handle: &'a str,
    ) -> Work<'a, Result<Option<ProjectView>, EngineError>> {
        Box::pin(ready(self.window("openWindow", project, handle)))
    }

    fn hide_window<'a>(
        &'a self,
        project: i64,
        handle: &'a str,
    ) -> Work<'a, Result<Option<ProjectView>, EngineError>> {
        Box::pin(ready(self.window("hideWindow", project, handle)))
    }

    fn end_session<'a>(
        &'a self,
        project: i64,
        handle: &'a str,
    ) -> Work<'a, Result<ProjectView, EngineError>> {
        let mut seam = self.seam("endSession", vec![json!(project), json!(handle)]);
        let answered = seam
            .call("endSession", |ledger| {
                ledger.end_session(project, handle, "human")
            })
            .map_err(EngineError::from);
        seam.settle(&answered);
        Box::pin(ready(answered))
    }

    fn remove_member<'a>(
        &'a self,
        project: i64,
        handle: &'a str,
    ) -> Work<'a, Result<RemovedMember, EngineError>> {
        let mut seam = self.seam("removeMember", vec![json!(project), json!(handle)]);
        let answered = seam
            .call("removeMember", |ledger| {
                ledger.remove_member(project, handle)
            })
            .map_err(EngineError::from);
        seam.settle(&answered);
        Box::pin(ready(answered))
    }

    fn reassign_task(
        &self,
        project: i64,
        number: i64,
    ) -> Work<'_, Result<TaskReleased, EngineError>> {
        let mut seam = self.seam("reassignTask", vec![json!(project), json!(number)]);
        let answered = seam
            .call("releaseTask", |ledger| {
                ledger.release_task(project, number, "by @human")
            })
            .map_err(EngineError::from);
        seam.settle(&answered);
        Box::pin(ready(answered))
    }

    fn back_from_quota(&self, project: i64, handle: &str) -> Result<ParticipantView, EngineError> {
        let mut seam = self.seam("backFromQuota", vec![json!(project), json!(handle)]);
        let found = seam.call("project", |ledger| ledger.project(project));
        let participant = found
            .ok()
            .flatten()
            .and_then(|found| found.participants.into_iter().find(|p| p.handle == handle));
        let answered = match participant {
            Some(participant) if participant.role != "human" => {
                let owner = participant.member_id.unwrap_or(participant.id);
                seam.call("markBack", |ledger| ledger.mark_back(owner, "by @human"))
                    .map_err(EngineError::from)
            }
            _ => Err(refused(format!("no @{handle} in project {project}"))),
        };
        seam.settle(&answered);
        answered
    }

    fn require_adapter(&self, harness: &str) -> Result<(), EngineError> {
        let seam = self.seam("requireAdapter", vec![json!(harness)]);
        seam.expect_no_calls();
        thrown(&seam).map_or(Ok(()), Err)
    }

    fn activity(&self, participant: i64) -> Value {
        self.recorded("activity", vec![json!(participant)])
    }

    fn holding(&self, participant: i64) -> Result<bool, EngineError> {
        Ok(self.recorded("holding", vec![json!(participant)]) == true)
    }

    fn hidden(&self, participant: i64) -> bool {
        self.recorded("hidden", vec![json!(participant)]) == true
    }

    fn pending_switch(&self, participant: i64) -> Option<Value> {
        Some(self.recorded("pendingSwitch", vec![json!(participant)]))
            .filter(|shown| !shown.is_null())
    }

    fn pane(&self, participant: i64) -> Option<Value> {
        Some(self.recorded("pane", vec![json!(participant)])).filter(|shown| !shown.is_null())
    }
}

impl Standin {
    /// `openWindow` and `hideWindow` of the stand-ins: the project as the ledger
    /// has it, or what Node's threw (`corners-page.test.mjs:64`).
    fn window(
        &self,
        method: &str,
        project: i64,
        handle: &str,
    ) -> Result<Option<ProjectView>, EngineError> {
        let mut seam = self.seam(method, vec![json!(project), json!(handle)]);
        if let Some(thrown) = thrown(&seam) {
            seam.expect_no_calls();
            return Err(thrown);
        }
        let answered = seam
            .call("project", |ledger| ledger.project(project))
            .map_err(EngineError::from);
        seam.settle(&answered);
        answered
    }
}

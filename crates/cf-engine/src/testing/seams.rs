//! The engine's other seams as `core-dispatcher.test.mjs` makes them: a
//! token named after its window's handle, each window's environment naming
//! its participant, the saved agents' models, a role text per role, and a
//! trace, a log and launch files that keep what they are told. Every call is
//! written down in the Node traces' shape.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;

use cf_base::refusal::Refusal;
use cf_harness::contract::LaunchId;
use cf_ledger::{Ledger, ParticipantView, ProjectView};
use cf_proto::trace::{TraceLine, Traced};
use serde_json::{json, Value};

use super::recorder::Recorder;
use crate::seams::{
    Credentials, LaunchFiles, LaunchIds, Log, Operations, PaneEnv, Roles, Roster, SavedAgent, Trace,
};

/// The saved model of each fake agent, as the roster gives it at launch.
pub const MODELS: [(&str, &str); 6] = [
    ("apollo", "claude-opus-5"),
    ("zeus", "claude-opus-5"),
    ("diana", "gpt-5.6-luna"),
    ("hera", "muse-spark"),
    ("calliope", "claude-opus-5"),
    ("astraeus", "gpt-6-astra"),
];

/// A window's token, `token-<handle>`.
pub struct FakeCredentials {
    pub(crate) recorder: Recorder,
    pub(crate) ledger: Rc<RefCell<Ledger>>,
}

impl Credentials for FakeCredentials {
    fn issue(&self, project: i64, participant: i64) -> String {
        let handle = self
            .ledger
            .borrow()
            .project(project)
            .ok()
            .flatten()
            .and_then(|found| found.participants.into_iter().find(|p| p.id == participant))
            .map(|found| found.handle)
            .unwrap_or_default();
        let token = format!("token-{handle}");
        self.recorder.called(
            "credentials",
            Some("issue"),
            json!([{ "participant": { "id": participant, "handle": handle }, "project": { "id": project } }]),
            json!(token),
        );
        token
    }

    fn revoke(&self, token: &str) {
        self.recorder
            .called("credentials", Some("revoke"), json!([token]), Value::Null);
    }
}

/// Each window's environment names its participant.
pub struct FakePaneEnv {
    pub(crate) recorder: Recorder,
}

impl PaneEnv for FakePaneEnv {
    fn env(&self, participant: &ParticipantView, _project: &ProjectView) -> Vec<(String, String)> {
        let env = vec![(
            "CONSENSFLOW_PARTICIPANT".to_owned(),
            participant.handle.clone(),
        )];
        self.recorder.called(
            "paneEnv",
            None,
            json!([{ "handle": participant.handle }]),
            json!({ "CONSENSFLOW_PARTICIPANT": participant.handle }),
        );
        env
    }
}

/// A role text per role: `instructions for <role>`.
pub struct FakeRoles {
    pub(crate) recorder: Recorder,
}

impl Roles for FakeRoles {
    fn instructions(
        &self,
        participant: &ParticipantView,
        _project: &ProjectView,
    ) -> Result<String, String> {
        let text = format!("instructions for {}", participant.role);
        self.recorder.called(
            "roles",
            None,
            json!([{ "handle": participant.handle, "role": participant.role }]),
            json!(text),
        );
        Ok(text)
    }
}

/// The saved agents: each fake agent on its model, unless the test took it
/// away or broke the agents file.
pub struct FakeRoster {
    pub(crate) recorder: Recorder,
    /// Agents the human deleted.
    pub gone: RefCell<HashSet<String>>,
    /// Agents that are image agents.
    pub designers: RefCell<HashSet<String>>,
    /// The agents file cannot be read.
    pub broken: Cell<bool>,
}

impl Roster for FakeRoster {
    fn agent(&self, name: &str) -> Result<Option<SavedAgent>, Refusal> {
        let at = self.recorder.call("roster", None, json!([name]));
        if self.broken.get() {
            let refusal = Refusal::new("agents-unreadable", "the agents file cannot be read");
            self.recorder
                .answered(at, json!({ "$error": { "message": refusal.message } }));
            return Err(refusal);
        }
        if self.gone.borrow().contains(name) {
            self.recorder.answered(at, Value::Null);
            return Ok(None);
        }
        let model = MODELS
            .iter()
            .find(|(agent, _)| *agent == name)
            .map(|(_, model)| (*model).to_owned());
        let designer = self.designers.borrow().contains(name);
        self.recorder.answered(
            at,
            json!({ "id": name, "model": model, "designer": designer }),
        );
        Ok(Some(SavedAgent {
            model,
            effort: None,
            thinking: None,
            designer,
        }))
    }
}

/// The trace: every line kept, in order.
pub struct FakeTrace {
    pub(crate) recorder: Recorder,
    pub lines: RefCell<Vec<TraceLine>>,
    /// It forgets a deleted project's lines, as the event file in the home
    /// does; without, it has no `forget`, as the Node tests' own trace.
    pub forgets: Cell<bool>,
    /// The projects it forgot, in order.
    pub forgotten: RefCell<Vec<i64>>,
}

impl Trace for FakeTrace {
    fn line(&self, line: TraceLine) {
        let written = serde_json::to_value(&line).unwrap_or(Value::Null);
        self.recorder
            .called("trace", None, json!([written]), Value::Null);
        self.lines.borrow_mut().push(line);
    }

    fn forget(&self, project: i64) {
        if !self.forgets.get() {
            return;
        }
        self.recorder
            .called("trace", Some("forget"), json!([project]), Value::Null);
        self.forgotten.borrow_mut().push(project);
        self.lines.borrow_mut().retain(
            |line| !matches!(&line.what, Traced::Window { project: of, .. } if *of == Some(project)),
        );
    }
}

/// The engine's own calls of its operations (`paneExited` from a launch or a
/// close, `resumeProject` from a restart), written down where they begin as
/// the human's and the host's are.
pub struct FakeOperations {
    pub(crate) recorder: Recorder,
}

impl Operations for FakeOperations {
    fn called(&self, operation: &str, args: Value) {
        self.recorder.op(operation, args);
    }
}

/// The log: what failed apart from any pass, which no test may leave.
pub struct FakeLog {
    pub(crate) recorder: Recorder,
    pub failures: RefCell<Vec<String>>,
}

impl Log for FakeLog {
    fn error(&self, message: &str, cause: &str) {
        self.recorder
            .called("log", Some("error"), json!([message, cause]), Value::Null);
        self.failures.borrow_mut().push(cause.to_owned());
    }
}

/// The launch files: each launch forgotten, kept in order.
pub struct FakeLaunchFiles {
    pub(crate) recorder: Recorder,
    pub forgotten: RefCell<Vec<String>>,
}

impl LaunchFiles for FakeLaunchFiles {
    fn forget(&self, launch: &LaunchId) {
        self.recorder.called(
            "launchFiles",
            Some("forget"),
            json!([launch.as_str()]),
            Value::Null,
        );
        self.forgotten.borrow_mut().push(launch.as_str().to_owned());
    }
}

/// The launch ids a test draws: the n-th `00000000-0000-4000-8000-00000000000n`,
/// as the Node recorder hands them out.
#[derive(Default)]
pub struct CountingLaunchIds {
    drawn: Cell<u64>,
}

impl LaunchIds for CountingLaunchIds {
    fn draw(&self) -> LaunchId {
        self.drawn.set(self.drawn.get() + 1);
        let id = format!("00000000-0000-4000-8000-{:012}", self.drawn.get());
        LaunchId::new(&id).unwrap_or_else(|| panic!("{id} is a launch id"))
    }
}

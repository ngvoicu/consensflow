//! What the ported suites share: the tiered staff of `withTiers` with the
//! helpers it hands back, the quotas a harness says, and the patterns the
//! tests matched with.

use cf_base::time::iso;
use cf_engine::testing::Context;
use cf_harness::contract::Pane;
use cf_harness::records::{Level, Quota};
use cf_harness::seams::Time;
use cf_ledger::{NewTask, ParticipantView, ProjectView, TaskThread, TaskView};
use regex::Regex;

/// Asserts `text` matches `pattern` (`assert.match`).
#[track_caller]
pub fn assert_match(text: &str, pattern: &str) {
    let found = Regex::new(pattern).expect("the pattern").is_match(text);
    assert!(found, "{text:?} does not match /{pattern}/");
}

/// A task's state and its assignee, to be read as `[state, assignee]`.
pub fn placed(task: &TaskView) -> (&str, Option<&str>) {
    (task.state.as_str(), task.assignee.as_deref())
}

/// What a harness says when a quota is nearly spent: `{ state: 'low', usedPercent }`.
pub fn low(used_percent: u64) -> Quota {
    Quota::Usage {
        level: Level::Low,
        used_percent: Some(used_percent.into()),
        resets_at: None,
    }
}

/// What a harness says of a refusal for quota: `{ state: 'exhausted', at, resetsAt }`.
pub fn exhausted(at: Option<&str>, resets_at: Option<&str>) -> Quota {
    Quota::Exhausted {
        at: at.map(str::to_owned),
        resets_at: resets_at.map(str::to_owned),
    }
}

/// The time now (`clock.now().toISOString()`).
pub fn now(context: &Context) -> String {
    after(context, 0)
}

/// The time `ms` from now, as the ledger writes one.
pub fn after(context: &Context, ms: i64) -> String {
    iso(context.time.wall_ms() + ms)
}

/// The time `hours` from now (`soon`).
pub fn soon(context: &Context, hours: i64) -> String {
    after(context, hours * 3_600_000)
}

/// Whether the window opened as `pane` was closed.
pub fn closed(context: &Context, pane: &Pane) -> bool {
    context.host.killed().contains(pane)
}

/// T-1 to zeus, the only standard worker here, delivered and answered.
pub fn finished(context: &Context) -> Tiers<'_> {
    let tiers = Tiers::new(context, &["zeus"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working");
    context.adapter.answer("zeus", "Parser done");
    context.pass().unwrap();
    tiers
}

/// A tiered staff (`withTiers`): standard workers (zeus and diana, unless
/// told), a light worker, and two standard reviewers; with the helpers its
/// tests use.
pub struct Tiers<'a> {
    pub context: &'a Context,
    pub project: ProjectView,
}

impl<'a> Tiers<'a> {
    pub fn new(context: &'a Context, workers: &[&str]) -> Self {
        Self {
            context,
            project: context.with_tiers(workers),
        }
    }

    /// The id of `handle` in the project.
    pub fn id(&self, handle: &str) -> i64 {
        self.context.id(self.project.id, handle)
    }

    /// A task for the standard workers, as `open()` gives one.
    pub fn open(&self) -> TaskView {
        self.open_body("Write the parser")
    }

    /// A task for the standard workers, saying `body`.
    pub fn open_body(&self, body: &str) -> TaskView {
        self.open_for("worker", "standard", body)
    }

    /// A task for `tier` of `pool`.
    pub fn open_for(&self, pool: &str, tier: &str, body: &str) -> TaskView {
        self.context
            .create_task(
                self.project.id,
                NewTask {
                    from: "chief".to_owned(),
                    pool: Some(pool.to_owned()),
                    tier: Some(tier.to_owned()),
                    body: body.to_owned(),
                    ..NewTask::default()
                },
            )
            .task
    }

    /// Task `number` with its thread.
    pub fn task(&self, number: i64) -> TaskThread {
        self.context.task(self.project.id, number)
    }

    /// Task `number`'s assignee, which it must have.
    pub fn assignee(&self, number: i64) -> String {
        self.task(number)
            .task
            .assignee
            .unwrap_or_else(|| panic!("T-{number} has no assignee"))
    }

    /// A participant of the project, as the ledger has it now, if it is still there.
    pub fn participant(&self, handle: &str) -> Option<ParticipantView> {
        participant(self.context, self.project.id, handle)
    }

    /// Until when a member is out of quota: none when it is not.
    pub fn out_until(&self, handle: &str) -> Option<String> {
        self.participant(handle)
            .unwrap_or_else(|| panic!("no @{handle}"))
            .out_until
    }

    /// The notes `handle` was sent, oldest first.
    pub fn notes(&self, handle: &str) -> Vec<String> {
        notes(self.context, self.id(handle))
    }
}

/// A participant of `project` by handle, as the ledger has it now.
pub fn participant(context: &Context, project: i64, handle: &str) -> Option<ParticipantView> {
    context
        .ledger
        .borrow()
        .project(project)
        .unwrap()
        .unwrap()
        .participants
        .into_iter()
        .find(|participant| participant.handle == handle)
}

/// The notes a participant was sent, oldest first.
pub fn notes(context: &Context, participant: i64) -> Vec<String> {
    let mut sent: Vec<String> = context
        .ledger
        .borrow()
        .inbox(participant, 100)
        .unwrap()
        .into_iter()
        .filter(|message| message.kind == "note")
        .map(|message| message.body)
        .collect();
    sent.reverse();
    sent
}

/// The native session a participant's conversation is on.
pub fn native_of(context: &Context, participant: i64) -> Option<String> {
    context
        .ledger
        .borrow()
        .current_conversation(participant)
        .unwrap()
        .and_then(|conversation| conversation.native_session)
}

/// The launch the adapter was last asked for, its handle and the conversation it resumes.
pub fn last_launch(context: &Context) -> (String, Option<String>) {
    let launch = context.adapter.prepared().last().cloned().unwrap();
    (
        launch["participant"]["handle"].as_str().unwrap().to_owned(),
        launch["resume"].as_str().map(str::to_owned),
    )
}

/// The message the adapter's last launch opened with.
pub fn last_message(context: &Context) -> String {
    let launch = context.adapter.prepared().last().cloned().unwrap();
    launch["message"].as_str().unwrap().to_owned()
}

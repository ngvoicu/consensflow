//! The work a Muse turn leaves running, which Muse wakes its agent for as each piece
//! ends: what keeps a task held after its turn's `terminal` ([`Pending`]).
//!
//! Every piece of work a run starts is a task of the session's stream (`payload.kind:
//! task`, by `task_id`, of a `run_id`): its `started`, then `completed`, `failed`,
//! `cancelled` or `rejected`. A tool call's task answers within the turn, and its run's
//! `terminal` waits for it, and for Muse's own tasks (its reminder children are
//! cancelled before it: at every `terminal` of T-705's and T-718's live logs no task of
//! the run was open). So a run's task is background work where Muse says so, and where
//! it outlives its turn:
//! - a command run in the background: its task's `output` says `execution_state:
//!   background_running` (Muse 1.4.13's `bash`, which ends the call and runs on);
//! - a monitor: its `output` names its work `work.v2.monitor.…`, and its task ends when
//!   the watched stream ends (or its `timeout_ms` passes);
//! - any task of the run still open at the run's `terminal`, whatever it is.
//!
//! A task ends with its own end, however it came: stopped by the model (`work_stop`,
//! accepted, then the task `cancelled`, `terminated by bash_input`) or by the human from
//! Muse's `/tasks` (`cancelled`, `cancelled by runtime client`); the stop's acceptance
//! ends nothing. Either way Muse queues the end for the agent (`background_task_terminal`,
//! `terminal_kind: cancelled`), and that delivery holds as any other.
//!
//! **A piece of work is over when its agent has been told,** as Claude Code's job ends
//! when its notification is delivered: Muse queues what the work says
//! (`inbox_item_queued`, from `background_task_terminal`, `monitor_event`,
//! `subagent_result`, …) and drains each item into a run (`inbox_item_drained`, by the
//! queued record's `source_run_record_id`, into its `drain_target_run_stream`), which is
//! the turn the agent is woken in. So an item is pending from its queuing until it is
//! drained into a run that has started: in the moment between a command's end and the
//! start of the turn it wakes, its task is over and nothing has begun, and the task
//! that waits on it must not be called done there. The user's own message pasted
//! mid-turn (`user_steer`) and a goal the user sets with `/goal` (`goal_control`) are
//! queued and drained the same way: they are no work, and a turn is under way until a
//! run takes them in ([`Work::steering`]), as Claude Code's queued message keeps its turn
//! in flight. The deliveries are queued under the run that ended, not the one they wake
//! (live): who drains them, and when, is what counts.
//!
//! **A child agent** (`subagent_spawn`) is no task of the parent's that starts: its
//! life is told by `subagent.control` records, from `spawn_accepted` (by
//! `subagent_id`); its `result_ready` comes a record before its result is queued for
//! the agent (`subagent_result`, by the same id), and it is pending until that is taken
//! in as above. No status says it completed (live: a child its parent did not wait on);
//! a status that says it ended (`CHILD_ENDED`) ends it too, provisionally.
//! It is counted among the agents, where a turn's own children (its reminders) never
//! are. Where Muse differs from Codex: Codex keeps a turn in flight while a subagent it
//! started runs; Muse ends the turn first, as Claude Code does, and the agent pending is
//! said of a settled turn.
//!
//! **What a process leaves is retired, not pending,** when its segment of the log ends
//! (`session.end`, which a resume writes for a dead process too, `crash_inferred`, or a
//! later `session.started`): no Muse that runs is going to tell its agent of it. A turn
//! finished is never called working for good so, as Claude Code's jobs of a process that
//! is gone are retired at a relaunch. And where Muse then ends the task itself (live:
//! the resume cancels it, `resume_reconcile:orphaned_by_process_loss`, though the session
//! starts with `background_tasks: keep`), word came that it ended, and it is retired no
//! more: the engine says of a retired job that no word of its end came.
//!
//! **A goal** (`/goal`) is bound to the run that takes its `goal_control` in
//! (`goal_run_binding`, by `goal_id`), and is that run's work: the run is in flight
//! while it works on it. It is closed when a tool's result says its state is no longer
//! `active` (`update_goal`'s `{"goal": {goal_id, status: "complete"}}`, live). A goal
//! still open when its run ends is counted among the agents, as Muse continues an open
//! goal in a run of its own (provisional: the live goal closed within its run; its
//! continuation is in Muse's own `goals.db`), and one the user pauses or clears
//! (`goal_control` `pause`, `clear`) runs no more (provisional). A goal outlives its
//! process's end: Muse keeps it in its store and recovers it on a resume.
//!
//! Seen live (Muse 1.4.13): a background command, a monitor and a child, each through to
//! its end and drained into the run that started it, its agent waiting on it (T-705's
//! contributor run); a command that outlived its turn and the run it woke (T-789's live
//! case, `tests/fixtures/muse/live/background-outlives-turn.jsonl`); and T-801's shapes
//! (`live/monitor-outlives-turn.jsonl`, `child-not-waited.jsonl`,
//! `stopped-by-the-model.jsonl`, `stopped-by-the-human.jsonl`, `killed-and-resumed.jsonl`,
//! `goal.jsonl`). What stays provisional is listed in `provisional`.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use serde_json::Value;

use super::field;
use crate::shared::record::reading::Pending;

/// The ends of a task.
const ENDED: [&str; 4] = ["completed", "failed", "cancelled", "rejected"];

/// The kind a child's status says it ended in: provisional (the binary's strings; no
/// live child ended so), where `result_ready` is the live end.
const CHILD_ENDED: [&str; 5] = ["completed", "failed", "cancelled", "closed", "errored"];

/// What a queued item is to the task that waits.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Family {
    /// The user's own input: a message pasted mid-turn (`user_steer`), or a goal they
    /// set, changed, paused or cleared (`goal_control`). No work: a turn it starts.
    Input,
    /// A child's result.
    Agent,
    /// What a command, a monitor or Muse itself delivers.
    Job,
}

/// An item queued for the agent: what it is, and the work it comes from (the task of
/// a command or a monitor, the child of a result), which is counted once however many
/// of its items wait.
#[derive(Clone)]
struct Item {
    family: Family,
    owner: Option<Arc<str>>,
}

/// A run's task that has not ended.
struct Task {
    run: Arc<str>,
    background: bool,
}

/// What the log said of the work its turns started.
#[derive(Default)]
pub(super) struct Work {
    tasks: HashMap<Arc<str>, Task>,
    children: HashSet<Arc<str>>,
    /// Items queued and not drained, by the id their drain names.
    queued: HashMap<Arc<str>, Item>,
    /// Items drained into a run that has not started yet, by that run.
    draining: HashMap<Arc<str>, Vec<Item>>,
    started: HashSet<Arc<str>>,
    /// The goals bound to a run and not closed, paused or cleared.
    goals: HashSet<Arc<str>>,
    /// The work a process's segment left, by its id, no word of whose end came since.
    retired: HashSet<Arc<str>>,
    /// The deliveries a process's segment left that name no work.
    retired_unnamed: usize,
}

impl Work {
    /// A task's event (`payload.kind: task`).
    pub(super) fn task(&mut self, payload: Option<&Value>) {
        let event = field(payload, "event");
        let (Some(task), Some(run)) = (
            text(field(payload, "task_id")),
            text(field(payload, "run_id")),
        ) else {
            return;
        };
        match field(event, "kind").and_then(Value::as_str) {
            Some("started") => {
                self.tasks.insert(
                    task,
                    Task {
                        run,
                        background: false,
                    },
                );
            }
            Some("output") => {
                let said = field(event, "chunk").and_then(Value::as_str).map(chunk);
                if said.is_some_and(|said| said.backgrounds()) {
                    if let Some(open) = self.tasks.get_mut(&task) {
                        open.background = true;
                    }
                }
            }
            Some(kind) if ENDED.contains(&kind) => {
                self.tasks.remove(&task);
                // A resume's word that a dead process's task ended (live:
                // `resume_reconcile:orphaned_by_process_loss`): it is retired no more.
                self.retired.remove(&task);
            }
            _ => {}
        }
    }

    /// The binary's word that a task runs in the background (`task_backgrounded`), which
    /// no live log has written: provisional.
    pub(super) fn backgrounded(&mut self, task: Option<&Value>) {
        if let Some(open) = text(task).and_then(|task| self.tasks.get_mut(&task)) {
            open.background = true;
        }
    }

    /// A run started: what was drained into it is its agent's now.
    pub(super) fn run_started(&mut self, run: &Arc<str>) {
        self.started.insert(Arc::clone(run));
        self.draining.remove(run);
    }

    /// A run ended: what it left open is background work.
    pub(super) fn run_ended(&mut self, run: &str) {
        for task in self.tasks.values_mut().filter(|task| &*task.run == run) {
            task.background = true;
        }
    }

    /// An item queued for the session's agent, whose record's payload is `payload`.
    pub(super) fn queued(&mut self, payload: Option<&Value>, event: Option<&Value>) {
        let Some(id) = text(field(payload, "source_run_record_id")) else {
            return;
        };
        let source = field(event, "source");
        let (family, owner) = match field(source, "source").and_then(Value::as_str) {
            Some("user_steer") => (Family::Input, None),
            Some("goal_control") => {
                // A goal the user paused or cleared runs no more (provisional: the live
                // goal was set, then closed by its agent).
                let action = field(source, "action").and_then(Value::as_str);
                if matches!(action, Some("pause" | "clear")) {
                    if let Some(goal) = text(field(source, "goal_id")) {
                        self.goals.remove(&goal);
                    }
                }
                (Family::Input, None)
            }
            Some("subagent_result") => {
                let owner = text(field(source, "subagent_id"));
                // The child's result: it is pending as this item now, until taken in.
                if let Some(owner) = &owner {
                    self.children.remove(owner);
                }
                (Family::Agent, owner)
            }
            _ => (Family::Job, text(field(source, "task_id"))),
        };
        self.queued.insert(id, Item { family, owner });
    }

    /// An item drained into a run.
    pub(super) fn drained(&mut self, event: Option<&Value>) {
        let Some(item) = text(field(event, "item_id")).and_then(|id| self.queued.remove(&id))
        else {
            return;
        };
        let target = text(field(field(event, "drain_target_run_stream"), "id"));
        match target {
            Some(run) if !self.started.contains(&run) => {
                self.draining.entry(run).or_default().push(item);
            }
            _ => {}
        }
    }

    /// A child agent's control record.
    pub(super) fn child(&mut self, record: Option<&Value>) {
        let Some(child) = text(field(record, "subagent_id")) else {
            return;
        };
        match field(record, "kind").and_then(Value::as_str) {
            Some("spawn_accepted") => {
                self.children.insert(child);
            }
            Some("status_updated") => {
                let status = field(record, "control_status").and_then(Value::as_str);
                if status.is_some_and(|status| CHILD_ENDED.contains(&status)) {
                    self.children.remove(&child);
                    self.retired.remove(&child);
                }
            }
            _ => {}
        }
    }

    /// A goal bound to its run (`goal_run_binding`): it is work while it is open.
    pub(super) fn goal_bound(&mut self, record: Option<&Value>) {
        if let Some(goal) = text(field(record, "goal_id")) {
            self.goals.insert(goal);
        }
    }

    /// A tool's result that says a goal's state (`update_goal`'s `{"goal": {goal_id,
    /// status}}`, live): one that is not `active` closes it.
    pub(super) fn goal_said(&mut self, said: &str) {
        let Ok(said) = serde_json::from_str::<Value>(said) else {
            return;
        };
        let goal = said.get("goal");
        let status = field(goal, "status").and_then(Value::as_str);
        if let (Some(goal), Some(status)) = (text(field(goal, "goal_id")), status) {
            if status != "active" {
                self.goals.remove(&goal);
            }
        }
    }

    /// The process's segment of the log ended: what it left is retired. A goal is not:
    /// Muse keeps it in its own store and continues it in the process that resumes.
    pub(super) fn segment_ended(&mut self) {
        let running = self
            .tasks
            .iter()
            .filter(|(_, task)| task.background)
            .map(|(id, _)| Arc::clone(id))
            .chain(self.children.iter().cloned());
        let mut left: HashSet<Arc<str>> = running.collect();
        let mut unnamed = 0;
        for item in self.items().filter(|item| item.family != Family::Input) {
            match &item.owner {
                Some(owner) => {
                    left.insert(Arc::clone(owner));
                }
                None => unnamed += 1,
            }
        }
        self.retired_unnamed += unnamed;
        self.retired.extend(left);
        self.tasks.clear();
        self.children.clear();
        self.queued.clear();
        self.draining.clear();
    }

    /// Whether the user's input (a message pasted mid-turn, a goal set) waits to be taken
    /// in by a run.
    pub(super) fn steering(&self) -> bool {
        self.items().any(|item| item.family == Family::Input)
    }

    /// What runs, and what its agent has not yet been told of: each piece of work once.
    pub(super) fn pending(&self) -> Pending {
        let tasks = self
            .tasks
            .iter()
            .filter(|(_, task)| task.background)
            .map(|(id, _)| Arc::clone(id));
        Pending {
            agents: self.count(Family::Agent, self.children.iter().cloned()) + self.goals.len(),
            jobs: self.count(Family::Job, tasks),
            retired: self.retired.len() + self.retired_unnamed,
            noticed: 0,
        }
    }

    /// How many pieces of work of `family` wait: `running`, and those whose items are
    /// not taken in, each once, and each item of no work's.
    fn count(&self, family: Family, running: impl Iterator<Item = Arc<str>>) -> usize {
        let mut work: HashSet<Arc<str>> = running.collect();
        let mut ownerless = 0;
        for item in self.items().filter(|item| item.family == family) {
            match &item.owner {
                Some(owner) => {
                    work.insert(Arc::clone(owner));
                }
                None => ownerless += 1,
            }
        }
        work.len() + ownerless
    }

    /// Every item not yet taken in by a run that started.
    fn items(&self) -> impl Iterator<Item = &Item> + '_ {
        self.queued.values().chain(self.draining.values().flatten())
    }
}

/// What a task's output says of its work.
struct Chunk {
    state: Option<String>,
    work: Option<String>,
}

impl Chunk {
    /// Whether it says the work runs on in the background: a command's, or a monitor's.
    fn backgrounds(&self) -> bool {
        self.state.as_deref() == Some("background_running")
            || self
                .work
                .as_deref()
                .is_some_and(|work| work.starts_with("work.v2.monitor."))
    }
}

/// A task's output, a JSON object as text: what it says of its work, nothing of what
/// is no object.
fn chunk(text: &str) -> Chunk {
    let said: Option<Value> = serde_json::from_str(text).ok();
    let get = |name: &str| {
        said.as_ref()
            .and_then(|said| said.get(name))
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    Chunk {
        state: get("execution_state"),
        work: get("work_id"),
    }
}

/// A text value, not empty.
fn text(value: Option<&Value>) -> Option<Arc<str>> {
    value
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .map(Arc::from)
}

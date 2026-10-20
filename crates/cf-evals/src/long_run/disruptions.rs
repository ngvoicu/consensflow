//! The disruptions of a long run, and when each is played: one at a time, in a
//! fixed order, each due `--every-min` minutes after the one before it was played,
//! and played at the first look after it is due whose moment makes its shape real
//! (a worker in the middle of a tool call, a chief at rest…). A disruption whose
//! strict moment does not come within half the interval after it is due takes a
//! looser one, and the timeline says which it was. One that cannot be played by this
//! run's staff (no Devin, a chief that is not Claude Code for `/compact`) says so
//! from the start; one whose moment never came is not played, and the run says so:
//! it could not be run as asked.
//!
//! A disruption is played when what it asks of ConsensFlow is seen done (the window
//! killed and its task paused, the new chief's window open, a compaction boundary in
//! the chief's record, the project open again); one whose effect is not seen failed,
//! which is a break of its own, with its evidence (`play`).
//!
//! The order is the field's: a pause and its resume; a window killed mid-task (B2);
//! `/compact` typed into the idle chief, and the app quit and started again after it
//! while a worker is mid-tool and the chief has had no turn since, so that the Resume
//! reads a record that ends in an idle compaction (A5; a loose moment, half an
//! interval late, takes any worker at work); the chief switched while the staff work
//! and a job runs; the app quit and started again with a Devin worker mid-tool (B1);
//! a project suspended and resumed. The compaction comes before the switch, and is
//! typed only into a chief that is Claude Code at its moment: the chief it is typed
//! into is the one whose record says it compacted. (A chief that begins on another
//! harness never compacts, though a switch would make it Claude Code: the switch
//! comes after.)

use std::time::Duration;

/// A disruption a long run plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Disruption {
    /// A task paused and resumed.
    Pause,
    /// One worker's window killed mid-task (B2's shape).
    Kill,
    /// `/compact` typed into the idle chief (A5's shape, with the restart after it).
    Compact,
    /// The app quit and started again while a worker is mid-tool.
    Restart,
    /// The chief switched while staff work and a background job runs.
    Switch,
    /// The app quit and started again while a Devin worker is mid-tool (B1's shape).
    DevinCut,
    /// The project suspended and resumed.
    Suspend,
}

impl Disruption {
    /// Every disruption, in the order a run plays them.
    pub const ALL: [Self; 7] = [
        Self::Pause,
        Self::Kill,
        Self::Compact,
        Self::Restart,
        Self::Switch,
        Self::DevinCut,
        Self::Suspend,
    ];

    /// Its flag on the command line, and its name in a report.
    pub const fn flag(self) -> &'static str {
        match self {
            Self::Pause => "pause",
            Self::Kill => "kill",
            Self::Compact => "compact",
            Self::Restart => "restart",
            Self::Switch => "switch",
            Self::DevinCut => "devin-cut",
            Self::Suspend => "suspend",
        }
    }

    /// What it is, in a line.
    pub const fn about(self) -> &'static str {
        match self {
            Self::Pause => "a task paused and resumed",
            Self::Kill => "a worker's window killed mid-task (B2)",
            Self::Compact => "/compact typed into the idle chief (A5, with the restart)",
            Self::Restart => "the app quit and started again while a worker is mid-tool",
            Self::Switch => "the chief switched while the staff work and a job runs",
            Self::DevinCut => {
                "the app quit and started again while a Devin worker is mid-tool (B1)"
            }
            Self::Suspend => "the project suspended and resumed",
        }
    }
}

/// A worker's window as a look found it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worker {
    pub handle: String,
    /// The harness's command name (`claude`, `devin`…).
    pub harness: String,
    /// The task at work on its lane, by number.
    pub task: Option<i64>,
    /// Whether its window is in a turn.
    pub working: bool,
    /// Whether its turn has something running (`pane.background`): a tool call.
    pub tool: bool,
}

/// What a look found that a disruption's moment is told by.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Moment {
    /// How long the run has gone.
    pub elapsed: Duration,
    /// The chief's harness now (it may have been switched), and whether its window
    /// is at rest.
    pub chief: String,
    pub chief_idle: bool,
    /// Whether the chief's own record ends in a compaction, no turn since: what a
    /// Resume after the app's restart must read for A5's shape to be real.
    pub chief_compacted: bool,
    pub workers: Vec<Worker>,
}

/// Where a disruption is aimed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// The project, or the chief: nothing more to name.
    Project,
    /// A task, by number.
    Task(i64),
    /// A worker's window, and the task it works on.
    Window { handle: String, task: Option<i64> },
}

/// What came of a disruption.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    /// Not yet played.
    Waiting,
    /// Played at that point of the run, its effect seen: what it did.
    Played { at: Duration, said: String },
    /// Played at that point of the run, and its effect not seen: what it did and
    /// what did not come of it (a break, said with its evidence).
    Failed { at: Duration, said: String },
    /// This run cannot play it, and why.
    Impossible(String),
    /// Its moment never came, and why.
    NotPlayed(String),
}

/// A disruption of the run, when it is due, and what came of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Planned {
    pub disruption: Disruption,
    /// When it is due: the interval after the one before it was played (the first,
    /// after the run began).
    pub due: Duration,
    pub state: State,
    /// The field's shape it was played without, where its moment lacked it: the run
    /// did not play what it is for, and says so.
    pub lacked: Option<String>,
}

/// A disruption to play now: where, at which moment (`strict`, or `loose` once half
/// an interval has gone by), and the field's shape the moment lacks, if it lacks one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    pub disruption: Disruption,
    pub target: Target,
    pub how: &'static str,
    pub lacks: Option<&'static str>,
}

/// What a restart after a played compaction lacks when the chief has had a turn
/// since: A5's shape, a Resume that reads a record ending in an idle compaction.
pub const NO_IDLE_COMPACTION: &str =
    "A5's shape: the chief had a turn after the compaction, so the Resume read no idle compaction";

/// The harness whose `/compact` the watch can see done: Claude Code's record says it
/// (`compact_boundary`), and A5 is its shape.
const COMPACTS: &str = "claude";

/// The disruptions of a run, in the order they are played.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Schedule {
    pub planned: Vec<Planned>,
    every: Duration,
}

impl Schedule {
    /// The `chosen` disruptions, each due `every` after the one before, for a run
    /// whose chief is on `chief` and whose staff has `staff` (command names).
    pub fn new(chosen: &[Disruption], every: Duration, chief: &str, staff: &[String]) -> Self {
        let planned = Disruption::ALL
            .into_iter()
            .filter(|disruption| chosen.contains(disruption))
            .enumerate()
            .map(|(at, disruption)| Planned {
                disruption,
                due: every * u32::try_from(at + 1).unwrap_or(u32::MAX),
                state: match impossible(disruption, chief, staff) {
                    Some(why) => State::Impossible(why),
                    None => State::Waiting,
                },
                lacked: None,
            })
            .collect();
        Self { planned, every }
    }

    /// The disruption to play now, and where, if one is due and its moment has
    /// come: the first still waiting, never before the one before it was played. The
    /// restart after a played compaction waits for a chief whose record still ends in
    /// it; the loose moment it takes when none comes lacks A5's shape, and says so.
    pub fn next(&self, moment: &Moment) -> Option<Choice> {
        let waiting = self
            .planned
            .iter()
            .find(|planned| planned.state == State::Waiting)?;
        if moment.elapsed < waiting.due {
            return None;
        }
        let compacted = self.planned.iter().any(|planned| {
            planned.disruption == Disruption::Compact
                && matches!(planned.state, State::Played { .. })
        });
        let choice = |target: Target, how: &'static str| {
            let lacks =
                (waiting.disruption == Disruption::Restart && compacted && !moment.chief_compacted)
                    .then_some(NO_IDLE_COMPACTION);
            Choice {
                disruption: waiting.disruption,
                target,
                how,
                lacks,
            }
        };
        if let Some(target) = aim(waiting.disruption, moment, true, compacted) {
            return Some(choice(target, "strict"));
        }
        let late = moment.elapsed >= waiting.due + self.every / 2;
        late.then(|| aim(waiting.disruption, moment, false, compacted))
            .flatten()
            .map(|target| choice(target, "loose"))
    }

    /// `disruption` was played without the field's shape it is for: `what`.
    pub fn lacked(&mut self, disruption: Disruption, what: &str) {
        if let Some(planned) = self
            .planned
            .iter_mut()
            .find(|planned| planned.disruption == disruption)
        {
            planned.lacked = Some(what.to_owned());
        }
    }

    /// The disruptions played without the field's shape they are for.
    pub fn without_their_shape(&self) -> Vec<&Planned> {
        self.planned
            .iter()
            .filter(|planned| planned.lacked.is_some())
            .collect()
    }

    /// `disruption` was played `at`, its effect seen, and did what `said` says.
    pub fn played(&mut self, disruption: Disruption, at: Duration, said: String) {
        self.resolve(disruption, at, State::Played { at, said });
    }

    /// `disruption` was played `at`, and its effect was not seen.
    pub fn failed(&mut self, disruption: Disruption, at: Duration, said: String) {
        self.resolve(disruption, at, State::Failed { at, said });
    }

    /// `disruption` came to `state` at `at`: the next waiting is due the interval after.
    fn resolve(&mut self, disruption: Disruption, at: Duration, state: State) {
        let Some(place) = self
            .planned
            .iter()
            .position(|planned| planned.disruption == disruption)
        else {
            return;
        };
        self.planned[place].state = state;
        let every = self.every;
        if let Some(next) = self.planned[place + 1..]
            .iter_mut()
            .find(|planned| planned.state == State::Waiting)
        {
            next.due = at + every;
        }
    }

    /// The run is over: every disruption still waiting was not played, for `why`.
    pub fn close(&mut self, why: &str) {
        for planned in &mut self.planned {
            if planned.state == State::Waiting {
                planned.state = State::NotPlayed(why.to_owned());
            }
        }
    }

    /// Whether every disruption that could be played was, or none is left to play.
    pub fn done(&self) -> bool {
        self.planned
            .iter()
            .all(|planned| planned.state != State::Waiting)
    }

    /// The disruptions whose moment never came.
    pub fn not_played(&self) -> Vec<&Planned> {
        self.planned
            .iter()
            .filter(|planned| matches!(planned.state, State::NotPlayed(_)))
            .collect()
    }
}

/// Why this run cannot play `disruption`, if it cannot.
fn impossible(disruption: Disruption, chief: &str, staff: &[String]) -> Option<String> {
    match disruption {
        Disruption::DevinCut if !staff.iter().any(|name| name == "devin") => {
            Some("no Devin on the staff".to_owned())
        }
        Disruption::Compact if chief != COMPACTS => Some(format!(
            "the chief's harness, {chief}, is not Claude Code, whose record says a compaction"
        )),
        Disruption::Switch if !staff.iter().any(|name| name != chief) => Some(format!(
            "no harness on the staff other than the chief's, {chief}"
        )),
        _ => None,
    }
}

/// Where `disruption` is aimed at `moment`, if the moment fits: `strict`, the
/// shape it is for (a tool call at work, a job running, and for the restart after a
/// compaction that was played, `compacted`, a chief whose record still ends in it);
/// else any turn at work.
fn aim(disruption: Disruption, moment: &Moment, strict: bool, compacted: bool) -> Option<Target> {
    let at_work = |devin: bool| {
        moment
            .workers
            .iter()
            .filter(move |worker| worker.working && worker.task.is_some())
            .filter(move |worker| !devin || worker.harness == "devin")
            .find(|worker| !strict || worker.tool)
    };
    match disruption {
        Disruption::Pause => at_work(false).and_then(|worker| worker.task.map(Target::Task)),
        Disruption::Kill => at_work(false).map(|worker| Target::Window {
            handle: worker.handle.clone(),
            task: worker.task,
        }),
        Disruption::Compact => {
            (moment.chief == COMPACTS && moment.chief_idle).then_some(Target::Project)
        }
        Disruption::Restart => at_work(false)
            .filter(|_| !strict || !compacted || moment.chief_compacted)
            .map(|_| Target::Project),
        Disruption::Switch => {
            let job = moment.workers.iter().any(|worker| worker.tool);
            let working = moment.workers.iter().any(|worker| worker.working);
            (working && (job || !strict)).then_some(Target::Project)
        }
        Disruption::DevinCut => at_work(true).map(|worker| Target::Window {
            handle: worker.handle.clone(),
            task: worker.task,
        }),
        Disruption::Suspend => moment
            .workers
            .iter()
            .any(|worker| worker.working && worker.task.is_some())
            .then_some(Target::Project),
    }
}

#[cfg(test)]
mod tests;

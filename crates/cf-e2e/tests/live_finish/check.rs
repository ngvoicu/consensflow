//! One worker, run: it opens as the member of a project whose chief is the stand-in,
//! is given its task, and the check, as the chief, looks at the board and the pane
//! host once a second until the task ends ([`observe`]); what it saw is held to the
//! brief ([`verdict`]) and said, with what the pane host listed as the turn's work at
//! the turn's end whatever the verdict.

use std::path::Path;
use std::time::{Duration, Instant};

use cf_e2e::live::agents;
use cf_e2e::live::members::{
    self, delivered_result, find, is_over, Account, Members, Plan, STAND_IN_WORKER,
};
use cf_e2e::live::tree::{self, Read};
use cf_e2e::live::{self, cut, last_lines, text, Harness, Log, Who};
use cf_e2e::{Error, Result};
use serde_json::Value;

use crate::task::Task;
use crate::verdict::{at_the_close, at_the_turns_end, judge, summary, Seen};

/// How long the worker has to do its task and have it end on the board, its result to
/// be delivered to the chief once it has, and its window to close once the task is over.
pub const WORK: Duration = Duration::from_secs(300);
pub const DELIVERS: Duration = Duration::from_secs(60);
const CLOSES: Duration = Duration::from_secs(30);

/// How often the board and the pane host are looked at while the worker is waited for.
const LOOK: Duration = Duration::from_secs(1);

/// How often the run says how the worker is getting on when nothing has changed.
const HEARTBEAT: Duration = Duration::from_secs(30);

/// The one a word on the command line names as a worker: `fake` (the rig's stand-in
/// agent, which has no model and spends no quota), or a harness.
pub fn named(word: &str) -> Result<Who> {
    Who::named(word, "worker")
}

/// The id of the agent that is `who` in the project: the worker.
fn agent(who: Who) -> String {
    match who {
        Who::StandIn => STAND_IN_WORKER.to_owned(),
        Who::Harness(harness) => agents::id(harness),
    }
}

/// What `who` is given to do.
pub fn task(who: Who) -> Result<Task> {
    match who {
        Who::StandIn => Ok(Task::stand_in()),
        Who::Harness(_) => Task::real(),
    }
}

/// What a worker's run came to: what it saw, and the line of it that the table says.
pub struct Finished {
    pub case: Account,
    pub summary: String,
}

impl Finished {
    /// A run that came to nothing: it could not be made, which is why.
    pub fn failed(failure: Error) -> Self {
        let mut case = Account::new();
        case.fail(&failure);
        Self {
            case,
            summary: failure.to_string(),
        }
    }
}

/// The project of `who`'s run: a chief that is the stand-in and the worker alone, in a
/// folder of its own.
pub fn plan(who: Who) -> Plan {
    let worker = match who {
        Who::StandIn => members::stand_in_worker(),
        Who::Harness(harness) => members::member(harness),
    };
    Plan::new(&format!("finish-{}", who.name()), members::stand_in_chief()).with(worker)
}

/// Runs the worker `who` on the project `plan` opens, with the task `task`: what it came
/// to, which holds when the task was done and its result reached the chief with nothing
/// held back.
pub fn run(who: Who, plan: &Plan, task: &Task, stand_in: &Path, log: &Log) -> Result<Finished> {
    let log = log.scoped(who.name());
    let mut members = Members::open(plan, stand_in, &log)?;
    let finished = observe(&mut members, &agent(who), task, &log);
    let closed = members.close();
    let mut finished = finished?;
    finished.case.closed(closed);
    Ok(finished)
}

/// What the run says of how the worker is getting on, when it changes and now and then
/// when it does not.
#[derive(Default)]
struct Progress {
    said: Option<(String, Instant)>,
}

impl Progress {
    fn say(&mut self, log: &Log, line: String) {
        let due = self
            .said
            .as_ref()
            .is_none_or(|(last, at)| *last != line || at.elapsed() >= HEARTBEAT);
        if due {
            log.say(&line);
            self.said = Some((line, Instant::now()));
        }
    }
}

/// How the worker is getting on at a look: its task's state, what its window is doing
/// and why where the board says why, and what the pane host lists of its tree.
fn progress_line(number: i64, thread: &Value, lane: &Value, listing: Option<&Read>) -> String {
    let activity = &lane["activity"];
    let why = activity["reason"]
        .as_str()
        .map_or_else(String::new, |reason| format!(" ({reason})"));
    let host = match listing {
        Some(Ok(tree)) => format!("; the host lists {}", tree.line()),
        Some(Err(said)) => format!("; the host: {said}"),
        None => String::new(),
    };
    format!(
        "T-{number} is {}; the worker's window is {}{why}{host}",
        text(&thread["state"]),
        activity["state"].as_str().unwrap_or("not there")
    )
}

/// The worker given its task, and the board and the pane host looked at once a second
/// until the task ends; then the result waited for at the chief, and what was seen
/// held to the brief.
fn observe(members: &mut Members, agent: &str, task: &Task, log: &Log) -> Result<Finished> {
    let mut case = Account::new();
    if let Some(trust) = members.trust() {
        case.say(format!("trust: {trust}"));
    }
    members.add(agent, &[])?;
    let number = members.give(agent, &task.brief)?;
    log.say(format!("T-{number} is given: {}", cut(&task.brief, 140)));

    let mut seen = Seen::default();
    let mut progress = Progress::default();
    let ended = find(log, "the worker's task to end", WORK, LOOK, || {
        let thread = members.task(number)?;
        let lane = members.lane(agent)?;
        let listing = (!lane["pane"].is_null()).then(|| tree::read(members.rig(), &lane["pane"]));
        progress.say(log, progress_line(number, &thread, &lane, listing.as_ref()));
        seen.look(log.elapsed(), &text(&thread["state"]), &lane, listing);
        Ok(is_over(&thread).then_some(()))
    })?
    .is_some();
    if seen.state == "done" {
        find(
            log,
            "the result to be delivered to the chief",
            DELIVERS,
            LOOK,
            || {
                seen.result = delivered_result(&members.inbox(Some(members.chief()))?);
                Ok(seen.result.is_some().then_some(()))
            },
        )?;
    }
    // A task that is over has its window closed, as the daemon closes a worker's when
    // its task ends: the host says what it left running as it does.
    if let (true, Some(pane)) = (ended, seen.pane.clone()) {
        find(log, "the worker's window to close", CLOSES, LOOK, || {
            seen.exit = tree::left_at_exit(members.rig(), &pane);
            Ok(seen.exit.is_some().then_some(()))
        })?;
    }

    let verdict = judge(&seen, &task.output);
    case.say(format!(
        "T-{number} is {} after {} s; what the board said of the worker's window: {}",
        seen.state,
        live::secs(seen.took()),
        seen.reasons.line()
    ));
    case.say(at_the_turns_end(&seen));
    case.say(at_the_close(&seen));
    match &verdict {
        Ok(()) => case.check(
            "the result reached the chief, nothing held back",
            true,
            summary(&seen, &verdict),
        ),
        Err(problems) => {
            for problem in problems {
                case.fail(problem);
            }
            for line in failure(members, agent, number) {
                case.say(line);
            }
        }
    }
    Ok(Finished {
        case,
        summary: summary(&seen, &verdict),
    })
}

/// `body` under `title`, a line to each of its lines, indented.
fn block(title: &str, body: &str) -> String {
    let lines: Vec<String> = body.lines().map(|line| format!("    {line}")).collect();
    format!("     {title}:\n{}", lines.join("\n"))
}

/// What the product showed of a task that did not end as it should have: the worker's
/// lane of the board, what its window recorded and last drew, and what the daemon
/// warned of.
fn failure(members: &Members, agent: &str, number: i64) -> Vec<String> {
    let rig = members.rig();
    let screen = members
        .window_of(agent)
        .ok()
        .flatten()
        .map(|window| last_lines(&rig.screen(window.id())))
        .unwrap_or_default();
    vec![
        block(
            "the worker's lane of the board",
            &live::pretty(members.lane(agent)),
        ),
        block(
            "what the worker's window recorded",
            &live::transcript(rig, members.project_id(), number),
        ),
        block("what the worker's window last drew", &screen),
        block("the daemon's log", &live::warnings(&rig.log()).join("\n")),
    ]
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_worker_is_named_by_a_harness_or_by_fake_and_nothing_else() {
        assert_eq!(named("fake").unwrap(), Who::StandIn);
        assert_eq!(named("devin").unwrap(), Who::Harness(Harness::Devin));
        let failed = named("kimi").unwrap_err().to_string();
        assert_eq!(
            failed,
            "no worker is called kimi: fake, claude, opencode, pi, devin, codex"
        );
    }

    #[test]
    fn the_worker_of_a_harness_is_its_agent_and_the_stand_ins_is_the_worker() {
        assert_eq!(agent(Who::Harness(Harness::Devin)), "live-devin");
        assert_eq!(agent(Who::StandIn), "worker");
        assert_eq!(task(Who::StandIn).unwrap(), Task::stand_in());
    }

    #[test]
    fn the_project_of_a_worker_has_the_stand_in_for_its_chief_and_the_worker_alone() {
        let plan = plan(Who::Harness(Harness::Pi));
        assert_eq!(plan.folder, "finish-pi");
        assert_eq!(plan.chief, "chief");
        let ids: Vec<&str> = plan
            .roster
            .iter()
            .filter_map(|row| row["id"].as_str())
            .collect();
        assert_eq!(ids, ["chief", "live-pi"]);
        let fake = super::plan(Who::StandIn);
        assert_eq!(fake.folder, "finish-fake");
        assert_eq!(fake.roster[1]["id"], "worker");
    }

    #[test]
    fn how_the_worker_is_getting_on_is_its_task_its_window_why_and_what_the_host_lists() {
        let thread = json!({ "state": "working" });
        let lane = json!({ "activity": {
            "state": "working",
            "reason": "background work running (1 process): hostname",
        } });
        let listed: Read = Ok(tree::Tree::of(&json!({
            "ok": true,
            "total": 1,
            "processes": [{ "pid": 41, "command": "hostname" }],
        })));
        assert_eq!(
            progress_line(1, &thread, &lane, Some(&listed)),
            "T-1 is working; the worker's window is working (background work running (1 process): hostname); the host lists 41 hostname (1)"
        );
        // A window the board says nothing of, and a host that could not be asked.
        assert_eq!(
            progress_line(2, &json!({ "state": "queued" }), &Value::Null, None),
            "T-2 is queued; the worker's window is not there"
        );
        let unread: Read = Err("(the host said: no such pane)".to_owned());
        assert_eq!(
            progress_line(
                2,
                &thread,
                &json!({ "activity": { "state": "idle" } }),
                Some(&unread)
            ),
            "T-2 is working; the worker's window is idle; the host: (the host said: no such pane)"
        );
    }

    #[test]
    fn a_line_is_said_when_it_changes_and_again_only_after_a_while() {
        let log = Log::new("live finish test");
        let mut progress = Progress::default();
        progress.say(&log, "a".to_owned());
        let first = progress.said.clone().unwrap().1;
        progress.say(&log, "a".to_owned());
        assert_eq!(progress.said.as_ref().unwrap().1, first, "said once");
        progress.say(&log, "b".to_owned());
        assert_eq!(progress.said.as_ref().unwrap().0, "b");
        assert!(progress.said.as_ref().unwrap().1 >= first);
    }

    #[test]
    fn a_run_that_could_not_be_made_is_a_case_that_did_not_hold_and_says_why() {
        let finished = Finished::failed(Error::Daemon("claude is not installed".to_owned()));
        assert!(!finished.case.held());
        assert_eq!(finished.summary, "claude is not installed");
        assert_eq!(finished.case.lines(), ["FAIL claude is not installed"]);
    }
}

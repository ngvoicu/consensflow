//! The baseline of a worker: the human gives the chief one task for it, which the
//! chief must run itself (`cf task add` for the worker's tier); the daemon picks
//! the worker (the free one of that tier with the fewest tasks so far, the earliest
//! joined first), opens its window with the task, the worker answers in
//! full-permission mode, the core records the answer as the task's result and
//! delivers it into the chief's window, and the worker's window closes with its
//! task (one task per session). Then the continuation: the chief sends a follow-up
//! to the window that did the task, which comes back on its own conversation and
//! answers again.

use std::time::{Duration, Instant};

use cf_e2e::live::members::{find, tier_flag};
use cf_e2e::live::Harness;
use cf_e2e::Result;
use serde_json::{json, Value};

use crate::bench::{first_chars, seconds_since, took, Bench};

/// How long a worker's window has to close once its task is done.
const CLOSES: Duration = Duration::from_secs(60);

/// A worker of the bench: its harness, its agent's id in the roster, and the tier
/// the daemon gave it (a task that names the tier lands on it).
#[derive(Debug, Clone)]
pub struct Worker {
    pub harness: Harness,
    pub tier: String,
}

impl Worker {
    /// What it is called: its harness's name.
    pub const fn name(&self) -> &'static str {
        self.harness.name()
    }

    /// Its agent's id in the roster.
    pub fn agent(&self) -> String {
        agent_id(self.harness)
    }
}

/// The id of the agent of `harness` among the bench's workers.
pub fn agent_id(harness: Harness) -> String {
    format!("bench-{}", harness.name())
}

/// What the human types to the chief to have it give a worker a task: the command,
/// and not to accept the result yet, so that the continuation has it to follow.
pub fn dispatch_brief(tier: &str, marker: &str) -> String {
    format!(
        "Run exactly this command in your shell, then reply with one line:\ncf task add {} \"Reply with exactly: {marker}\"\nWhen its result arrives, do not accept it yet: reply with one line and wait for my next message.",
        tier_flag(tier).join(" ")
    )
}

/// What the human types to the chief to have it send a follow-up to the window that
/// did the task `number`.
pub fn continue_brief(number: i64, marker: &str) -> String {
    format!(
        "Run exactly this command in your shell, then reply with one line:\ncf task add --after T-{number} \"Reply with exactly: {marker}\""
    )
}

/// Runs the baseline of `worker`, and its continuation.
pub fn run(bench: &mut Bench, worker: &Worker) -> Result<()> {
    let (name, agent) = (worker.name(), worker.agent());
    let marker = format!("BENCH_OK_{}", name.to_uppercase());
    let started = Instant::now();
    let told = bench.tell(&dispatch_brief(&worker.tier, &marker));
    let task = if told {
        bench.wait(&format!("{name}'s task, given by the chief"), || {
            Ok(bench
                .tasks_of(&agent)?
                .into_iter()
                .find(|task| task["requester"] == "chief"))
        })?
    } else {
        None
    };
    let detail = match &task {
        Some(task) => took(seconds_since(started), json!({ "task": task["number"] })),
        None => took(
            seconds_since(started),
            json!({ "chiefActivity": bench.chief_activity()? }),
        ),
    };
    bench.record(
        &format!("{name}-dispatched-by-chief"),
        task.is_some(),
        &detail,
    );
    let Some(task) = task else {
        return Ok(());
    };
    let number = task["number"].as_i64().unwrap_or_default();

    let done = bench.wait(&format!("{name}'s task to be done"), || {
        Ok(bench
            .task_of(&agent, number)?
            .filter(|task| task["state"] == "done"))
    })?;
    let result = if done.is_some() {
        bench
            .chief_inbox()?
            .into_iter()
            .find(|message| message["kind"] == "result" && message["taskNumber"] == number)
    } else {
        None
    };
    let body = result
        .as_ref()
        .and_then(|result| result["body"].as_str())
        .unwrap_or_default();
    let detail = if done.is_some() {
        took(
            seconds_since(started),
            json!({ "result": result.as_ref().map(|_| first_chars(body, 80)) }),
        )
    } else {
        let pane = bench.pane_of(&agent)?;
        took(
            seconds_since(started),
            json!({
                "worker": bench.members.lane(&agent)?["activity"],
                "state": bench.task_of(&agent, number)?.map(|task| task["state"].clone()),
                "exits": bench.members.rig().exits().into_iter().filter(|exit| exit["id"] == pane.as_str()).collect::<Vec<_>>(),
                "output": bench.output_of(&agent, 1_500)?,
            }),
        )
    };
    bench.record(
        &format!("{name}-answered"),
        done.is_some() && body.contains(&marker),
        &detail,
    );
    let Some(result) = result else {
        return Ok(());
    };
    let id = result["id"].clone();
    let delivered = bench.wait("the result to be delivered to the chief", || {
        Ok(bench
            .chief_inbox()?
            .into_iter()
            .find(|message| message["id"] == id && message["state"] == "delivered"))
    })?;
    let detail = if delivered.is_some() {
        took(seconds_since(started), json!({}))
    } else {
        let state = bench
            .chief_inbox()?
            .into_iter()
            .find(|message| message["id"] == id);
        took(
            seconds_since(started),
            json!({ "state": state, "chief": bench.chief_activity()? }),
        )
    };
    bench.record(
        &format!("{name}-received-by-chief"),
        delivered.is_some(),
        &detail,
    );

    // One task per session: the worker's window closes once its task is done.
    let closed = find(
        bench.log,
        &format!("{name}'s window to close"),
        CLOSES,
        Duration::from_secs(1),
        || Ok((bench.members.lane(&agent)?["activity"]["state"] == "closed").then_some(())),
    )?;
    let detail = if closed.is_some() {
        json!({})
    } else {
        json!({ "activity": bench.members.lane(&agent)?["activity"] })
    };
    bench.record(&format!("{name}-window-closed"), closed.is_some(), &detail);
    continued(bench, worker, &task)
}

/// Continuation: the chief sends a follow-up to the window that did the task; it
/// comes back on its own conversation and answers again.
fn continued(bench: &mut Bench, worker: &Worker, task: &Value) -> Result<()> {
    let (name, agent) = (worker.name(), worker.agent());
    let number = task["number"].as_i64().unwrap_or_default();
    let again = format!("BENCH_AGAIN_{}", name.to_uppercase());
    let begun = Instant::now();
    let told = bench.tell(&continue_brief(number, &again));
    let follow = if told {
        bench.wait("the follow-up task, given by the chief", || {
            Ok(bench.board_tasks()?.into_iter().find(|candidate| {
                candidate["requester"] == "chief"
                    && candidate["number"].as_i64().unwrap_or_default() > number
                    && candidate["pool"].is_null()
            }))
        })?
    } else {
        None
    };
    let detail = match &follow {
        Some(follow) => took(
            seconds_since(begun),
            json!({ "task": follow["number"], "window": follow["assignee"] }),
        ),
        None => took(
            seconds_since(begun),
            json!({ "chief": bench.chief_activity()? }),
        ),
    };
    bench.record(
        &format!("{name}-continued-in-same-window"),
        follow
            .as_ref()
            .is_some_and(|follow| follow["assignee"] == task["assignee"]),
        &detail,
    );
    let Some(follow) = follow else {
        return Ok(());
    };
    let follow_number = follow["number"].as_i64().unwrap_or_default();
    let finished = bench.wait("the follow-up task to be done", || {
        Ok(bench
            .board_tasks()?
            .into_iter()
            .find(|candidate| candidate["number"] == follow_number && candidate["state"] == "done"))
    })?;
    let answered = if finished.is_some() {
        bench
            .chief_inbox()?
            .into_iter()
            .find(|message| message["kind"] == "result" && message["taskNumber"] == follow_number)
    } else {
        None
    };
    let body = answered
        .as_ref()
        .and_then(|answered| answered["body"].as_str())
        .unwrap_or_default();
    let detail = if answered.is_some() {
        took(
            seconds_since(begun),
            json!({ "result": first_chars(body, 80) }),
        )
    } else {
        took(
            seconds_since(begun),
            json!({
                "state": bench.task_of(&agent, follow_number)?.map(|task| task["state"].clone()),
                "output": bench.output_of(&agent, 1_200)?,
            }),
        )
    };
    bench.record(
        &format!("{name}-continued-answered"),
        answered.is_some() && body.contains(&again),
        &detail,
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_human_has_the_chief_run_the_command_that_gives_the_workers_tier_a_task_and_wait() {
        assert_eq!(
            dispatch_brief("light", "BENCH_OK_OPENCODE"),
            "Run exactly this command in your shell, then reply with one line:\n\
             cf task add --tier light \"Reply with exactly: BENCH_OK_OPENCODE\"\n\
             When its result arrives, do not accept it yet: reply with one line and wait for my next message."
        );
        // Critical work names a purpose, and the flag carries it.
        assert!(dispatch_brief("critical", "X").contains(
            "cf task add --tier critical --purpose hard-problem \"Reply with exactly: X\""
        ));
    }

    #[test]
    fn the_follow_up_is_a_task_after_the_one_the_window_did() {
        assert_eq!(
            continue_brief(3, "BENCH_AGAIN_PI"),
            "Run exactly this command in your shell, then reply with one line:\n\
             cf task add --after T-3 \"Reply with exactly: BENCH_AGAIN_PI\""
        );
    }

    #[test]
    fn a_worker_is_named_by_its_harness_and_has_an_agent_of_the_benchs_own() {
        let worker = Worker {
            harness: Harness::Devin,
            tier: "critical".to_owned(),
        };
        assert_eq!(
            (worker.name(), worker.agent().as_str()),
            ("devin", "bench-devin")
        );
        assert_eq!(agent_id(Harness::Claude), "bench-claude");
    }
}

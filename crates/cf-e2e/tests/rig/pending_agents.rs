//! A turn that ends while background agents it started still run (the report of
//! 2026-10-11: a chief's four research agents, and a result that waited twelve
//! minutes for the last). Claude Code runs them in its own process, says how many
//! a turn left on the `turn_duration` that ends it, and reports its status `busy`
//! for as long as any runs, though its prompt takes input; the end of one wakes a
//! turn of its own. End to end through the real daemon and pane host (`npm run
//! test:integration`): fake agents in real PTYs that do all of that (`AGENTS` of
//! the fake agent, which starts no program: the pane host can see nothing of them).
//!
//! - A result for a chief whose turn ended with agents pending goes into its
//!   window at once, though they run on: its window is at rest, and nothing waits
//!   for them.
//! - A worker's turn that ends so is its task's result at once (the owner's decision
//!   of 2026-10-13, which reverses the hold of 2026-10-11): nothing waits for the
//!   agents, the chief is told in a note beside the result that the turn left them,
//!   and they end with the window that closes after the result.
//! - A switch the human asked for after the chief's turn is no message: it closes
//!   the window, and the agents end with it. It waits for a turn that leaves none.

use std::thread;

use cf_e2e::rig::{Project, Rig, OPEN};
use serde_json::{json, Value};

use crate::{config, secs, session_of, Outcome};

/// How long the chief's agent runs: past anything the case waits for.
const LONG: u64 = 150;

/// How long an agent runs that ends by itself in the case: past the time the case
/// looks at it, and not much more.
const SHORT: u64 = 30;

/// The tier the worker was given, as a word the chief's brief names.
fn tier(added: &Value) -> &str {
    added["member"]["tier"].as_str().unwrap_or_default()
}

/// The results the chief has been given, oldest first.
fn results(project: &Project) -> cf_e2e::Result<Vec<Value>> {
    Ok(project
        .inbox("chief")?
        .into_iter()
        .filter(|message| message["kind"] == "result")
        .collect())
}

/// How many turns of the conversation `session` ended with an agent still
/// running.
fn turns_with_agents(rig: &Rig, session: &str) -> usize {
    rig.transcript(session)
        .matches(r#""pendingBackgroundAgentCount":1"#)
        .count()
}

#[test]
fn a_result_for_a_chief_whose_turn_ended_with_agents_pending_goes_in_at_once_and_does_not_wait_for_them(
) -> Outcome {
    let rig = Rig::start(config())?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    let session = session_of(&rig.open_frame(&format!("p{}-chief", project.id()), OPEN)?);

    // The chief gives a worker a task that takes a while, then starts a research
    // agent, for longer than the case runs, and ends its turn.
    rig.tell(
        project.id(),
        &format!(
            "DISPATCH --tier {} SLEEP 25 Reply with exactly: WORKER_OK",
            tier(&added)
        ),
    )?;
    rig.tell(
        project.id(),
        &format!("AGENTS 1 {LONG} started the research"),
    )?;
    rig.wait_for(
        "the chief's turn to end with its agent running",
        secs(60),
        || Ok(turns_with_agents(&rig, &session) == 1),
    )?;

    // The worker's result comes while the agent runs: it goes into the chief's
    // window, which answers it, its agent running still.
    rig.wait_for("T-1 to be done", secs(90), || {
        Ok(project.task(1)?["state"] == "done")
    })?;
    rig.wait_for("the result to reach the chief's window", secs(30), || {
        Ok(rig.transcript(&session).contains("result from @"))
    })?;
    rig.wait_for("the chief to answer it", secs(30), || {
        Ok(turns_with_agents(&rig, &session) == 2)
    })?;
    assert_ne!(
        results(&project)?[0]["state"],
        "queued",
        "the result is in the chief's window, with its agent still running"
    );
    rig.close()?;
    Ok(())
}

#[test]
fn a_workers_turn_that_ends_with_agents_pending_is_its_result_at_once_and_the_chief_is_told_it_left_them(
) -> Outcome {
    let rig = Rig::start(config())?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    // Agents that run for a minute: the result does not wait for them.
    rig.tell(
        project.id(),
        &format!(
            "DISPATCH --tier {} AGENTS 2 60 started the research",
            tier(&added)
        ),
    )?;
    rig.wait_for("T-1 to be done", secs(30), || {
        Ok(project.task(1)?["state"] == "done")
    })?;
    rig.wait_for("the result to reach the chief", secs(30), || {
        Ok(!results(&project)?.is_empty())
    })?;
    assert_eq!(
        results(&project)?[0]["body"],
        "started the research",
        "the turn the agents' end would have woken is not waited for"
    );
    rig.wait_for("the chief to be told it left them", secs(30), || {
        Ok(project.inbox("chief")?.iter().any(|message| {
            message["kind"] == "note"
                && message["body"] == "T-1's turn left background agents running."
        }))
    })?;
    assert_eq!(
        project.lane("worker")?["activity"]["reason"],
        Value::Null,
        "the board says no work is running"
    );
    rig.close()?;
    Ok(())
}

#[test]
fn a_switch_after_the_chiefs_turn_waits_for_its_agents_which_would_end_with_its_window_and_goes_when_a_turn_ends_with_none(
) -> Outcome {
    let rig = Rig::start(config())?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let id = project.id();
    let chiefs = || {
        rig.open_frames()
            .into_iter()
            .filter(|frame| frame["id"] == format!("p{id}-chief").as_str())
            .count()
    };
    let session = session_of(&rig.open_frame(&format!("p{id}-chief"), OPEN)?);

    // The chief starts a research agent that ends by itself in a while, and ends
    // its turn.
    rig.tell(id, &format!("AGENTS 1 {SHORT} started the research"))?;
    rig.wait_for(
        "the chief's turn to end with its agent running",
        secs(60),
        || Ok(turns_with_agents(&rig, &session) == 1),
    )?;

    // The human switches the chief after its turn. A switch closes the window,
    // and the agent would end with it: it waits, though the window is at rest.
    let switched = rig.page(
        "chief.switch",
        json!({ "project": id, "agent": "worker", "when": "turn" }),
    )?;
    assert_eq!(switched["ok"], true, "{switched}");
    thread::sleep(secs(6));
    assert_eq!(chiefs(), 1, "no second window yet");
    assert!(rig.exits().is_empty(), "the chief's window is open");

    // The agent ends, the turn it wakes leaves none, and the chief goes.
    rig.wait_for("a second chief window to be opened", secs(60), || {
        Ok(chiefs() == 2)
    })?;
    assert!(
        rig.transcript(&session)
            .contains(r#""text":"woken: started the research""#),
        "the old chief's last turn was the one its agent's end woke"
    );
    rig.close()?;
    Ok(())
}

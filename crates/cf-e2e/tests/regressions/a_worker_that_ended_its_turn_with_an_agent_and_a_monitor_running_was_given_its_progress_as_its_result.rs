//! A worker that ended its turn while an asynchronous Agent and a Monitor still ran was
//! given up for done, with its progress for its result.
//!
//! Seen on a field board (kluris, T-47, 2026-10-24, ConsensFlow 3.0.6-alpha.1; the diagnosis
//! is T-674's, `operations/work-in-flight/results/kluris-t47-diagnosis-T-674.md` of
//! consensflow-sme). A Claude Code worker launched an Agent in the background, armed two
//! Monitors (one of which ended inside the turn) and ended its turn. ConsensFlow took the turn
//! for finished at once, without holding it for the agent and the monitor that were still
//! running, and the result it gave the chief was all seven of the turn's progress messages
//! joined, a report in none of them. The chief had to open the task again. alpha.2's `8504697f`
//! mended both: a hold for the jobs and agents a turn leaves (`turn_end.rs`, `Hold`), and a
//! result that is the turn's last words and not the progress before them. Each is tested apart
//! (`dispatcher/turn_end.rs`, `pending_agents.rs`, `final_answer.rs`, the reader's
//! `record/tests/agents.rs`, the rig's `pending_agents.rs`); none was in the field's shape,
//! from the harness's record to the board.
//!
//! What the chief should have seen: T-1 stays working, held for the agent and the monitor (one
//! each), with no result; the chief is told once, in the words of the turn's last message, and
//! nothing is said of anything ended; when the agent ends the hold is the monitor's alone, and
//! the chief is told nothing more; and the turn the monitor's end wakes is the result, once,
//! its report and none of the progress before it. A worker that left an agent alone is held
//! for it the same way, so that a monitor's count cannot hide a broken agent's.
//!
//! The stand-in writes what Claude Code 2.1.3 writes of such a turn (`CLAUDE-HELD` of the fake
//! agent), with words of its own: four steps of progress, a Monitor armed and its end taken into
//! the turn (an attachment, the queue's copy absorbed), an Agent whose call's result says it
//! was launched asynchronously, a second Monitor that is not ended, the last message, the stop
//! hooks' summary and a `turn_duration` that counts the one agent. A moment lies between the
//! summary and the duration (`CF_TEST_DURATION_MS`), the window's status `busy` through it, so
//! that a look falls between them: the turn reads as over with no agent, and must not be taken
//! for the result. Nothing ends by a clock: the agent and the monitor end when the case says
//! so (`Job::let_its_agent_end`, `Job::let_its_monitor_end`), after it has seen the board in
//! the state it looks for.

use cf_e2e::rig::{Project, Rig};
use cf_e2e::Error;
use serde_json::json;

use crate::a_workers_still_going_was_given_as_its_result::{tier, told};
use crate::{config, secs, Outcome};

/// What the worker said as its turn ended, with its agent and monitor running.
const WORDS: &str = "The survey runs in the background; I will write it up when it ends";

/// What the turn the monitor's end woke reported.
const REPORT: &str = "The parser has three entry points and none of them is unsafe";

/// What the worker's agent reported when it ended, where a monitor still ran.
const AGENT_ENDED: &str = "The agent ended; the monitor still watches.";

/// How long the stand-in waits between the stop hooks' summary and the `turn_duration`: five
/// passes of the daemon (a second each), in which it looks at a window whose turn the record
/// says is over and whose status says `busy`, with no agent counted yet. It is a chance for the
/// look to fall there and no bound of the case's: a case whose daemon looked at no time in it
/// passes all the same, and the plant that takes a busy window for one at rest (T-1332) says
/// whether the look fell there.
const BETWEEN: &str = "5000";

/// The one job of the stand-in's `held` files: the file the case writes its gates beside.
fn held(rig: &Rig) -> cf_e2e::Result<cf_e2e::rig::Job> {
    rig.jobs("held")
        .into_iter()
        .next()
        .ok_or_else(|| Error::Daemon("the worker's turn left nothing held".to_owned()))
}

/// What the chief was told of T-1 besides its result: the notes' words.
fn notes(project: &Project) -> cf_e2e::Result<Vec<String>> {
    Ok(told(project)?
        .into_iter()
        .filter(|(kind, _)| kind == "note")
        .map(|(_, body)| body)
        .collect())
}

/// The case, from the worker's brief to its result: `line` is the stand-in's verb, `held` what
/// the worker's lane shows while the turn's agent and monitor run (`jobs`, `agents`).
fn run(line: &str, jobs: u64, agents: u64) -> Outcome {
    let rig = Rig::start(config().var("CF_TEST_DURATION_MS", BETWEEN))?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    rig.tell(
        project.id(),
        &format!("DISPATCH --tier {} {line}", tier(&added)),
    )?;
    rig.wait_for("the chief to be told it goes on", secs(120), || {
        Ok(notes(&project)?.iter().any(|body| body.contains(WORDS)))
    })?;

    // Held for what it left running: the lane says how many of each, the task works on, and
    // the chief has no result, and one note, in the last words of the turn and none of its
    // progress.
    rig.wait_for("the lane to show what is held", secs(60), || {
        let held = &project.lane("worker")?["held"];
        Ok(held["jobs"] == jobs && held["agents"] == agents)
    })?;
    assert_eq!(project.task(1)?["state"], "working");
    let thread = told(&project)?;
    let kinds: Vec<&str> = thread.iter().map(|(kind, _)| kind.as_str()).collect();
    assert_eq!(kinds, ["note"], "the hold, told once: {thread:?}");
    let hold = &thread[0].1;
    assert!(
        hold.starts_with("T-1 is still working in the background (")
            && hold.ends_with(&format!("What it said:\n\n{WORDS}")),
        "told in the turn's last words: {hold}"
    );
    assert!(
        !hold.contains("Step 1 of") && !hold.contains("left background"),
        "none of the progress, and nothing ended: {hold}"
    );
    let mut running = Vec::new();
    if agents > 0 {
        running.push("1 agent");
    }
    if jobs > 0 {
        running.push("1 monitor");
    }
    let held_for = format!("({}); its result comes when", running.join(", "));
    assert!(
        hold.contains(&held_for),
        "held for {held_for:?}, and the note says so: {hold}"
    );

    // The agent ends: a turn of its own answers (the words of the monitor still watching,
    // or the report where nothing else is held), and the chief is told nothing more.
    held(&rig)?.let_its_agent_end()?;
    if jobs > 0 {
        rig.wait_for("the hold to be the monitor's alone", secs(120), || {
            let held = &project.lane("worker")?["held"];
            Ok(held["jobs"] == jobs && held["agents"] == 0)
        })?;
        assert_eq!(project.task(1)?["state"], "working");
        assert_eq!(
            told(&project)?.len(),
            1,
            "nothing more is told, and no result: {:?}",
            told(&project)?
        );
        held(&rig)?.let_its_monitor_end()?;
    }

    // The turn the last end woke is the result, once: its report, and none of the progress
    // before it or of the turn the agent's end woke.
    rig.wait_for("T-1 to be done", secs(120), || {
        Ok(project.task(1)?["state"] == "done")
    })?;
    let thread = told(&project)?;
    let kinds: Vec<&str> = thread.iter().map(|(kind, _)| kind.as_str()).collect();
    assert_eq!(kinds, ["note", "result"], "{thread:?}");
    assert_eq!(thread[1].1, REPORT);
    assert!(
        !thread[1].1.contains("Step ") && !thread[1].1.contains(AGENT_ENDED),
        "{}",
        thread[1].1
    );
    assert_eq!(notes(&project)?.len(), 1, "the chief was told once");
    rig.close()?;
    Ok(())
}

#[test]
fn a_worker_whose_turn_ended_with_an_agent_and_a_monitor_running_is_held_for_both_and_its_report_is_the_result(
) -> Outcome {
    run(&format!("CLAUDE-HELD {WORDS} :: {REPORT}"), 1, 1)
}

#[test]
fn a_worker_whose_turn_ended_with_an_agent_alone_running_is_held_for_it_and_its_report_is_the_result(
) -> Outcome {
    // The agent's end wakes the turn that reports: no monitor is there to hold the task, or to
    // make up a count that is the agent's.
    run(
        &format!("CLAUDE-HELD --agent-only {WORDS} :: {REPORT}"),
        0,
        1,
    )
}

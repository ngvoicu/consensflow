//! A worker's turn that ended to wait for its own work was given as its result.
//!
//! Seen by the owner on 2026-11-21, on 3.0.7-alpha.12, twice (T-1923; the advice T-1919). A
//! Claude Code worker ended its turn on a line about waiting for runs it had started, with
//! nothing pending that the board could see, and the board took the line for the task's
//! result: "Final-head workspace, whole rig and Windows harness runs are going (head …); I'll
//! check them at the next wakeup." (T-1887) and "Windows dispatcher passes: 1791 tests. The
//! rig is still waiting for its build turn, so I'm ending this turn until it runs." (T-1910).
//! The chief decided on an interim line, and had to reopen each task for the work to go on.
//!
//! What the owner should have seen: no result for such a turn. The worker's window is told
//! once to go on and wait inside its turn, the chief is told nothing, and the turn the note
//! begins ends with the result. The words are the worker's own, so the case is the same for
//! each harness the rig has a stand-in for that pastes a note into a window: Claude Code,
//! Codex and Devin (the rig's Muse echoes its input and its Pi is not seated).
//!
//! The worker is the stand-in, whose `SAY` line answers the words it is given as its last
//! words; the note it is then sent has no such line, so its answer is the stand-in's `noted:`.

use cf_e2e::rig::{Config, Project, Rig};
use serde_json::json;

use crate::a_workers_still_going_was_given_as_its_result::{tier, told};
use crate::{config, config_with_codex, devin_config, secs, Outcome};

/// T-1887's m-12797.
const AT_THE_NEXT_WAKEUP: &str = "Final-head workspace, whole rig and Windows harness runs are going (head c61c13a60); I'll check them at the next wakeup.";

/// T-1910's m-12911.
const ENDING_THE_TURN: &str = "Windows dispatcher passes: 1791 tests. The rig is still waiting for its build turn, so I'm ending this turn until it runs.";

/// What a window of a harness is, for the rig.
fn on(harness: &str) -> Config {
    match harness {
        "codex" => config_with_codex(),
        "devin" => devin_config(),
        _ => config(),
    }
}

/// A board whose worker, of `harness`, ends its turn on T-1 with `words`: the project, once T-1
/// is done.
fn case(harness: &str, words: &str) -> Outcome {
    let rig = Rig::start(on(harness))?;
    rig.stand_ins(&json!({
        "schemaVersion": 1,
        "agents": [
            { "id": "chief", "kind": "claude-code", "model": "fake-chief" },
            { "id": "worker", "kind": harness, "model": "fake", "workTier": "standard" },
        ],
    }))?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    rig.tell(
        project.id(),
        &format!("DISPATCH --tier {} SAY {words}", tier(&added)),
    )?;
    rig.wait_for(
        "T-1 to be done on the turn the note began",
        secs(120),
        || Ok(project.task(1)?["state"] == "done"),
    )?;

    let thread = told(&project)?;
    let kinds: Vec<&str> = thread.iter().map(|(kind, _)| kind.as_str()).collect();
    assert_eq!(
        kinds,
        ["result"],
        "no interim result, no word of the note: {thread:?}"
    );
    let result = &thread[0].1;
    assert_ne!(result, words, "the wait was the result");
    assert!(
        result.starts_with("noted: [ConsensFlow m-") && result.contains("note from ConsensFlow]"),
        "the result is the stand-in's answer to the go-on note the window was given: {result}"
    );
    rig.close()?;
    Ok(())
}

#[test]
fn a_claude_code_turn_that_ended_to_wait_is_sent_on_and_its_next_turn_is_the_result() -> Outcome {
    case("claude-code", AT_THE_NEXT_WAKEUP)?;
    case("claude-code", ENDING_THE_TURN)
}

#[test]
fn a_codex_turn_that_ended_to_wait_is_sent_on_and_its_next_turn_is_the_result() -> Outcome {
    case("codex", ENDING_THE_TURN)
}

#[test]
fn a_devin_turn_that_ended_to_wait_is_sent_on_and_its_next_turn_is_the_result() -> Outcome {
    case("devin", AT_THE_NEXT_WAKEUP)
}

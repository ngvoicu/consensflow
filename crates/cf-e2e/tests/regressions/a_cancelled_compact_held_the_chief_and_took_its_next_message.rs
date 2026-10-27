//! A `/compact` the human cancelled with Escape held the Claude chief, and would have taken
//! the next message given to it.
//!
//! Seen on 2026-10-20 by the live suite `commands-at-rest` (Claude Code 2.1.304), and again
//! on 2.1.310 (T-483): `/compact` typed into an idle Claude, and Escape a second later. Claude
//! writes of that the bare `/compact` as it was typed and, at the Escape, the words it writes
//! before a command (`<local-command-caveat>`), a sibling of the bare command, and nothing
//! more: no boundary, no summary, no command. It goes back to its prompt with the `/compact`
//! put back in its input box. ConsensFlow read the turn the bare command began as in flight
//! until the next prompt, so a chief whose human did that read as working, and a worker's
//! result waited for the human to type. A reader that read the cancel at rest alone would
//! have pasted the result into the box behind the `/compact`: sent as one, a compaction ran
//! with the result as its instructions, and the result never reached the model (the suite's
//! second run). The report is the issue
//! `issues/a-cancelled-compact-stalls-the-chief-and-restores-its-input.md` in consensflow-sme.
//!
//! What should have been seen: the chief at rest once Claude is back at its prompt, and the
//! worker's result handed to it alone, as a message of its own.
//!
//! The case is that shape on the rig: the chief hands a worker a turn of 45 s and ends its
//! own; the human types `/compact` into its idle window and Escape a second later, and its
//! stand-in, whose compaction takes 30 s (`CF_TEST_COMPACT_MS`), writes what Claude Code
//! writes of the cancel and puts the `/compact` back in its input box (`fake_agent`'s
//! `cancelled`), its box drawn as Claude draws it (`CF_TEST_INPUT_BOX`). The worker's result
//! must reach the chief as a message that begins with its header, and nothing be compacted.
//! In the second case the chief draws no box the adapter can read: the result must wait, the
//! board telling the human the window holds what they typed, and nothing go in behind the
//! `/compact` (a paste into a box not known empty would).

use std::thread::sleep;
use std::time::Duration;

use cf_e2e::rig::{Project, Rig};
use serde_json::{json, Value};

use crate::a_compact_typed_into_an_idle_chief_held_a_workers_result::tier;
use crate::resume_of_a_chief_with_no_turn::{after, wait_for_the_chief, windows_of};
use crate::{config, secs, Outcome};

/// The human's keys: `/compact`, the Enter that sends it, and the Escape that cancels it.
const COMPACT: &[u8] = b"/compact";
const ENTER: &[u8] = b"\r";
const ESCAPE: &[u8] = &[27];

/// The chief's record's user rows, as their words.
fn user_rows(transcript: &str) -> Vec<String> {
    transcript
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|record| record["type"] == "user")
        .filter_map(|record| record["message"]["content"].as_str().map(str::to_owned))
        .collect()
}

/// The chief's result, once it is given one: its id, and whether it is delivered.
fn result(project: &Project) -> cf_e2e::Result<Option<(i64, bool)>> {
    Ok(project
        .inbox("chief")?
        .iter()
        .find(|message| message["kind"] == "result")
        .map(|message| {
            (
                message["id"].as_i64().unwrap_or_default(),
                message["state"] == "delivered",
            )
        }))
}

/// The chief, given a worker's turn of 45 s, and cancelled by the human in a `/compact`
/// typed into its idle window: the rig, the project and the chief's conversation, once the
/// cancel is written and the chief reads idle again, the worker still at work.
fn cancelled_with_the_worker_at_work(
    draws_its_box: bool,
) -> Result<(Rig, i64, String), Box<dyn std::error::Error>> {
    let mut configured = config().var("CF_TEST_COMPACT_MS", "30000");
    if draws_its_box {
        configured = configured.var("CF_TEST_INPUT_BOX", "chief");
    }
    let rig = Rig::start(configured)?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    let session = windows_of(&rig, &project)
        .first()
        .and_then(|first| after(first, "--session-id"))
        .ok_or("the chief's window has no session")?
        .to_owned();
    rig.tell(
        project.id(),
        &format!(
            "DISPATCH --tier {} SLEEP 45 Reply with exactly: WORKER_OK",
            tier(&added)
        ),
    )?;
    rig.wait_for("T-1 to be at work", secs(60), || {
        Ok(project.task(1)?["state"] == "working")
    })?;
    wait_for_the_chief(&rig, &project)?;

    // The human types /compact into the chief's idle window, a key at a time, and Escape a
    // moment after it began compacting.
    let pane = project.lane("chief")?["pane"].clone();
    for keys in [COMPACT, ENTER] {
        rig.types(&pane, keys)?;
        sleep(Duration::from_millis(600));
    }
    rig.wait_for("the chief's record to say it compacts", secs(30), || {
        Ok(user_rows(&rig.transcript(&session))
            .last()
            .is_some_and(|row| row == "/compact"))
    })?;
    sleep(Duration::from_secs(1));
    rig.types(&pane, ESCAPE)?;
    rig.wait_for(
        "the chief's record to say it was cancelled",
        secs(30),
        || {
            Ok(user_rows(&rig.transcript(&session))
                .last()
                .is_some_and(|row| row.starts_with("<local-command-caveat>")))
        },
    )?;
    // Claude is back at its prompt, and so is the chief.
    wait_for_the_chief(&rig, &project)?;
    assert_eq!(
        project.task(1)?["state"],
        "working",
        "the worker's result came before the cancel: the case holds nothing"
    );
    let id = project.id();
    Ok((rig, id, session))
}

#[test]
fn a_claude_chief_whose_compact_the_human_cancelled_is_at_rest_and_is_handed_its_workers_result_alone(
) -> Outcome {
    let (rig, id, session) = cancelled_with_the_worker_at_work(true)?;
    let project = Project::new(&rig, id);

    // The worker answers, and its result goes into the chief's window alone.
    rig.wait_for("T-1 to be done", secs(90), || {
        Ok(project.task(1)?["state"] == "done")
    })?;
    let mut handed = None;
    rig.wait_for(
        "the worker's result to be handed to the chief",
        secs(60),
        || {
            handed = result(&project)?.and_then(|(id, delivered)| {
                (delivered
                    && rig
                        .transcript(&session)
                        .contains(&format!("[ConsensFlow m-{id} ")))
                .then_some(id)
            });
            Ok(handed.is_some())
        },
    )?;
    let id = handed.ok_or("no result was handed")?;
    let transcript = rig.transcript(&session);
    let header = format!("[ConsensFlow m-{id} ");
    let rows = user_rows(&transcript);
    assert!(
        rows.iter().any(|row| row.starts_with(&header)),
        "the result went in behind what the box held: {rows:?}"
    );
    assert!(
        !transcript.contains("compact_boundary"),
        "a compaction ran: the result was its instructions"
    );
    wait_for_the_chief(&rig, &project)?;
    rig.close()?;
    Ok(())
}

#[test]
fn a_claude_chief_whose_box_cannot_be_read_after_a_cancelled_compact_is_handed_nothing_behind_it(
) -> Outcome {
    let (rig, id, session) = cancelled_with_the_worker_at_work(false)?;
    let project = Project::new(&rig, id);
    rig.wait_for("T-1 to be done", secs(90), || {
        Ok(project.task(1)?["state"] == "done")
    })?;
    // The result waits while the box may hold the `/compact`: nothing is pasted behind it.
    sleep(Duration::from_secs(15));
    let rows = user_rows(&rig.transcript(&session));
    assert!(
        rows.last()
            .is_some_and(|row| row.starts_with("<local-command-caveat>")),
        "something went into the box behind the /compact: {rows:?}"
    );
    assert!(
        result(&project)?.is_some_and(|(_, delivered)| !delivered),
        "the result was handed over"
    );
    // The board tells the human the window waits for what is in its box.
    assert_eq!(project.lane("chief")?["holding"], true);
    rig.close()?;
    Ok(())
}

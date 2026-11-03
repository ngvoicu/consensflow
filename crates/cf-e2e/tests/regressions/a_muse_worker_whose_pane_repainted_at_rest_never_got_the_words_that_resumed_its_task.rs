//! A Muse worker whose pane went on writing at rest never got the words that resumed its
//! task.
//!
//! Seen by T-1139's live runs on 2026-11-03 (a Mac, Muse Code 1.4.14, 3.0.7-alpha.1
//! plus T-1139's branch, which had no screen clock), the same night's shape as T-1081's
//! of 2026-11-02. The chief told the worker's window inside a long command (`cf tell`,
//! which stops the command), the worker answered, and the chief resumed the task. The
//! window's turn ended; its record said so; its screen showed an empty box. The resume's
//! words were held for four minutes, the trace saying only "its screen printed within the
//! last 3 s, and its harness says nothing of its own rest", and never went in. The
//! worker's task read queued while its window waited for words that were not coming.
//!
//! What the pane did: Muse, at rest after a command it ran was stopped, writes colour
//! resets every 50 ms and shows the same. The daemon read a window with no word of its own
//! that it is at rest (Muse's, whose TUI says none) by its output going quiet for the
//! settle, so a pane that never stopped writing was never at rest. Re-run on T-1083's
//! branch, which judges the screen by what it shows, the same live case let the words in
//! at +81.5 s, the pane having printed 45 ms before and its screen having held still for
//! 4.3 s (`.runs/T-1155/live-muse-1.log`).
//!
//! The report is `issues/a-window-that-repaints-never-counts-as-quiet.md` in consensflow-sme.
//!
//! Echo never works in the background, so it never gets into that state: the rig's Muse
//! is asked to write the same bytes at rest (`CF_TEST_MUSE_REPAINT_MS`, which
//! [`cf_e2e::muse::write_stand_in`] explains), beside the real program, its TUI, its hooks and
//! its record, on the real daemon and pane host. The shape is a task stopped before its
//! first word and resumed (a message the pacing gates: the resume's words, pasted at rest).
//!
//! What the owner should have seen: the resume's words go in on their first attempt, the pane
//! having printed within the last 3 s and the screen having shown the same for the settle
//! (the look `delivery.begun` keeps), and the task ends with its result.

use cf_e2e::muse::members::{echo_worker, in_its_turn, project};
use cf_e2e::muse::{self, has, terminals};
use cf_e2e::rig::{Config, Rig};
use serde_json::{json, Value};

use crate::{config, secs, Outcome};

/// How long the echo turn is: long enough for the pause to meet it at work.
const TURN_MS: u64 = 20_000;

/// How often the rig's Muse repaints at rest, as the real one did (T-1081's probe: every
/// 50 ms).
const REPAINT_MS: &str = "50";

/// The daemon's settle, which a message waits for (`Limits::settle_ms`).
const SETTLE_MS: i64 = 3_000;

/// A rig whose Muse windows repaint at rest, or why there is no Muse to run.
fn repainting_rig() -> Option<Config> {
    match muse::installed() {
        Ok(install) => Some(
            config()
                .muse(install, TURN_MS)
                .var("CF_TEST_MUSE_REPAINT_MS", REPAINT_MS),
        ),
        Err(why) => {
            #[allow(clippy::print_stderr)] // Said in the output of the case it skips.
            {
                eprintln!("skipped: {why}");
            }
            None
        }
    }
}

/// The `delivery.begun` events of the messages the daemon let into a window it read by the
/// fallback (Muse's: its TUI says nothing of its own rest), oldest first, each with the look
/// that let it go.
fn looks(rig: &Rig) -> Vec<Value> {
    rig.events()
        .into_iter()
        .filter(|event| {
            event["kind"] == "delivery.begun"
                && event["data"]["look"]["status"] == "idle by the fallback"
        })
        .map(|event| event["data"].clone())
        .collect()
}

#[test]
fn a_muse_worker_whose_pane_repainted_at_rest_got_the_words_that_resumed_its_task() -> Outcome {
    let Some(config) = repainting_rig() else {
        return Ok(());
    };
    let rig = Rig::start(config)?;
    let project = project(&rig, &echo_worker())?;
    rig.tell(
        project.id(),
        "DISPATCH --tier standard Reply with exactly: T1_DONE",
    )?;
    in_its_turn(&rig, &project, 1, secs(90))?;
    let paused = rig.page("task.pause", json!({ "project": project.id(), "task": 1 }))?;
    assert_eq!(paused["ok"], true, "{paused}");
    rig.wait_for("T-1 to be paused", secs(60), || {
        Ok(project.task(1)?["state"] == "paused")
    })?;
    // One Escape stopped the turn before its first word; the window is at rest, and its
    // pane goes on writing.
    rig.wait_for("Muse to write the turn taken back", secs(30), || {
        Ok(has(
            &muse::members::the_session(&rig).1,
            "run/run_retracted",
        ))
    })?;
    let resumed = rig.page("task.resume", json!({ "project": project.id(), "task": 1 }))?;
    assert_eq!(resumed["ok"], true, "{resumed}");
    // The words that resumed it go in, and the task ends with its result: a window the
    // pacing held for its output never did.
    rig.wait_for("T-1 to be answered", secs(120), || {
        Ok(project.task(1)?["state"] == "done")
    })?;
    assert_eq!(
        terminals(&muse::members::the_session(&rig).1),
        ["cancelled", "completed"]
    );
    // What the daemon read of the window as it let the words in: the pane had written
    // within the last 3 s (the repaint was on) and its screen had shown the same for the
    // settle; it was read by the fallback, and went on its first attempt.
    let looks = looks(&rig);
    let resume = looks.last().expect("a look of the delivery of the words");
    assert_eq!(resume["attempt"], 1, "{resume}");
    let look = &resume["look"];
    assert_eq!(look["status"], "idle by the fallback", "{look}");
    let printed = look["outputQuietMs"].as_i64().expect("outputQuietMs");
    let shown = look["screenQuietMs"].as_i64().expect("screenQuietMs");
    assert!(
        printed < SETTLE_MS,
        "the pane had printed {printed} ms before: the repaint was on ({look})"
    );
    assert!(
        shown >= SETTLE_MS,
        "the screen had shown the same for {shown} ms ({look})"
    );
    rig.close()?;
    Ok(())
}

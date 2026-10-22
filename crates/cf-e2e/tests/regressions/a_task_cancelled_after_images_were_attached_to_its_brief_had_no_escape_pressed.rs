//! A task cancelled while its window was at work on a brief that had images attached had no
//! Escape pressed into its turn.
//!
//! Found on 2026-10-22 reading the code for T-215 (a result that starts with an image path was
//! pasted three times into a Claude chief, `a_result_that_starts_with_an_image_path_was_pasted_
//! three_times`); not seen by the owner as such. When Claude Code attaches the images a paste
//! names by their paths, its record has the message and, after it, a companion item of its own
//! that says where each picture came from (`[Image: source: …]`). The daemon stops the turn of a
//! task that was called off by looking at the last item of the user's in the window's record
//! (`Dispatcher::interrupt_cancelled`): the companion is that item, and it holds no header, so
//! the turn is taken for none about the task, and no Escape is pressed. The window is closed all
//! the same, by what closes the window of a task that is over, so the turn is cut off and not
//! interrupted: Claude writes no record of an interrupted turn, and what the cancel's three
//! rounds of Escape are for (the turn stopped where it is, its tools asked to stop) is skipped.
//! Only a brief (or a follow-up) with an image path on a line of its own can do it, and only for
//! a harness that attaches images and writes such a record (Claude Code). The report is T-215's
//! second finding; no issue in consensflow-sme holds it yet.
//!
//! What should be seen: the daemon presses Escape into the turn of the worker whose task was
//! called off, as it does for any other.
//!
//! The case is that shape on the rig: the chief hands the worker a turn of two minutes with the
//! path of a picture on a line of its own, the stand-in makes an attachment of it as Claude Code
//! does, and the human cancels the task. It is the known failure: ignored until it is fixed. Its
//! control, the same with no picture, passes.

use cf_e2e::files;
use cf_e2e::rig::{Project, Rig};
use serde_json::{json, Value};

use crate::resume_of_a_chief_with_no_turn::{after, wait_for_the_chief};
use crate::{config, secs, Outcome};

/// The Escape key, which stops a turn.
const ESCAPE: &[u8] = &[27];

/// The tier the worker was given, as a word the chief's brief names.
fn tier(added: &Value) -> &str {
    added["member"]["tier"].as_str().unwrap_or_default()
}

/// The windows of the project's worker the daemon opened so far, oldest first.
fn workers_windows(rig: &Rig, project: &Project) -> Vec<Value> {
    let chief = format!("p{}-chief", project.id());
    rig.open_frames()
        .into_iter()
        .filter(|frame| frame["id"] != chief.as_str())
        .collect()
}

#[test]
fn a_task_cancelled_while_its_window_works_on_a_brief_with_no_image_has_escape_pressed_into_its_turn(
) -> Outcome {
    cancelled_while_at_work(false)
}

#[test]
#[ignore = "known failure, T-215's second finding: no Escape is pressed into a turn whose message had images attached when its task is cancelled; its window is closed all the same (the issue page is for the chief to file in consensflow-sme)"]
fn a_task_cancelled_after_images_were_attached_to_its_brief_has_escape_pressed_into_its_turn(
) -> Outcome {
    cancelled_while_at_work(true)
}

/// A worker is given a turn of two minutes, with a picture's path on a line of its own where
/// `with_picture`, and the human cancels its task: Escape must be pressed into the turn. (Without
/// the picture it is: the control that holds the case to the companion record.)
fn cancelled_while_at_work(with_picture: bool) -> Outcome {
    let rig = Rig::start(config())?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    wait_for_the_chief(&rig, &project)?;
    let picture = rig.workspace().join("reference.png");
    files::write(&picture, cf_e2e::png::SMALL)?;

    // The brief: a turn of two minutes, then the picture's path on a line of its own.
    let path = if with_picture {
        format!("\\n{}", picture.display())
    } else {
        String::new()
    };
    rig.tell(
        project.id(),
        &format!(
            "DISPATCH --tier {} SLEEP 120 Reply with exactly: DONE{path}",
            tier(&added)
        ),
    )?;
    rig.wait_for("T-1 to be at work", secs(60), || {
        Ok(project.task(1)?["state"] == "working")
    })?;
    let window = workers_windows(&rig, &project)
        .into_iter()
        .next()
        .ok_or("the worker has no window")?;
    let session = after(&window, "--session-id")
        .ok_or("the worker's window has no session")?
        .to_owned();
    if with_picture {
        rig.wait_for(
            "the worker's record to show the picture attached",
            secs(30),
            || Ok(rig.transcript(&session).contains("[Image: source: ")),
        )?;
    }

    // The human calls it off: the worker's window is stopped, with Escape pressed into its turn
    // first. (Under load the daemon's close may come before the stand-in has written a record of
    // the interrupt, so the keys the daemon pressed are what is read.) The turn runs two minutes,
    // so the end of the window is not the end of the turn.
    let cancelled = rig.page("task.cancel", json!({ "project": project.id(), "task": 1 }))?;
    assert_eq!(cancelled["ok"], true, "{cancelled}");
    let id = window["id"].as_str().unwrap_or_default();
    rig.wait_for("the worker's window to be stopped", secs(75), || {
        let ended = rig
            .exits()
            .iter()
            .any(|exit| exit["id"] == window["id"] && exit["generation"] == window["generation"]);
        Ok(ended || rig.keys_pressed(id, ESCAPE) > 0)
    })?;
    assert!(
        rig.keys_pressed(id, ESCAPE) > 0,
        "the worker's window was ended with no Escape pressed into its turn"
    );
    rig.close()?;
    Ok(())
}

//! A task cancelled while its window was at work on a message that had images attached had no
//! Escape pressed into its turn.
//!
//! Found on 2026-10-22 reading the code for T-215 (a result that starts with an image path was
//! pasted three times into a Claude chief, `a_result_that_starts_with_an_image_path_was_pasted_
//! three_times`); fixed in T-265, the issue is the brain's `issues/a-cancelled-tasks-turn-with-
//! images-attached-gets-no-escape.md`. When Claude Code attaches the images a paste names by their
//! paths, its record has the message and, after it, a record of the companion's of its own that
//! says where each picture came from (`[Image: source: …]`, flagged `turnCompanion`). The daemon
//! stops the turn of a task that was called off by looking at the last item of the user's in the
//! window's record (`Dispatcher::interrupt_cancelled`): the companion was that item, it holds no
//! header, the turn was taken for none about the task, and no Escape was pressed. The window was
//! closed all the same, by what closes the window of a task that is over, so the turn was cut off
//! and not interrupted: Claude writes no record of an interrupted turn, and what the cancel's
//! three rounds of Escape are for (the turn stopped where it is, its tools asked to stop) was
//! skipped. The cases are pastes: the words that resume a paused task, and, since a window is
//! handed its brief once it is up as any later message (T-432, T-523), the brief itself, whose
//! image paths are attached like any paste's (the chief's rule, T-681; the probe of 2026-10-22,
//! T-265, found no attachment made of a brief only while it was an argument of the program). The
//! same held of a companion Claude writes in the middle of a turn, the images a tool read.
//!
//! What should be seen: the daemon presses Escape into the turn of the worker whose task was
//! called off, as it does for any other.
//!
//! The case is that shape on the rig: the chief hands the worker a turn of two minutes, pauses
//! it, and resumes it with words that have the path of a picture on a line of their own; the
//! stand-in makes an attachment of it as Claude Code does, and the human cancels the task. The
//! Escape that the pause pressed is not counted. Its control, the same with no picture, passes.

use cf_e2e::files;
use cf_e2e::rig::{Project, Rig, OPEN};
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

/// `cf` as the chief runs it in its window: what it printed, and a `cf` that fails is an
/// error.
fn cf(rig: &Rig, chief: &Value, words: &[&str]) -> cf_e2e::Result<String> {
    let ran = rig.cf_in_window(chief, words)?;
    if ran.code == Some(0) {
        Ok(ran.stdout)
    } else {
        Err(cf_e2e::Error::Daemon(format!(
            "cf {}: {ran}",
            words.join(" ")
        )))
    }
}

#[test]
fn a_task_cancelled_while_its_window_works_on_words_with_no_image_has_escape_pressed_into_its_turn(
) -> Outcome {
    cancelled_while_at_work(false)
}

#[test]
fn a_task_cancelled_after_images_were_attached_to_the_words_that_resumed_it_has_escape_pressed_into_its_turn(
) -> Outcome {
    cancelled_while_at_work(true)
}

// A task's `\n` is a line break, which a Windows path (`C:\Users\ngvoi\…`) has in it.
#[cfg(unix)]
#[test]
fn the_brief_a_window_is_handed_once_up_is_a_paste_whose_picture_is_attached_and_its_answer_is_the_result(
) -> Outcome {
    // T-681: the brief has the picture's path on a line of its own; the stand-in makes an
    // attachment of it as Claude Code does of any paste, and its companion record is the
    // harness's own: the reply after it is T-1's result.
    briefed_with(
        true,
        "Reply with exactly: DONE",
        |rig, project, session, path| {
            rig.wait_for("T-1 to be done", secs(60), || {
                Ok(project.task(1)?["state"] == "done")
            })?;
            let transcript = rig.transcript(session);
            assert!(
                transcript.contains(&format!("[Image: source: {path}]")),
                "no attachment was made of the picture the brief names: {transcript}"
            );
            assert_eq!(results(project)?, ["DONE"]);
            Ok(())
        },
    )
}

#[cfg(unix)]
#[test]
fn a_path_in_a_brief_to_no_picture_that_exists_stays_text() -> Outcome {
    // The control: the brief names a picture that is not there, and the window keeps its
    // words as they were, the path in them, with no placeholder or companion.
    briefed_with(
        false,
        "Reply with exactly: DONE",
        |rig, project, session, path| {
            rig.wait_for("T-1 to be done", secs(60), || {
                Ok(project.task(1)?["state"] == "done")
            })?;
            let transcript = rig.transcript(session);
            assert!(transcript.contains(path), "{transcript}");
            assert!(
                !transcript.contains("[Image") && !transcript.contains("turnCompanion"),
                "an attachment was made of a path to no picture: {transcript}"
            );
            assert_eq!(results(project)?, ["DONE"]);
            Ok(())
        },
    )
}

#[cfg(unix)]
#[test]
fn a_task_cancelled_while_its_window_works_on_a_brief_whose_picture_was_attached_has_escape_pressed_into_its_turn(
) -> Outcome {
    briefed_with(
        true,
        "SLEEP 120 Reply with exactly: DONE",
        |rig, project, session, _| {
            rig.wait_for(
                "the worker's record to show the picture attached",
                secs(30),
                || Ok(rig.transcript(session).contains("[Image: source: ")),
            )?;
            rig.wait_for("T-1 to be at work", secs(60), || {
                Ok(project.task(1)?["state"] == "working")
            })?;
            let window = workers_windows(rig, project)
                .into_iter()
                .next()
                .ok_or("the worker has no window")?;
            let id = window["id"].as_str().unwrap_or_default().to_owned();
            let pressed = rig.keys_pressed(&id, ESCAPE);
            let cancelled =
                rig.page("task.cancel", json!({ "project": project.id(), "task": 1 }))?;
            assert_eq!(cancelled["ok"], true, "{cancelled}");
            rig.wait_for("the worker's window to be stopped", secs(75), || {
                let ended = rig.exits().iter().any(|exit| {
                    exit["id"] == window["id"] && exit["generation"] == window["generation"]
                });
                Ok(ended || rig.keys_pressed(&id, ESCAPE) > pressed)
            })?;
            assert!(
                rig.keys_pressed(&id, ESCAPE) > pressed,
                "the worker's window was ended with no Escape pressed into its turn"
            );
            Ok(())
        },
    )
}

/// The bodies of T-1's results, oldest first.
fn results(project: &Project) -> cf_e2e::Result<Vec<String>> {
    Ok(project.task(1)?["messages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|message| message["kind"] == "result")
        .filter_map(|message| message["body"].as_str().map(str::to_owned))
        .collect())
}

/// A worker given T-1, whose brief is `words` and then, on a line of its own, the path of a
/// picture, written where `exists`; once its brief is in its record, `then` is given the rig,
/// the project, the worker's conversation and the path, and the rig is closed after.
fn briefed_with(
    exists: bool,
    words: &str,
    then: impl FnOnce(&Rig, &Project, &str, &str) -> Outcome,
) -> Outcome {
    let rig = Rig::start(config())?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    wait_for_the_chief(&rig, &project)?;
    let picture = rig.workspace().join("reference.png");
    if exists {
        files::write(&picture, cf_e2e::png::SMALL)?;
    }
    rig.tell(
        project.id(),
        &format!(
            "DISPATCH --tier {} {words}\\n{}",
            tier(&added),
            picture.display()
        ),
    )?;
    rig.wait_for("the worker's window", secs(60), || {
        Ok(!workers_windows(&rig, &project).is_empty())
    })?;
    let window = workers_windows(&rig, &project)
        .into_iter()
        .next()
        .ok_or("the worker has no window")?;
    let session = after(&window, "--session-id")
        .ok_or("the worker's window has no session")?
        .to_owned();
    let path = picture.display().to_string();
    rig.wait_for("the brief to be in the worker's record", secs(60), || {
        let transcript = rig.transcript(&session);
        Ok(transcript.contains("T-1 · task from @chief"))
    })?;
    then(&rig, &project, &session, &path)?;
    rig.close()?;
    Ok(())
}

/// A worker is given a turn of two minutes and the chief pauses it, then resumes it with
/// words that are another such turn, a picture's path on a line of their own where
/// `with_picture`, and the human cancels its task: Escape must be pressed into the turn. (Without
/// the picture it is: the control that holds the case to the companion record.)
fn cancelled_while_at_work(with_picture: bool) -> Outcome {
    let rig = Rig::start(config())?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    wait_for_the_chief(&rig, &project)?;
    let picture = rig.workspace().join("reference.png");
    files::write(&picture, cf_e2e::png::SMALL)?;

    rig.tell(
        project.id(),
        &format!(
            "DISPATCH --tier {} SLEEP 120 Reply with exactly: DONE",
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
    let id = window["id"].as_str().unwrap_or_default().to_owned();

    // The chief pauses T-1 and resumes it with words that are a turn of two minutes again,
    // the picture's path after them on a line of its own.
    let chief = rig.open_frame(&format!("p{}-chief", project.id()), OPEN)?;
    cf(&rig, &chief, &["task", "pause", "T-1"])?;
    rig.wait_for("T-1 to be paused", secs(60), || {
        Ok(project.task(1)?["state"] == "paused")
    })?;
    let path = if with_picture {
        format!("\n{}", picture.display())
    } else {
        String::new()
    };
    let words = format!("\nSLEEP 120 Reply with exactly: DONE{path}");
    cf(&rig, &chief, &["task", "resume", "T-1", &words])?;
    rig.wait_for("T-1 to be at work again", secs(60), || {
        Ok(project.task(1)?["state"] == "working")
    })?;
    if with_picture {
        rig.wait_for(
            "the worker's record to show the picture attached",
            secs(30),
            || Ok(rig.transcript(&session).contains("[Image: source: ")),
        )?;
    }

    // The human calls it off: the worker's window is stopped, with Escape pressed into its turn
    // first. (Under load the daemon's close may come before the stand-in has written a record of
    // the interrupt, so the keys the daemon pressed are what is read, those since the pause.)
    // The turn runs two minutes, so the end of the window is not the end of the turn.
    let pressed = rig.keys_pressed(&id, ESCAPE);
    let cancelled = rig.page("task.cancel", json!({ "project": project.id(), "task": 1 }))?;
    assert_eq!(cancelled["ok"], true, "{cancelled}");
    rig.wait_for("the worker's window to be stopped", secs(75), || {
        let ended = rig
            .exits()
            .iter()
            .any(|exit| exit["id"] == window["id"] && exit["generation"] == window["generation"]);
        Ok(ended || rig.keys_pressed(&id, ESCAPE) > pressed)
    })?;
    assert!(
        rig.keys_pressed(&id, ESCAPE) > pressed,
        "the worker's window was ended with no Escape pressed into its turn"
    );
    rig.close()?;
    Ok(())
}

//! Another board of the owner's, 2026-11-02, ConsensFlow 3.0.7 (its harness and system not known;
//! the board had Windows work, and Claude Code and Devin workers): **a follow-up showed "working"
//! for an hour while its window never began it, and nothing told anyone.**
//!
//! What the owner saw: T-9 was added `--after T-8` for a session whose window ConsensFlow had
//! closed and opened again. Once T-8 was accepted, T-9 went from queued to working, and an hour
//! later `cf task get T-9 --transcript` showed one item, the brief as it was sent to the window,
//! and nothing after it. The worker's terminal showed no working marker. The chief could not
//! deliver it again nor nudge the window; the only way out was to cancel T-9 and give the work to
//! a fresh window, which lost the window's context. The report is board task T-1130.
//!
//! Why: a task is working once its window's record shows its brief, and every wait of the
//! delivery's is over then; a window that kept the words and began nothing read as a turn in
//! flight for ever (Claude Code's reader: its status idle, its record ending on the message), and
//! a turn's end had nothing to take and said nothing.
//!
//! What should have happened: the window is pressed Enter once, and when that begins nothing
//! either, T-9 is paused and the chief told once what went in and what the window shows. The
//! chief's `cf task resume` sends its words into the same window, which then works the task.
//!
//! The case is that shape on the rig: the worker answers T-1 and its window closes; the chief
//! accepts it and adds T-2 `--after T-1`, whose window, opened again on T-1's conversation, takes
//! its brief and stays in its prompt's hooks for an hour (the stand-in's `HOOKS`: its record holds
//! the message, its status reads idle, it draws nothing). The daemon's arrival wait is cut to 15 s.

use cf_e2e::rig::{Project, Rig, OPEN};
use cf_e2e::Error;
use serde_json::{json, Value};

use crate::{config, secs, Outcome};

/// `cf` as the chief runs it in its window, which must do what it is asked.
fn cf(rig: &Rig, chief: &Value, words: &[&str]) -> cf_e2e::Result<()> {
    let ran = rig.cf_in_window(chief, words)?;
    if ran.code == Some(0) {
        Ok(())
    } else {
        Err(Error::Daemon(format!("cf {}: {ran}", words.join(" "))))
    }
}

/// The windows the rig's pane host opened for `session`, oldest first.
fn windows_of_session(rig: &Rig, project: &Project, session: &str) -> Vec<Value> {
    let id = format!("p{}-{session}", project.id());
    rig.open_frames()
        .into_iter()
        .filter(|frame| frame["id"] == id.as_str())
        .collect()
}

/// The notes to the chief that say T-2 is paused.
fn paused_notes(project: &Project) -> cf_e2e::Result<Vec<String>> {
    Ok(project
        .inbox("chief")?
        .into_iter()
        .filter_map(|message| message["body"].as_str().map(str::to_owned))
        .filter(|body| body.starts_with("T-2 is paused"))
        .collect())
}

#[test]
fn a_follow_up_whose_window_never_began_its_brief_is_paused_its_chief_told_and_its_resume_goes_into_the_same_window(
) -> Outcome {
    let rig = Rig::start(config().daemon_var("CONSENSFLOW_ARRIVAL_TIMEOUT_MS", "15000"))?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    let tier = added["member"]["tier"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    let chief = rig.open_frame(&format!("p{}-chief", project.id()), OPEN)?;
    cf(
        &rig,
        &chief,
        &["task", "add", "--tier", &tier, "Reply with exactly: ONE"],
    )?;
    rig.wait_for("T-1 to be done", secs(180), || {
        Ok(project.task(1)?["state"] == "done")
    })?;
    let session = project.task(1)?["assignee"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    let first = windows_of_session(&rig, &project, &session)
        .into_iter()
        .next()
        .ok_or("T-1's window was never opened")?;
    rig.wait_for("T-1's window to close", secs(60), || {
        Ok(rig
            .exits()
            .iter()
            .any(|exit| exit["id"] == first["id"] && exit["generation"] == first["generation"]))
    })?;
    cf(
        &rig,
        &chief,
        &[
            "task",
            "add",
            "--after",
            "T-1",
            "HOOKS 3600 Reply with exactly: TWO",
        ],
    )?;
    cf(&rig, &chief, &["task", "accept", "T-1"])?;
    rig.wait_for("T-2 to be working", secs(180), || {
        Ok(project.task(2)?["state"] == "working")
    })?;
    let windows = windows_of_session(&rig, &project, &session).len();
    assert_eq!(windows, 2, "T-2's window, opened again on the conversation");

    // Its record shows the brief, and nothing begins: the task leaves `working`.
    rig.wait_for("T-2 to be paused", secs(150), || {
        Ok(project.task(2)?["state"] == "paused")
    })?;
    let notes = paused_notes(&project)?;
    assert_eq!(notes.len(), 1, "{notes:#?}");
    assert!(
        notes[0].contains("began no turn") && notes[0].contains("Enter was pressed once more"),
        "{}",
        notes[0]
    );

    // The way on: the chief's resume goes into the same window, which works the task.
    cf(
        &rig,
        &chief,
        &["task", "resume", "T-2", "Reply with exactly: TWO"],
    )?;
    rig.wait_for("T-2 to be done", secs(180), || {
        Ok(project.task(2)?["state"] == "done")
    })?;
    assert_eq!(
        windows_of_session(&rig, &project, &session).len(),
        windows,
        "no window opened for the resume"
    );
    assert_eq!(paused_notes(&project)?.len(), 1, "told once");
    rig.close()?;
    Ok(())
}

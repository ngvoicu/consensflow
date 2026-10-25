//! Board friction this project met on 2026-10-25, the live board moved from 3.0.5 to
//! 3.0.6-alpha.2 (board task T-541): **after the app's restart, no `cf task add --after`
//! follow-up went on in its window's conversation.**
//!
//! What the chief saw: T-530, `--after T-317`, a merge round for a branch the window that
//! did T-317 had built over three rounds, went to that session in a new conversation, with
//! the note that its window "closed with no proof its program was gone" (m-3891). The
//! window had closed with its result before the restart, and the pane host had proved its
//! program gone; the daemon kept that word in memory alone, and the restart lost it. Every
//! follow-up added before the restart went the same way, to a worker with no memory of
//! its branch. The report is board task T-541 (m-3906).
//!
//! What should have happened: the follow-up goes on in the conversation of the window that
//! did T-1, as it does without a restart. The ledger keeps the host's proof on that
//! conversation (`program_gone`, migration 0023).
//!
//! The case is that shape on the rig: the worker's window closes with T-1's result, the
//! chief accepts it, the app quits (the daemon killed, then the pane host) and starts
//! again on the same home, the owner resumes the project, and the chief adds the follow-up
//! with its own `cf` in its window.

use cf_e2e::rig::{Project, Rig, OPEN};
use cf_e2e::Error;
use serde_json::{json, Value};

use crate::resume_of_a_chief_with_no_turn::{after, resumed_on};
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

/// The notes to the human that say T-2 goes to a new conversation.
fn told_new(project: &Project) -> cf_e2e::Result<Vec<String>> {
    Ok(project
        .inbox("human")?
        .into_iter()
        .filter_map(|message| message["body"].as_str().map(str::to_owned))
        .filter(|body| body.starts_with("T-2 goes to @") && body.contains("new conversation"))
        .collect())
}

#[test]
fn a_follow_up_added_after_the_apps_restart_goes_on_in_the_conversation_of_the_window_that_did_its_task(
) -> Outcome {
    let mut rig = Rig::start(config())?;
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
    let conversation = after(&first, "--session-id")
        .ok_or("T-1's window opened on no session of its own")?
        .to_owned();
    // The window closes with its result, and the pane host tells how it ended.
    rig.wait_for("T-1's window to close", secs(60), || {
        Ok(rig
            .exits()
            .iter()
            .any(|exit| exit["id"] == first["id"] && exit["generation"] == first["generation"]))
    })?;
    cf(&rig, &chief, &["task", "accept", "T-1"])?;

    // The app quits, daemon first, and starts again on the same home; the owner resumes.
    let id = project.id();
    rig.kill_daemon();
    rig.wait_for_daemon_exit(secs(10))?;
    let root = rig.close_keeping_the_root()?;
    let rig = Rig::start(config().existing_root(root))?;
    let resumed = rig.page("project.resume", json!({ "project": id }))?;
    assert_eq!(resumed["ok"], true, "{resumed}");
    let project = Project::new(&rig, id);
    let chief = rig.open_frame(&format!("p{id}-chief"), OPEN)?;
    cf(
        &rig,
        &chief,
        &["task", "add", "--after", "T-1", "Reply with exactly: TWO"],
    )?;
    rig.wait_for("T-2 to be done", secs(180), || {
        Ok(project.task(2)?["state"] == "done")
    })?;
    assert_eq!(
        project.task(2)?["assignee"],
        session.as_str(),
        "the same session"
    );
    let window = windows_of_session(&rig, &project, &session)
        .into_iter()
        .next()
        .ok_or("T-2's window was never opened")?;
    assert_eq!(
        resumed_on(&window),
        Some(conversation.as_str()),
        "T-1's conversation: {}",
        window["argv"]
    );
    assert_eq!(told_new(&project)?, Vec::<String>::new());
    rig.close()?;
    Ok(())
}

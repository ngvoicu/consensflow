//! A window with nothing left to stop was closed as one that ignored its stop, and the
//! close ended what it had left running in the background.
//!
//! Seen on this project's own board on 2026-10-25, ConsensFlow 3.0.6-alpha.2, Claude
//! Code 2.1.310 (T-551). At ~11:52 the account's weekly limit refused seven Claude
//! windows; ConsensFlow held each task, and for T-502 pressed Escape three times and then
//! closed its window, "it ignored the interrupt" (m-3943), which ended the plant run and
//! the build it had left in the background (m-3945); the same close was told twice
//! (m-3943, m-3944). Its turn had ended on the refusal: nothing was left to stop. At
//! ~21:05 a `cf tell` of T-604 pressed Escape into a turn whose three background agents
//! ran; Claude's record shows the interrupt worked, and ConsensFlow closed the window
//! all the same (m-4188), ending the agents. Claude says busy while agents run, and an
//! interrupted or refused turn writes no count of them, so the look did not read the
//! window at rest.
//!
//! What the owner should have seen: no key into a turn the refusal ended, one into the
//! turn the tell stopped; the window open with its work; nobody told it did not stop;
//! and the task going on in that window once its words go in.
//!
//! The stand-in does what Claude Code does: a job of the window's tree (`JOB`), agents
//! that keep its status busy (`AGENTS --then`), and a turn the account's limit refuses
//! after its work (`CF_TEST_QUOTA_SPENT`) or a tell's Escape stops (`SLEEP`). T-502's
//! own window read at work for another reason (its record held a message a process
//! closed before the daemon's restart had queued, which `wt/exit-proof-kept` and
//! `wt/stuck-working-2` fix); the agents are this case's way to the same look.

use std::path::PathBuf;
use std::thread::sleep;

use cf_e2e::process::is_alive;
use cf_e2e::rig::{write_staff, Project, Rig, OPEN};
use cf_e2e::{files, Error};
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::{config, secs, Outcome};

/// The account the workers share: spent while its file is there (`CF_TEST_QUOTA_FILE`).
struct Account {
    folder: TempDir,
}

impl Account {
    fn new() -> cf_e2e::Result<Self> {
        let folder = tempfile::Builder::new()
            .prefix("cf-account-")
            .tempdir()
            .map_err(|source| Error::File {
                action: "make a folder in",
                path: std::env::temp_dir(),
                source,
            })?;
        Ok(Self { folder })
    }

    fn file(&self) -> PathBuf {
        self.folder.path().join("spent")
    }
}

/// The notes of the project that say a window did not stop, or that a close ended what
/// it left running, the human's and the chief's.
fn closes_told(rig: &Rig, project: &Project) -> cf_e2e::Result<Vec<String>> {
    let mut messages: Vec<Value> = rig.page("inbox.get", json!({ "project": project.id() }))?
        ["messages"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    messages.extend(project.inbox("chief")?);
    Ok(messages
        .into_iter()
        .filter(|message| message["kind"] == "note")
        .filter_map(|message| message["body"].as_str().map(str::to_owned))
        .filter(|body| body.contains("did not stop") || body.contains("its close ended"))
        .collect())
}

/// The pane of the lane of `session`, as the board shows it.
fn pane_of(project: &Project, session: &str) -> cf_e2e::Result<Value> {
    Ok(project.lane(session)?["pane"].clone())
}

/// The Escapes the daemon pressed into `pane`.
fn escapes(rig: &Rig, pane: &Value) -> usize {
    rig.presses(pane["id"].as_str().unwrap_or_default(), &[27])
        .len()
}

/// Waits for task `number` to be at work, and gives its assignee.
fn at_work(rig: &Rig, project: &Project, number: i64) -> cf_e2e::Result<String> {
    rig.wait_for(&format!("T-{number} to be at work"), secs(60), || {
        Ok(project.task(number)?["state"] == "working")
    })?;
    Ok(project.task(number)?["assignee"]
        .as_str()
        .unwrap_or_default()
        .to_owned())
}

#[test]
fn an_accounts_limit_holds_its_windows_and_closes_none_that_left_work_running() -> Outcome {
    let account = Account::new()?;
    let rig = Rig::start(
        config()
            .var("CF_TEST_QUOTA_SPENT", "worker,worker2")
            .var("CF_TEST_QUOTA_FILE", account.file().to_string_lossy()),
    )?;
    write_staff(&rig)?;
    let project = Project::open_with_staff(
        &rig,
        "chief",
        None,
        &[("worker", "worker"), ("worker2", "worker")],
    )?;
    let tier = project
        .tiers()?
        .into_values()
        .next()
        .ok_or("the staff has no tier")?;
    // T-502's shape: a job and agents in the background, and the turn works on.
    rig.tell(
        project.id(),
        &format!(
            "DISPATCH --tier {tier} JOB 600 AGENTS --then 2 60 SLEEP 30 Reply with exactly: HELD_OK"
        ),
    )?;
    let held = at_work(&rig, &project, 1)?;
    rig.wait_for(
        "its job to run",
        secs(60),
        || Ok(rig.jobs("job").len() == 1),
    )?;
    let job = rig
        .jobs("job")
        .into_iter()
        .next()
        .ok_or_else(|| Error::Daemon("the job is gone".to_owned()))?;
    rig.tell(
        project.id(),
        &format!("DISPATCH --tier {tier} SLEEP 20 Reply with exactly: PLAIN_OK"),
    )?;
    let plain = at_work(&rig, &project, 2)?;
    let pane = pane_of(&project, &held)?;

    // The account's limit is spent: each turn is refused as it asks again.
    files::write(&account.file(), "")?;
    rig.wait_for("both tasks to be held", secs(90), || {
        Ok(project.task(1)?["state"] == "paused" && project.task(2)?["state"] == "paused")
    })?;
    // Every round a stop could have, and more.
    sleep(secs(15));
    assert_eq!(
        escapes(&rig, &pane),
        0,
        "no key into a turn the refusal ended"
    );
    assert_eq!(
        pane_of(&project, &held)?,
        pane,
        "the window is open, as it was"
    );
    assert!(
        is_alive(job.pid()) && !job.released(),
        "its job runs on, never asked to end"
    );
    assert_eq!(closes_told(&rig, &project)?, Vec::<String>::new());

    // Quota is back: each task goes on in its own window.
    let _ = std::fs::remove_file(account.file());
    for member in ["worker", "worker2"] {
        let back = rig.page(
            "member.back",
            json!({ "project": project.id(), "participant": member }),
        )?;
        assert_eq!(back["ok"], true, "{back}");
    }
    rig.wait_for("both tasks to be done", secs(120), || {
        Ok(project.task(1)?["state"] == "done" && project.task(2)?["state"] == "done")
    })?;
    for session in [&held, &plain] {
        let launches = rig
            .open_frames()
            .into_iter()
            .chain(rig.exits())
            .filter(|frame| {
                frame["id"]
                    .as_str()
                    .is_some_and(|id| id.contains(session.as_str()))
            })
            .filter_map(|frame| frame["generation"].as_i64())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            launches.len(),
            1,
            "{session}: one window, never opened again"
        );
    }
    assert_eq!(closes_told(&rig, &project)?, Vec::<String>::new());
    rig.close()?;
    Ok(())
}

#[test]
fn a_tell_that_stopped_a_turn_whose_agents_run_keeps_the_window_and_its_work() -> Outcome {
    let rig = Rig::start(config())?;
    write_staff(&rig)?;
    let project = Project::open_with_staff(&rig, "chief", None, &[("worker", "worker")])?;
    let tier = project
        .tiers()?
        .remove("worker")
        .ok_or("the worker has no tier")?;
    // T-604's shape: agents in the background, and a long turn at work.
    rig.tell(
        project.id(),
        &format!(
            "DISPATCH --tier {tier} JOB 600 AGENTS --then 2 30 SLEEP 300 Reply with exactly: NEVER_OK"
        ),
    )?;
    let session = at_work(&rig, &project, 1)?;
    rig.wait_for(
        "its job to run",
        secs(60),
        || Ok(rig.jobs("job").len() == 1),
    )?;
    let job = rig
        .jobs("job")
        .into_iter()
        .next()
        .ok_or_else(|| Error::Daemon("the job is gone".to_owned()))?;
    let pane = pane_of(&project, &session)?;
    let chief = rig.open_frame(&format!("p{}-chief", project.id()), OPEN)?;
    let told = rig.cf_in_window(&chief, ["tell", "T-1", "Drop the cap. REPLY dropped"])?;
    if told.code != Some(0) {
        return Err(Error::Daemon(format!("cf tell: {told}")).into());
    }
    rig.wait_for("the turn to be stopped", secs(30), || {
        Ok(escapes(&rig, &pane) >= 1)
    })?;
    sleep(secs(15));
    assert_eq!(
        escapes(&rig, &pane),
        1,
        "one Escape, which stopped the turn"
    );
    assert_eq!(pane_of(&project, &session)?, pane, "the window is open");
    assert!(is_alive(job.pid()) && !job.released(), "its job runs on");
    assert_eq!(closes_told(&rig, &project)?, Vec::<String>::new());

    // The agents end; the window rests, and the tell's words go into it.
    rig.wait_for("the worker to answer the tell", secs(90), || {
        Ok(project
            .inbox("chief")?
            .iter()
            .any(|message| message["kind"] == "answer"))
    })?;
    assert_eq!(pane_of(&project, &session)?, pane, "the same window");
    assert_eq!(escapes(&rig, &pane), 1);
    assert_eq!(closes_told(&rig, &project)?, Vec::<String>::new());
    rig.close()?;
    Ok(())
}

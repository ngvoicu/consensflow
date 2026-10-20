//! A Codex chief opened again after the app's restart read "starting" for good.
//!
//! Seen on 2026-10-19 by zeewin's 2-hour live long run, on 3.0.5's main (`9c460cf8`),
//! Windows 11, a Codex 0.160.0 chief: its turn ended at 21:26:58.924Z, the run's restart
//! quit the app a second later and started it again, and the owner resumed the project.
//! ConsensFlow opened the chief's window again on its thread at 21:27:11Z, at rest, with
//! nothing to begin a turn. Codex wrote nothing to the thread from then on, the window's
//! broker never named the thread, and the chief's lane read "starting" for the 78
//! minutes left: a worker's result for it (T-21's, 21:27:33Z) never reached it, and the
//! project stalled with 12 of 32 tasks open. The report is
//! `issues/a-chief-stalls-after-a-compact-or-a-restart.md` in consensflow-sme.
//!
//! What the owner should have seen: a chief whose window does not come up is not left
//! starting. Its window is closed within the time a window has to come up, the human is
//! told once that the chief could not start, the result waits for it, and the chief is
//! opened again with the result once it can come up.
//!
//! Why the real Codex never named its thread is not known: the run's home, with its
//! ledger and screens, went with the run. The stand-in Codex's server leaves a resume
//! unanswered while a file is there (`CF_TEST_CODEX_RESUME_HANGS`), which the broker
//! reads as it reads a resume Codex's TUI never asked for: the window shows no thread.
//! The case is the field's: a Codex chief whose turn has ended, a worker at work on the
//! task it gave, the app quit (the daemon killed, then the pane host) and started again
//! on the same home, and the owner's Resume; the worker finishes, and its result is for
//! the chief. A launch here has 10 s to come up (`CONSENSFLOW_LAUNCH_TIMEOUT_MS`), not
//! the minute it has in the app.

use std::path::Path;

use cf_e2e::rig::{Config, Project, Rig, OPEN};
use cf_e2e::{files, Error};
use serde_json::{json, Value};

use crate::resume_of_a_chief_with_no_turn::{resumed_on, windows_of};
use crate::{config_with_codex, secs, Outcome};

/// What the note to the human begins with when the chief's window does not come up.
const NOT_UP: &str =
    "The chief could not start: the window never came up: the Codex broker never named the thread";

/// The rig, its launches given 10 s, its stand-in Codex leaving a resume unanswered
/// while `hangs` is there.
fn config(hangs: &Path) -> Config {
    config_with_codex()
        .var("CONSENSFLOW_LAUNCH_TIMEOUT_MS", "10000")
        .var("CF_TEST_CODEX_RESUME_HANGS", hangs.to_string_lossy())
}

/// `cf` in the window `frame`, which must do what it is asked.
fn cf(rig: &Rig, frame: &Value, words: &[&str]) -> cf_e2e::Result<()> {
    let ran = rig.cf_in_window(frame, words)?;
    if ran.code == Some(0) {
        Ok(())
    } else {
        Err(Error::Daemon(format!("cf {}: {ran}", words.join(" "))))
    }
}

/// The notes to the human that say the chief could not start.
fn not_up(project: &Project) -> cf_e2e::Result<Vec<String>> {
    Ok(project
        .inbox("human")?
        .into_iter()
        .filter_map(|message| message["body"].as_str().map(str::to_owned))
        .filter(|body| body.starts_with(NOT_UP))
        .collect())
}

/// The result for the task `number` in the chief's inbox.
fn result_for(project: &Project, number: i64) -> cf_e2e::Result<Option<Value>> {
    Ok(project
        .inbox("chief")?
        .into_iter()
        .find(|message| message["kind"] == "result" && message["taskNumber"] == number))
}

#[test]
fn a_codex_chief_whose_window_does_not_come_up_after_a_restart_is_closed_the_human_told_and_its_result_goes_in_once_it_can(
) -> Outcome {
    let flags = tempfile::tempdir()?;
    let hangs = flags.path().join("resume-hangs");
    let rig = Rig::start(config(&hangs))?;
    let roster = json!({
        "schemaVersion": 1,
        "agents": [
            { "id": "codex-chief", "kind": "codex", "model": "fake" },
            { "id": "worker", "kind": "claude-code", "model": "fake", "workTier": "standard" },
        ],
    });
    files::write(&rig.home().join("agents.json"), format!("{roster}\n"))?;
    let project = Project::open(&rig, "codex-chief", json!({}))?;
    let added = project.add_member("worker")?;
    let tier = added["member"]["tier"]
        .as_str()
        .unwrap_or("standard")
        .to_owned();
    let chief = rig.open_frame(&format!("p{}-chief", project.id()), OPEN)?;

    // The chief has a turn, which gives its thread a record to be resumed from: its
    // worker's first result.
    cf(
        &rig,
        &chief,
        &[
            "task",
            "add",
            "--tier",
            &tier,
            "Reply with exactly: FIRST_OK",
        ],
    )?;
    rig.wait_for(
        "the first result to reach the chief and its turn to end",
        secs(90),
        || {
            Ok(
                result_for(&project, 1)?.is_some_and(|result| result["state"] == "delivered")
                    && project.lane("chief")?["activity"]["state"] == "idle",
            )
        },
    )?;
    // Its turn gave the worker another task, at work when the app quits; the chief is at rest.
    cf(
        &rig,
        &chief,
        &[
            "task",
            "add",
            "--tier",
            &tier,
            "SLEEP 600 Reply with exactly: WORKED",
        ],
    )?;
    rig.wait_for(
        "the worker at work on T-2 and the chief at rest",
        secs(60),
        || {
            Ok(project.task(2)?["state"] == "working"
                && project.lane("worker")?["activity"]["state"] == "working"
                && project.lane("chief")?["activity"]["state"] == "idle")
        },
    )?;

    // The app quits, daemon first, and starts again on the same home; from now on a
    // Codex window opened on its thread never shows it. The owner resumes the project.
    files::write(&hangs, "")?;
    let id = project.id();
    let mut rig = rig;
    rig.kill_daemon();
    rig.wait_for_daemon_exit(secs(10))?;
    let root = rig.close_keeping_the_root()?;
    let rig = Rig::start(config(&hangs).existing_root(root))?;
    let resumed = rig.page("project.resume", json!({ "project": id }))?;
    assert_eq!(resumed["ok"], true, "{resumed}");
    let project = Project::new(&rig, id);

    // The chief's window is opened again on its thread, and given up: the human is told.
    let told = rig.wait_for(
        "the human to be told the chief could not start",
        secs(60),
        || Ok(!not_up(&project)?.is_empty()),
    );
    if let Err(failed) = told {
        let lane = project.lane("chief")?;
        return Err(format!("{failed}; the chief's lane reads {}", lane["activity"]).into());
    }
    let reopened = windows_of(&rig, &project)
        .into_iter()
        .next()
        .ok_or("the chief's window was never opened again")?;
    assert!(
        resumed_on(&reopened).is_some(),
        "opened on its thread: {}",
        reopened["argv"]
    );
    assert!(
        reopened["argv"]
            .as_array()
            .and_then(|argv| argv.last())
            .is_some_and(|last| last == "--dangerously-bypass-approvals-and-sandbox"),
        "at rest: nothing begins a turn: {}",
        reopened["argv"]
    );
    rig.wait_for(
        "the window that never came up to be closed",
        secs(30),
        || {
            Ok(rig.exits().iter().any(|exit| {
                exit["id"] == reopened["id"] && exit["generation"] == reopened["generation"]
            }))
        },
    )?;
    assert_eq!(
        project.board()?["project"]["state"],
        "open",
        "the staff works on"
    );

    // The worker finishes: its result waits for the chief, not spent on windows that
    // never came up. Each launch of the chief begins it as its first message and gives
    // it back when the window is given up.
    rig.wait_for("T-2 to be done", secs(90), || {
        Ok(project.task(2)?["state"] == "done")
    })?;
    let waiting = result_for(&project, 2)?.ok_or("no result for T-2")?;
    assert!(
        matches!(waiting["state"].as_str(), Some("queued" | "delivering")),
        "neither delivered nor failed: {waiting}"
    );

    // Codex's windows come up again: the chief is opened again with the result.
    std::fs::remove_file(&hangs)?;
    rig.wait_for("the result to be delivered to the chief", secs(120), || {
        Ok(result_for(&project, 2)?.is_some_and(|result| result["state"] == "delivered"))
    })?;
    rig.wait_for("the chief to be idle", secs(60), || {
        Ok(project.lane("chief")?["activity"]["state"] == "idle")
    })?;
    assert_eq!(not_up(&project)?.len(), 1, "told once");
    rig.close()?;
    Ok(())
}

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
//! opened again with the result once it can come up, which it is given once. And a
//! window that is slow to come up is not given up at every try (Astra on round 1 of the
//! fix): each window after one given up has longer, so one that needs more than the
//! first time comes up in the end.
//!
//! Why the real Codex never named its thread is not known: the run's home, with its
//! ledger and screens, went with the run. The stand-in Codex's server leaves a resume
//! unanswered while a file is there (`CF_TEST_CODEX_RESUME_HANGS`), which the broker
//! reads as it reads a resume Codex's TUI never asked for: the window shows no thread;
//! or answers it late (`CF_TEST_CODEX_RESUME_MS`). The cases are the field's: a Codex
//! chief whose turn has ended, a worker at work on the task it gave, the app quit (the
//! daemon killed, then the pane host) and started again on the same home, and the
//! owner's Resume; the worker finishes, and its result is for the chief. A launch here
//! has 10 s to come up (`CONSENSFLOW_LAUNCH_TIMEOUT_MS`), not the minute it has in the
//! app.

use std::path::{Path, PathBuf};

use cf_e2e::rig::{Config, Project, Rig, OPEN};
use cf_e2e::{files, rollout, Error};
use serde_json::{json, Value};

use crate::resume_of_a_chief_with_no_turn::{resumed_on, windows_of};
use crate::{config_with_codex, secs, Outcome};

/// What the note to the human begins with when the chief's window does not come up.
const NOT_UP: &str =
    "The chief could not start: the window never came up: the Codex broker never named the thread";

/// The rig, its launches given 10 s.
fn config() -> Config {
    config_with_codex().var("CONSENSFLOW_LAUNCH_TIMEOUT_MS", "10000")
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

/// How many times the chief's thread was given the message `id`: the user's items of
/// the rollouts in the rig's Codex home that are its paste.
fn received(rig: &Rig, id: i64) -> cf_e2e::Result<usize> {
    let home = PathBuf::from(rig.var("CODEX_HOME").unwrap_or_default());
    let marker = format!("[ConsensFlow m-{id} ");
    let mut times = 0;
    for path in rollout::rollouts(&home) {
        times += rollout::said_by(&rollout::read(&path)?, "user")
            .iter()
            .filter(|said| said.starts_with(&marker))
            .count();
    }
    Ok(times)
}

/// The field's shape up to the owner's Resume: on a rig made with `config`, a Codex
/// chief has its worker's first result, which gives its thread a record to be resumed
/// from, and gives the worker T-2, at work when the app quits (where `worker_at_work`
/// says), the chief at rest. The app quits, daemon first, and starts again on the same
/// home, made with `after`; the owner resumes the project. The rig, and the project's id.
fn chief_at_rest_when_the_app_quits(
    config: Config,
    after: Config,
    worker_at_work: bool,
) -> Result<(Rig, i64), Box<dyn std::error::Error>> {
    let mut rig = Rig::start(config)?;
    let roster = json!({
        "schemaVersion": 1,
        "agents": [
            { "id": "codex-chief", "kind": "codex", "model": "fake" },
            { "id": "worker", "kind": "claude-code", "model": "fake", "workTier": "standard" },
        ],
    });
    files::write(&rig.home().join("agents.json"), format!("{roster}\n"))?;
    // The worker is the staff from the start: a member added later changes the chief's
    // role, which its resumed thread is then told by a note that waits for it.
    let project = Project::open_with_staff(&rig, "codex-chief", None, &[("worker", "worker")])?;
    let tier = project.tiers()?.get("worker").cloned().unwrap_or_default();
    let chief = rig.open_frame(&format!("p{}-chief", project.id()), OPEN)?;
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
    if worker_at_work {
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
    }
    let id = project.id();
    rig.kill_daemon();
    rig.wait_for_daemon_exit(secs(10))?;
    let root = rig.close_keeping_the_root()?;
    let rig = Rig::start(after.existing_root(root))?;
    let resumed = rig.page("project.resume", json!({ "project": id }))?;
    assert_eq!(resumed["ok"], true, "{resumed}");
    Ok((rig, id))
}

/// The file that, while it is there, has the stand-in Codex leave a resume unanswered.
fn hangs_in(flags: &Path) -> PathBuf {
    flags.join("resume-hangs")
}

#[test]
fn a_codex_chief_whose_window_does_not_come_up_after_a_restart_is_closed_the_human_told_and_its_result_goes_in_once_it_can(
) -> Outcome {
    let flags = tempfile::tempdir()?;
    let hangs = hangs_in(flags.path());
    // From the restart on, a Codex window opened on its thread never shows it.
    let hanging = || config().var("CF_TEST_CODEX_RESUME_HANGS", hangs.to_string_lossy());
    files::write(&hangs, "")?;
    let (rig, id) = chief_at_rest_when_the_app_quits(hanging(), hanging(), true)?;
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

    // Codex's windows come up again: the chief is opened again with the result, once.
    std::fs::remove_file(&hangs)?;
    rig.wait_for("the result to be delivered to the chief", secs(180), || {
        Ok(result_for(&project, 2)?.is_some_and(|result| result["state"] == "delivered"))
    })?;
    rig.wait_for("the chief to be idle", secs(60), || {
        Ok(project.lane("chief")?["activity"]["state"] == "idle")
    })?;
    let result = waiting["id"].as_i64().ok_or("the result has no id")?;
    assert_eq!(received(&rig, result)?, 1, "its thread was given it once");
    assert_eq!(not_up(&project)?.len(), 1, "told once");
    rig.close()?;
    Ok(())
}

/// Astra on round 1 of the fix: a resume that takes longer than the first time a window
/// has, and less than ten times it, was given up at every try, and what waited for the
/// chief never reached it. Here each resume takes 12 s, a window has 10 s the first time:
/// the first is given up and the human told, and the next, given 20 s, comes up with the
/// result, which its thread is given once (a runner slow to start Codex's two programs
/// may take the try after, given 40 s).
#[test]
fn a_codex_chief_whose_resume_takes_longer_than_the_first_time_a_window_has_comes_up_on_a_later_try_and_is_given_its_result_once(
) -> Outcome {
    let slow = config().var("CF_TEST_CODEX_RESUME_MS", "12000");
    let (rig, id) = chief_at_rest_when_the_app_quits(config(), slow, true)?;
    let project = Project::new(&rig, id);

    rig.wait_for("T-2 to be done", secs(90), || {
        Ok(project.task(2)?["state"] == "done")
    })?;
    let delivered = rig.wait_for("the result to be delivered to the chief", secs(120), || {
        Ok(result_for(&project, 2)?.is_some_and(|result| result["state"] == "delivered"))
    });
    if let Err(failed) = delivered {
        let windows = windows_of(&rig, &project).len();
        return Err(format!(
            "{failed}: the chief's window was opened {windows} times, and its lane reads {}",
            project.lane("chief")?["activity"]
        )
        .into());
    }
    let result = result_for(&project, 2)?
        .and_then(|result| result["id"].as_i64())
        .ok_or("no result for T-2")?;
    assert_eq!(received(&rig, result)?, 1, "its thread was given it once");
    let windows = windows_of(&rig, &project).len();
    assert!(
        (2..=3).contains(&windows),
        "the first window given up and a later one up, not one given up at every try: {windows}"
    );
    assert_eq!(not_up(&project)?.len(), 1, "told once");
    rig.close()?;
    Ok(())
}

/// Sol and Astra on round 2 of the fix: a chief whose window was given up with nothing
/// waiting for it stayed closed, as only a message for it opened it again, and a window
/// that began a turn as it was closed left nothing queued. The chief is opened again at
/// the backoff whatever waits for it. Here nothing does: the app quits with the chief at
/// rest and no worker at work, the chief's first window after the restart never shows
/// its thread, and once Codex's windows come up again the next one does, and reads at
/// rest.
#[test]
fn a_codex_chief_given_up_with_nothing_waiting_for_it_opens_again_at_the_backoff() -> Outcome {
    let flags = tempfile::tempdir()?;
    let hangs = hangs_in(flags.path());
    let hanging = || config().var("CF_TEST_CODEX_RESUME_HANGS", hangs.to_string_lossy());
    files::write(&hangs, "")?;
    let (rig, id) = chief_at_rest_when_the_app_quits(hanging(), hanging(), false)?;
    let project = Project::new(&rig, id);
    rig.wait_for(
        "the human to be told the chief could not start",
        secs(60),
        || Ok(!not_up(&project)?.is_empty()),
    )?;
    let waiting: Vec<Value> = project
        .inbox("chief")?
        .into_iter()
        .filter(|message| matches!(message["state"].as_str(), Some("queued" | "delivering")))
        .collect();
    assert!(
        waiting.is_empty(),
        "nothing waits for the chief: {waiting:?}"
    );

    std::fs::remove_file(&hangs)?;
    let reopened = rig.wait_for(
        "the chief to be opened again and read idle",
        secs(120),
        || {
            Ok(windows_of(&rig, &project).len() >= 2
                && project.lane("chief")?["activity"]["state"] == "idle")
        },
    );
    if let Err(failed) = reopened {
        return Err(format!(
            "{failed}: the chief's window was opened {} times, and its lane reads {}",
            windows_of(&rig, &project).len(),
            project.lane("chief")?["activity"]
        )
        .into());
    }
    assert_eq!(not_up(&project)?.len(), 1, "told once");
    rig.close()?;
    Ok(())
}

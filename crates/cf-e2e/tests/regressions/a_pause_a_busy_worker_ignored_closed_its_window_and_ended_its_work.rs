//! A pause a busy worker ignored closed its window, and the close ended its work.
//!
//! A field report of Gabriel's, from a board on Windows, version unknown (T-1117): the
//! chief paused T-3 while its worker ran its database tests against a server in WSL
//! (`bash` → `wsl.exe -d Ubuntu … cf-t3-check.sh`). "T-3's window did not stop: it
//! ignored the interrupt, so ConsensFlow closed it" (m-374), and "T-3's window closed with
//! background work running, which its close ended" (m-375): the tests ended half-way, and
//! the chief could not safely redirect a busy worker. Seen three times (T-3 twice, T-8).
//!
//! What the chief should have seen: the window kept open, the task paused, its work
//! running on, and a note to the chief and the human of what runs and that only deleting
//! the session ends it sooner; and the human's delete-session ending it.
//!
//! The stand-in worker leaves a job in its window's tree (`JOB`) and its turn ignores
//! every key (`HANG`), as a turn on a tool call that holds it may. No harness of today
//! ignores the interrupt in that shape (T-1117's live runs: Claude Code, Codex and Devin on
//! the Mac and zeewin2, Pi on the Mac, each stopped at the first Escape), so the stand-in is
//! the shape's only red. A turn that ignores every key with nothing of its own running is
//! still closed: `a_pause_a_hung_worker_ignored_stranded_its_task`.

use std::thread::sleep;

use cf_e2e::process::is_alive;
use cf_e2e::rig::{Project, Rig, OPEN};
use cf_e2e::Error;
use serde_json::{json, Value};

use crate::{config, secs, Outcome};

/// The notes that say the worker's window did not stop: the human's, then the chief's.
fn told(rig: &Rig, project: &Project) -> cf_e2e::Result<[Vec<String>; 2]> {
    let human = rig.page("inbox.get", json!({ "project": project.id() }))?["messages"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let said = |messages: Vec<Value>| -> Vec<String> {
        messages
            .into_iter()
            .filter_map(|message| message["body"].as_str().map(str::to_owned))
            .filter(|body| body.contains("did not stop"))
            .collect()
    };
    Ok([said(human), said(project.inbox("chief")?)])
}

/// `cf` with `words`, run in the chief's window as its agent runs it.
fn cf(rig: &Rig, chief: &Value, words: &[&str]) -> cf_e2e::Result<()> {
    let ran = rig.cf_in_window(chief, words)?;
    if ran.code != Some(0) {
        return Err(Error::Daemon(format!("cf {}: {ran}", words.join(" "))));
    }
    Ok(())
}

#[test]
fn a_busy_worker_that_ignores_its_pause_keeps_its_window_and_its_work_until_the_human_ends_its_session(
) -> Outcome {
    let rig = Rig::start(config())?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    let tier = added["member"]["tier"].as_str().unwrap_or_default();
    rig.tell(
        project.id(),
        &format!("DISPATCH --tier {tier} JOB 600 HANG 600 Reply with exactly: NEVER_OK"),
    )?;
    rig.wait_for("the worker's job to run", secs(60), || {
        Ok(project.task(1)?["state"] == "working" && rig.jobs("job").len() == 1)
    })?;
    let job = rig
        .jobs("job")
        .into_iter()
        .next()
        .ok_or_else(|| Error::Daemon("the job is gone".to_owned()))?;
    let handle = project.task(1)?["assignee"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    let pane = project.lane(&handle)?["pane"].clone();
    let chief = rig.open_frame(&format!("p{}-chief", project.id()), OPEN)?;

    // The chief pauses T-1, and the window ignores every key: three rounds, then the look
    // that asks what it runs, then one more look at it ten seconds on.
    cf(&rig, &chief, &["task", "pause", "T-1"])?;
    rig.wait_for("the chief to be told the window is kept", secs(60), || {
        Ok(!told(&rig, &project)?[1].is_empty())
    })?;
    sleep(secs(15));
    assert_eq!(project.task(1)?["state"], "paused");
    assert_eq!(project.lane(&handle)?["pane"], pane, "the window is open, as it was");
    assert!(
        is_alive(job.pid()) && !job.released(),
        "its work runs on, never asked to end"
    );
    let [human, to_chief] = told(&rig, &project)?;
    assert!(
        to_chief.len() == 1
            && to_chief[0].contains("ConsensFlow keeps the window open for it")
            && to_chief[0].contains("only the human can end the work sooner, by deleting its session"),
        "the chief was told: {to_chief:?}"
    );
    assert!(
        human.len() == 1 && human[0].contains(&format!("delete @{handle}'s session")),
        "the human was told: {human:?}"
    );

    // The human deletes the session: that ends the window, and its work with it.
    let ended = rig.page(
        "session.end",
        json!({ "project": project.id(), "handle": handle }),
    )?;
    assert_eq!(ended["ok"], true, "{ended}");
    rig.wait_for("the window's work to end with it", secs(60), || {
        Ok(!is_alive(job.pid()))
    })?;
    Ok(())
}

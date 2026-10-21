//! Claude Code's question about trusting a folder stopped every launch in a new repository.
//!
//! Seen from 2026-10-17 to 10-19 by dip-agentos's chief, on ConsensFlow 3.0.4: after `git
//! init` in the project's folder, every worker's window ConsensFlow opened stopped at
//! Claude Code's "Do you trust this folder?", and T-11, T-13, T-14 and T-15 each failed and
//! needed a manual reopen. The owner hit it again on 2026-10-20 in other folders. The
//! project isn't on this Mac, so its transcripts weren't read. The report is item 2 of
//! `issues/field-reports-2026-10-19-dip-agentos.md` in consensflow-sme, and the decision
//! `decisions/2026-10-20-consensflow-answers-trust-in-the-projects-folder.md`.
//!
//! What the owner should have seen: choosing a folder for a project counts as trusting it.
//! A window that asks of the project's folder is answered yes by ConsensFlow on its screen,
//! as a person would, and the human's notes say so once, however many windows ask; its
//! task is done. A window whose yes would trust a folder outside the project's (the root
//! of the repository the project's folder is in) is never answered by ConsensFlow: the
//! human is told once, nothing fails while the question waits, and once they said yes in
//! a window every task that waited goes on by itself.
//!
//! The stand-in asks the question as Claude Code 2.1.305 does (`fake_agent`'s `trust`):
//! in a git repository whose root it was never told to trust, before anything else, its
//! `❯` on "No, exit", and the yes written down for the repository's root. Each case gives
//! the worker two tasks at once, two windows that ask together, as dip-agentos's did; a
//! chief opened in a new repository asks too, and must take what the human tells it. A
//! launch here has 8 s to show its first message (`CONSENSFLOW_LAUNCH_TIMEOUT_MS`), not
//! the three minutes it has in the app: on main each task fails at that. A window the
//! human said yes in may write its first message only seconds later: that message has
//! its whole time from the yes.

use std::path::Path;
use std::thread::sleep;
use std::time::Duration;

use cf_e2e::files;
use cf_e2e::process::Run;
use cf_e2e::rig::{Config, Project, Rig};
use serde_json::{json, Value};

use crate::{config, secs, Outcome};

/// The keys the human answers the question with: Down to the yes, then Enter.
const DOWN: &[u8] = b"\x1b[B";
const ENTER: &[u8] = b"\r";

/// The rig, the worker's windows asking, its launches given 8 s.
fn asking() -> Config {
    config()
        .var("CF_TEST_TRUST", "worker")
        .var("CONSENSFLOW_LAUNCH_TIMEOUT_MS", "8000")
}

/// `git init` in `folder`.
fn git_init(folder: &Path) -> Outcome {
    let ran = Run::new(Path::new("git"))
        .args(["init", "-q"])
        .arg(folder)
        .inheriting_env()
        .run()?;
    assert_eq!(ran.code, Some(0), "git init {}: {ran}", folder.display());
    Ok(())
}

/// The chief gives the worker two tasks at once.
fn two_tasks(rig: &Rig, project: &Project) -> Outcome {
    let added = project.add_member("worker")?;
    let tier = added["member"]["tier"].as_str().unwrap_or_default();
    for word in ["FIRST_DONE", "SECOND_DONE"] {
        rig.tell(
            project.id(),
            &format!("DISPATCH --tier {tier} Reply with exactly: {word}"),
        )?;
    }
    Ok(())
}

/// The notes to the human that speak of trusting `folder`.
fn notes_of(project: &Project, folder: &Path) -> cf_e2e::Result<Vec<String>> {
    let folder = folder.to_string_lossy();
    Ok(project
        .inbox("human")?
        .iter()
        .filter_map(|message| message["body"].as_str())
        .filter(|body| body.contains("trust") && body.contains(&*folder))
        .map(str::to_owned)
        .collect())
}

/// The folders the stand-in was told to trust, a yes each.
fn trusted(rig: &Rig) -> String {
    let config = rig.var("CLAUDE_CONFIG_DIR").unwrap_or_default();
    files::read_string(&Path::new(config).join("trusted-folders")).unwrap_or_default()
}

/// The worker's windows open now.
fn workers(rig: &Rig, project: &Project) -> Vec<Value> {
    let prefix = format!("p{}-worker", project.id());
    rig.open_frames()
        .into_iter()
        .filter(|frame| {
            frame["id"]
                .as_str()
                .is_some_and(|id| id.starts_with(&prefix))
        })
        .collect()
}

/// Both tasks done.
fn both_done(project: &Project) -> cf_e2e::Result<bool> {
    Ok(project.task(1)?["state"] == "done" && project.task(2)?["state"] == "done")
}

#[test]
fn a_worker_asked_to_trust_the_projects_new_repository_is_answered_yes_and_does_its_tasks(
) -> Outcome {
    let rig = Rig::start(asking())?;
    git_init(rig.workspace())?;
    let project = Project::open(&rig, "chief", json!({}))?;
    two_tasks(&rig, &project)?;
    rig.wait_for("T-1 and T-2 to be done", secs(90), || both_done(&project))?;
    // ConsensFlow said yes on the screen, once for the folder: the stand-in wrote it down.
    let folder = rig.workspace();
    let same = |line: &str| std::fs::canonicalize(line).ok() == std::fs::canonicalize(folder).ok();
    assert!(
        trusted(&rig).lines().any(same),
        "the stand-in was never told yes: {}",
        trusted(&rig)
    );
    let notes = notes_of(&project, folder)?;
    assert_eq!(notes.len(), 1, "the human is told once: {notes:?}");
    assert!(notes[0].contains("answered yes"), "{notes:?}");
    Ok(())
}

#[test]
fn a_chief_asked_to_trust_the_projects_new_repository_is_answered_yes_and_takes_what_it_is_told(
) -> Outcome {
    let rig = Rig::start(config().var("CF_TEST_TRUST", "chief"))?;
    git_init(rig.workspace())?;
    let project = Project::open(&rig, "chief", json!({}))?;
    // What the human tells the chief goes into its window once it is past the question.
    rig.tell(project.id(), "Reply with exactly: CHIEF_HEARD")?;
    let chief = project.lane("chief")?;
    assert_eq!(chief["activity"]["state"], "idle", "{chief}");
    let notes = notes_of(&project, rig.workspace())?;
    assert_eq!(notes.len(), 1, "the human is told once: {notes:?}");
    assert!(
        notes[0].contains("@chief") && notes[0].contains("answered yes"),
        "{notes:?}"
    );
    Ok(())
}

#[test]
fn a_worker_whose_yes_would_trust_a_folder_outside_the_project_waits_for_the_human_and_goes_on_after(
) -> Outcome {
    let rig = Rig::start(asking())?;
    // The repository is the workspace; the project's folder is below it.
    let root = rig.workspace().to_path_buf();
    git_init(&root)?;
    let folder = root.join("app");
    std::fs::create_dir(&folder)?;
    let project = Project::open(&rig, "chief", json!({ "directory": folder }))?;
    two_tasks(&rig, &project)?;
    rig.wait_for(
        "the human to be told of the folder outside",
        secs(60),
        || Ok(!notes_of(&project, &root)?.is_empty()),
    )?;
    rig.wait_for("both of the worker's windows to be open", secs(30), || {
        Ok(workers(&rig, &project).len() == 2)
    })?;
    // Past the time a launch has, nothing failed, and nothing said yes for the human.
    sleep(Duration::from_secs(12));
    for number in [1, 2] {
        let state = project.task(number)?["state"].clone();
        assert!(
            state != "failed" && state != "done",
            "T-{number} is {state} while its window asks"
        );
    }
    assert_eq!(trusted(&rig), "", "ConsensFlow answered for the human");
    // The board says why the worker's windows are still starting.
    let lane = project.lane("worker")?;
    assert_eq!(lane["activity"]["state"], "starting", "{lane}");
    assert_eq!(
        lane["activity"]["reason"], "the window asks whether to trust its folder",
        "{lane}"
    );
    let notes = notes_of(&project, &root)?;
    assert_eq!(notes.len(), 1, "the human is told once: {notes:?}");
    assert!(
        notes[0].contains("outside the project's folder"),
        "{notes:?}"
    );
    // The human says yes in one of the windows; both tasks go on by themselves.
    let window = workers(&rig, &project)
        .first()
        .cloned()
        .ok_or("no window of the worker")?;
    for keys in [DOWN, ENTER] {
        let pressed = rig.host(
            "pane.input",
            json!({ "id": window["id"], "generation": window["generation"], "bytes": keys }),
        )?;
        assert_eq!(pressed["ok"], true, "{pressed}");
        sleep(Duration::from_millis(600));
    }
    rig.wait_for("T-1 and T-2 to be done", secs(90), || both_done(&project))?;
    assert_eq!(notes_of(&project, &root)?.len(), 1, "told once");
    Ok(())
}

#[test]
fn a_worker_the_human_said_yes_in_that_starts_slowly_has_its_first_messages_whole_time_from_then(
) -> Outcome {
    // Its prompt drawn, it writes its first message 4 s after the yes, past the time
    // its launch had counted from the launch.
    let rig = Rig::start(asking().var("CF_TEST_TRUST_STARTS_MS", "4000"))?;
    let root = rig.workspace().to_path_buf();
    git_init(&root)?;
    let folder = root.join("app");
    std::fs::create_dir(&folder)?;
    let project = Project::open(&rig, "chief", json!({ "directory": folder }))?;
    let added = project.add_member("worker")?;
    let tier = added["member"]["tier"].as_str().unwrap_or_default();
    rig.tell(
        project.id(),
        &format!("DISPATCH --tier {tier} Reply with exactly: SLOW_DONE"),
    )?;
    rig.wait_for("the worker's window to ask", secs(60), || {
        Ok(!notes_of(&project, &root)?.is_empty() && workers(&rig, &project).len() == 1)
    })?;
    sleep(Duration::from_secs(10));
    let window = workers(&rig, &project)
        .first()
        .cloned()
        .ok_or("no window of the worker")?;
    for keys in [DOWN, ENTER] {
        let pressed = rig.host(
            "pane.input",
            json!({ "id": window["id"], "generation": window["generation"], "bytes": keys }),
        )?;
        assert_eq!(pressed["ok"], true, "{pressed}");
        sleep(Duration::from_millis(600));
    }
    rig.wait_for("T-1 to be done", secs(60), || {
        let state = project.task(1)?["state"].clone();
        assert_ne!(state, "failed", "T-1 failed after the yes");
        Ok(state == "done")
    })?;
    Ok(())
}

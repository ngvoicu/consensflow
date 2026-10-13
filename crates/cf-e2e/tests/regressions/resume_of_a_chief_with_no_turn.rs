//! A project whose chief never had a turn could not be resumed.
//!
//! Seen by the owner on 2026-10-13, on ConsensFlow 3.0.3 with codex-cli 0.162.1: a
//! project was opened with a Codex chief, whose window came up idle; the owner
//! closed the project (Close) and pressed Resume. The project went open and, 0.6 s
//! later, suspended again, and no window ever showed. Nothing had been said in the
//! chief's thread, and Codex writes a thread's record only once the thread has a
//! turn: the Resume opened the window on `codex resume <thread>`, which found
//! nothing and ended at once, and a chief's own exit closes its project. The report
//! is the issue `issues/a-chief-that-never-had-a-turn-could-not-be-resumed.md` in
//! consensflow-sme.
//!
//! What the owner should have seen: the chief's window opens, on a conversation of
//! its own (there was nothing to go on with), and the project stays open. A chief
//! that did have a turn is resumed on its conversation, as it always was.
//!
//! The stand-in Codex (`fake-codex`) keeps what Codex keeps: a thread has a rollout
//! only once it has had a turn, and a window resumed on one that has none ends at
//! once, with what the server said. So the case is the report's own: a Codex chief
//! that was opened, idle, closed and resumed, through the real daemon and pane host.
//! The Claude cases beside it say the same of a harness that keeps its transcript
//! once something was said in the conversation: a chief that never had a turn is
//! opened afresh, and one that did is resumed on its conversation, right after
//! the turn, before the daemon's next look has copied it
//! (`resume_of_a_turn_the_daemon_never_copied`).

use std::error::Error;
use std::thread::sleep;

#[cfg(unix)]
use cf_e2e::files;
#[cfg(unix)]
use cf_e2e::live::trace::Trace;
use cf_e2e::rig::{Project, Rig};
use serde_json::{json, Value};

#[cfg(unix)]
use crate::config_with_codex;
use crate::{config, secs, Outcome};

/// The chief's windows the daemon opened so far, oldest first.
pub(crate) fn windows_of(rig: &Rig, project: &Project) -> Vec<Value> {
    let id = format!("p{}-chief", project.id());
    rig.open_frames()
        .into_iter()
        .filter(|frame| frame["id"] == id.as_str())
        .collect()
}

/// The word after `flag` in a window's command line.
pub(crate) fn after<'a>(window: &'a Value, flag: &str) -> Option<&'a str> {
    let argv = window["argv"].as_array()?;
    let at = argv.iter().position(|word| word == flag)?;
    argv.get(at + 1)?.as_str()
}

/// The conversation a window was opened on that already was: the word after
/// `resume` (Codex's) or `--resume` (Claude's, Devin's); none for a window that
/// opened on a conversation of its own.
pub(crate) fn resumed_on(window: &Value) -> Option<&str> {
    after(window, "resume").or_else(|| after(window, "--resume"))
}

/// The state of the project, as the board says it.
fn state(project: &Project) -> cf_e2e::Result<String> {
    Ok(project.board()?["project"]["state"]
        .as_str()
        .unwrap_or_default()
        .to_owned())
}

/// The chief's window to be idle, with nothing said in it yet: the project as the
/// owner had it before Close.
pub(crate) fn wait_for_the_chief(rig: &Rig, project: &Project) -> cf_e2e::Result<()> {
    rig.wait_for("the chief's window to be idle", secs(60), || {
        Ok(project.lane("chief")?["activity"]["state"] == "idle")
    })
}

/// The human's Close and Resume: the project is suspended and the chief's window
/// gone, and then the project is open and the chief's window opened again. The
/// window that was open, and the one opened again.
fn closed_and_resumed(rig: &Rig, project: &Project) -> Result<(Value, Value), Box<dyn Error>> {
    let before = windows_of(rig, project)
        .into_iter()
        .next()
        .ok_or("the chief's window was never opened")?;
    let closed = rig.page("project.close", json!({ "project": project.id() }))?;
    assert_eq!(closed["ok"], true, "{closed}");
    rig.wait_for("the chief's window to close", secs(60), || {
        Ok(project.lane("chief")?["pane"].is_null())
    })?;
    assert_eq!(state(project)?, "suspended");

    let resumed = rig.page("project.resume", json!({ "project": project.id() }))?;
    assert_eq!(resumed["ok"], true, "{resumed}");
    rig.wait_for("the chief's window to be opened again", secs(60), || {
        Ok(windows_of(rig, project).len() >= 2)
    })?;
    let again = windows_of(rig, project).remove(1);
    Ok((before, again))
}

/// The project open, with the chief's window still there, well after the moment
/// the owner's chief went (0.6 s after the Resume).
pub(crate) fn stays_open(rig: &Rig, project: &Project, window: &Value) -> Outcome {
    sleep(secs(3));
    assert_eq!(
        state(project)?,
        "open",
        "the chief's window ended: {:?}",
        rig.exits()
    );
    let lane = project.lane("chief")?;
    assert_eq!(lane["pane"]["id"], window["id"], "{lane}");
    assert!(
        !rig.exits()
            .iter()
            .any(|exit| exit["id"] == window["id"] && exit["generation"] == window["generation"]),
        "the window it was opened with ended"
    );
    Ok(())
}

/// The owner's own: a Codex chief opened and idle, never spoken to, closed and
/// resumed. Unix's, the stand-in Codex being a script.
#[cfg(unix)]
#[test]
fn a_codex_chief_that_never_had_a_turn_comes_back_on_the_resume_and_the_project_stays_open(
) -> Outcome {
    let rig = Rig::start(config_with_codex())?;
    let roster = json!({
        "schemaVersion": 1,
        "agents": [{ "id": "codex-chief", "kind": "codex", "model": "fake" }],
    });
    files::write(&rig.home().join("agents.json"), format!("{roster}\n"))?;
    let project = Project::open(&rig, "codex-chief", json!({}))?;
    wait_for_the_chief(&rig, &project)?;

    let (before, again) = closed_and_resumed(&rig, &project)?;

    // There was no thread record to go on with: the window is not opened on one.
    assert_eq!(resumed_on(&before), None);
    assert_eq!(
        resumed_on(&again),
        None,
        "the window was opened on a thread nothing was said in"
    );
    // The broker names the thread the new window opened, which the daemon binds.
    rig.wait_for("the new window's thread to be named", secs(60), || {
        Ok(Trace::read(&rig).threads_bound().len() >= 2)
    })?;
    let named = Trace::read(&rig).threads_bound();
    assert_eq!(named.len(), 2, "a conversation for each window: {named:?}");
    assert_ne!(named[0], named[1], "the second is a thread of its own");
    stays_open(&rig, &project, &again)?;
    wait_for_the_chief(&rig, &project)?;
    rig.close()?;
    Ok(())
}

/// A Claude chief the same way: with nothing said in it, the window is opened on
/// a session of its own.
#[test]
fn a_claude_chief_that_never_had_a_turn_is_opened_afresh_by_the_resume_and_the_project_stays_open(
) -> Outcome {
    let rig = Rig::start(config())?;
    let project = Project::open(&rig, "chief", json!({}))?;
    wait_for_the_chief(&rig, &project)?;

    let (before, again) = closed_and_resumed(&rig, &project)?;

    let session = after(&before, "--session-id").ok_or("the first window has no session")?;
    assert_eq!(
        resumed_on(&again),
        None,
        "the window was opened on a session nothing was said in"
    );
    let fresh = after(&again, "--session-id").ok_or("the window opened again has none")?;
    assert_ne!(fresh, session, "a session of its own");
    stays_open(&rig, &project, &again)?;
    wait_for_the_chief(&rig, &project)?;
    rig.close()?;
    Ok(())
}

/// And one the human spoke to, which answered: it is resumed on its session.
#[test]
fn a_claude_chief_that_had_a_turn_is_resumed_on_its_conversation_and_the_project_stays_open(
) -> Outcome {
    let rig = Rig::start(config())?;
    let project = Project::open(&rig, "chief", json!({}))?;
    wait_for_the_chief(&rig, &project)?;
    let session = windows_of(&rig, &project)
        .first()
        .and_then(|first| after(first, "--session-id"))
        .ok_or("the first window has no session")?
        .to_owned();
    rig.tell(project.id(), "Reply with exactly: HELLO")?;
    rig.wait_for("the chief to answer", secs(60), || {
        Ok(rig.transcript(&session).contains("HELLO"))
    })?;

    let (_, again) = closed_and_resumed(&rig, &project)?;

    assert_eq!(
        resumed_on(&again),
        Some(session.as_str()),
        "the window was not opened on the session it spoke in"
    );
    stays_open(&rig, &project, &again)?;
    rig.close()?;
    Ok(())
}

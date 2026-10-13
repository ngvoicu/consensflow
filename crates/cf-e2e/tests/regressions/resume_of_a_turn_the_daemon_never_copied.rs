//! A chief whose window saved a turn that the daemon had not yet copied was opened
//! afresh by the Resume, and its conversation was thrown away.
//!
//! Found in the review of the fix for a chief that never had a turn
//! (`resume_of_a_chief_with_no_turn`, 2026-10-13), not seen by the owner: that fix
//! went on with a conversation only if the ledger had seen a turn in it. The daemon
//! copies a window's conversation a look at a time, once a second, so a turn the
//! human typed into the window, which its harness saved at once, was in no copy
//! until the next look; a Close or a quit before it opened the window afresh, with
//! the conversation behind it left in the harness. The report is the issue
//! `issues/a-chief-that-never-had-a-turn-could-not-be-resumed.md` in
//! consensflow-sme.
//!
//! What should have happened: the harness's own record says whether there is
//! anything to go on with, whatever the copy holds, so the window is opened on the
//! conversation it had, and the project stays open.
//!
//! The cases are the quit's own shape, and no look can come between the turn and
//! it: the app quits daemon first, so the daemon is gone before the human types into
//! the chief's window, which saves the turn all the same; the app is started again
//! on the same home, and the human resumes the project. A stand-in saves a turn as
//! its harness does (Claude's transcript, Codex's rollout, Devin's store).

use std::error::Error;
use std::path::Path;

use cf_e2e::files;
use cf_e2e::live::trace::Trace;
use cf_e2e::rig::{Config, Project, Rig};
use rusqlite::{Connection, OpenFlags};
use serde_json::{json, Value};

#[cfg(unix)]
use crate::config_with_codex;
use crate::resume_of_a_chief_with_no_turn::{
    resumed_on, stays_open, wait_for_the_chief, windows_of,
};
use crate::{config, secs, Outcome, FAKE_DEVIN};

/// What the human types into the chief's window, and what the stand-ins answer.
const TYPED: &str = "Reply with exactly: SAVED";

/// The app quits as the human types into the chief's window: the daemon goes first,
/// the window saves the turn (`saved` says when its harness's record has it, told the
/// conversation), and the app is started again on the same home, with `config`, and
/// the project resumed. The conversation the chief's window was on, the window the
/// Resume opened, and the rig it opened in.
fn resumed_after_a_turn_the_daemon_never_saw(
    mut rig: Rig,
    config: &Config,
    project: i64,
    saved: impl Fn(&Rig, &str) -> bool,
) -> Result<(String, Value, Rig), Box<dyn Error>> {
    wait_for_the_chief(&rig, &Project::new(&rig, project))?;
    let first = windows_of(&rig, &Project::new(&rig, project)).remove(0);
    let conversation = Trace::read(&rig)
        .threads_bound()
        .first()
        .cloned()
        .ok_or("the daemon bound no conversation to the chief")?;

    rig.kill_daemon();
    rig.wait_for_daemon_exit(secs(10))?;
    let typed = rig.host(
        "pane.input",
        json!({
            "id": first["id"],
            "generation": first["generation"],
            "bytes": format!("\u{1b}[200~{TYPED}\u{1b}[201~\r").as_bytes(),
        }),
    )?;
    assert_eq!(typed["ok"], true, "{typed}");
    rig.wait_for("the harness to have saved the turn", secs(30), || {
        Ok(saved(&rig, &conversation))
    })?;

    let root = rig.close_keeping_the_root()?;
    let rig = Rig::start(config.clone().existing_root(root))?;
    let resumed = rig.page("project.resume", json!({ "project": project }))?;
    assert_eq!(resumed["ok"], true, "{resumed}");
    let again = rig.open_frame(&format!("p{project}-chief"), secs(30))?;
    Ok((conversation, again, rig))
}

/// The window the Resume opened is on the conversation the first one was on, and
/// the project stays open.
fn goes_on_where_it_was(rig: &Rig, project: i64, conversation: &str, again: &Value) -> Outcome {
    assert_eq!(
        resumed_on(again),
        Some(conversation),
        "the window was not opened on the conversation it saved a turn in: {:?}",
        again["argv"]
    );
    stays_open(rig, &Project::new(rig, project), again)
}

#[test]
fn a_claude_chief_whose_window_saved_a_turn_the_daemon_never_copied_is_resumed_on_its_conversation(
) -> Outcome {
    let rig = Rig::start(config())?;
    let project = Project::open(&rig, "chief", json!({}))?.id();

    let (conversation, again, rig) =
        resumed_after_a_turn_the_daemon_never_saw(rig, &config(), project, |rig, session| {
            rig.transcript(session).contains("\"text\":\"SAVED\"")
        })?;

    goes_on_where_it_was(&rig, project, &conversation, &again)?;
    rig.close()?;
    Ok(())
}

/// The roster of a case whose chief is `id`, an agent of the harness `kind`.
fn write_roster(rig: &Rig, id: &str, kind: &str) -> cf_e2e::Result {
    let roster = json!({
        "schemaVersion": 1,
        "agents": [{ "id": id, "kind": kind, "model": "fake" }],
    });
    files::write(&rig.home().join("agents.json"), format!("{roster}\n"))
}

/// Whether Codex's rollout of the thread `thread` has the answer, which it writes
/// once the turn has been had (the stand-in's server, as Codex's, begins the rollout
/// with the first turn).
#[cfg(unix)]
fn codex_saved(rig: &Rig, thread: &str) -> bool {
    let ends = format!("-{thread}.jsonl");
    rig.var("CODEX_HOME")
        .and_then(|home| files::find(Path::new(home), &|name| name.ends_with(&ends)))
        .and_then(|rollout| files::read_string(&rollout).ok())
        .is_some_and(|text| text.contains("\"text\":\"SAVED\""))
}

#[cfg(unix)]
#[test]
fn a_codex_chief_whose_window_saved_a_turn_the_daemon_never_copied_is_resumed_on_its_thread(
) -> Outcome {
    let rig = Rig::start(config_with_codex())?;
    write_roster(&rig, "codex-chief", "codex")?;
    let project = Project::open(&rig, "codex-chief", json!({}))?.id();

    let (thread, again, rig) =
        resumed_after_a_turn_the_daemon_never_saw(rig, &config_with_codex(), project, codex_saved)?;

    goes_on_where_it_was(&rig, project, &thread, &again)?;
    rig.close()?;
    Ok(())
}

/// Whether Devin's store has the answer, which its window writes once the turn
/// is had.
fn devin_saved(rig: &Rig, _session: &str) -> bool {
    let store = rig
        .var("HOME")
        .and_then(|home| files::find(Path::new(home), &|name| name == "sessions.db"));
    store
        .and_then(|store| Connection::open_with_flags(store, OpenFlags::SQLITE_OPEN_READ_ONLY).ok())
        .and_then(|db| {
            db.query_row(
                "SELECT count(*) FROM message_nodes WHERE chat_message LIKE '%\"content\":\"SAVED\"%'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .ok()
        })
        .is_some_and(|saved| saved > 0)
}

#[test]
fn a_devin_chief_whose_window_saved_a_turn_the_daemon_never_copied_is_resumed_on_its_session(
) -> Outcome {
    let rig = Rig::start(config().devin(FAKE_DEVIN))?;
    write_roster(&rig, "devin-chief", "devin")?;
    let project = Project::open(&rig, "devin-chief", json!({}))?.id();

    let (session, again, rig) = resumed_after_a_turn_the_daemon_never_saw(
        rig,
        &config().devin(FAKE_DEVIN),
        project,
        devin_saved,
    )?;

    goes_on_where_it_was(&rig, project, &session, &again)?;
    rig.close()?;
    Ok(())
}

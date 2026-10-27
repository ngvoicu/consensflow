//! A Claude that quit idle with a message queued was read as cut in a turn: T-442's three, the
//! `SessionStart` sign (`issues/field-reports-2026-10-18-a-task-stuck-working-after-its-turn-ended.md`
//! in consensflow-sme, its limits for 3.0.6).
//!
//! Claude Code queues a job's notification as the job ends, in its process's memory, and writes
//! the enqueue to its record. A process that quits idle before it takes the message writes no
//! sign of its quit: no turn was cut, so no close of one, and ConsensFlow's settings run no hook
//! as Claude starts. Read with no window open, the record says a message is still to be taken,
//! a turn begun: at the Resume the chief was told "ConsensFlow stopped in the middle of your
//! turn, which was cut", as at the idle compaction of 2026-10-15
//! (`idle_compaction_read_as_a_cut_turn`), though it had been at rest. And a window opened again
//! on the conversation where the host never proved the program before it gone (a window open
//! when the app quit, which the window's quit ends after the daemon has gone) was told no
//! boundary, so the queued message held every turn of the new process: a chief that took one
//! message read working for good after it (T-526's shape, for a chief).
//!
//! What should have happened: Claude's own statuses say whether any Claude shows the
//! conversation, and none does once its window is gone: what the record holds then was written
//! by programs that are gone, and what they had queued went with them. The chief is told nothing
//! at the Resume, and takes the messages that come for it, one after the other.
//!
//! The cases are that shape on the rig: the chief answers a turn, a job's notification is
//! queued at the end of its record (as Claude Code 2.1.302 writes one), and the project is
//! closed and resumed, or the app quits with the chief's window open and starts again.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;

use cf_e2e::rig::{Project, Rig};
use cf_e2e::Error;
use serde_json::json;

use crate::idle_compaction_read_as_a_cut_turn::{told, wait_for_the_turn_to_end};
use crate::resume_of_a_chief_with_no_turn::{
    closed_and_resumed, resumed_on, stays_open, wait_for_the_chief, windows_of,
};
use crate::{config, secs, Outcome};

/// What a process that quit idle left at the end of the record of `session`, in the Claude
/// configuration `config`: a job's notification, queued after its turn ended, never taken.
fn queued_at_the_quit(config: &str, session: &str) -> std::io::Result<()> {
    let notification = "<task-notification>\n<task-id>bjob00003</task-id>\n<status>failed</status>\n<summary>Background command \"bjob00003\" failed with exit code 143</summary>\n</task-notification>";
    let record = json!({
        "type": "queue-operation", "operation": "enqueue", "timestamp": "2026-10-17T18:30:09.183Z",
        "sessionId": session, "content": notification,
    });
    let file = Path::new(config)
        .join("projects")
        .join("integration")
        .join(format!("{session}.jsonl"));
    let mut out = OpenOptions::new().append(true).open(file)?;
    out.write_all(format!("{record}\n").as_bytes())
}

/// The notes the chief was given that its turn was cut, and whether the words reached its
/// window's record of `session`.
fn told_cut(rig: &Rig, project: &Project, session: &str) -> cf_e2e::Result<(Vec<String>, bool)> {
    let notes = project
        .inbox("chief")?
        .iter()
        .filter(|message| message["kind"] == "note")
        .filter_map(|message| message["body"].as_str().map(str::to_owned))
        .filter(|body| body.contains("in the middle of your turn"))
        .collect();
    Ok((
        notes,
        rig.transcript(session)
            .contains("in the middle of your turn"),
    ))
}

/// The chief's turn on `said`, told to it, to end in its record of `session`.
fn answers(rig: &Rig, project: &Project, session: &str, said: &str) -> cf_e2e::Result<()> {
    rig.tell(project.id(), said)?;
    wait_for_the_turn_to_end(rig, session, said)
}

/// The chief answers HELLO, and a notification is queued at the end of its record: its session.
fn quit_with_a_message_queued(
    rig: &Rig,
    project: &Project,
) -> Result<String, Box<dyn std::error::Error>> {
    let session = told(rig, project, "Reply with exactly: HELLO")?;
    wait_for_the_turn_to_end(rig, &session, "Reply with exactly: HELLO")?;
    wait_for_the_chief(rig, project)?;
    let claude = rig
        .var("CLAUDE_CONFIG_DIR")
        .ok_or("the rig gives Claude no configuration")?
        .to_owned();
    queued_at_the_quit(&claude, &session)?;
    Ok(session)
}

#[test]
fn a_claude_chief_that_quit_idle_with_a_message_queued_is_given_no_note_at_a_resume() -> Outcome {
    let rig = Rig::start(config().var("CF_TEST_ANSWER_MS", "3000"))?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let session = quit_with_a_message_queued(&rig, &project)?;

    let (_, again) = closed_and_resumed(&rig, &project)?;
    stays_open(&rig, &project, &again)?;
    assert_eq!(resumed_on(&again), Some(session.as_str()));
    let (notes, reached) = told_cut(&rig, &project, &session)?;
    assert!(
        notes.is_empty(),
        "the chief was told its turn was cut: {notes:?}"
    );
    assert!(!reached, "the note reached the chief's window");
    answers(&rig, &project, &session, "Reply with exactly: AFTER")?;
    rig.close()?;
    Ok(())
}

#[test]
fn a_claude_chief_whose_window_the_apps_quit_ended_idle_with_a_message_queued_takes_every_message_after_the_restart(
) -> Outcome {
    let mut rig = Rig::start(config().var("CF_TEST_ANSWER_MS", "3000"))?;
    let (id, session) = {
        let project = Project::open(&rig, "chief", json!({}))?;
        let session = quit_with_a_message_queued(&rig, &project)?;
        (project.id(), session)
    };

    // The app quits with the chief's window open: the daemon dies, then the pane host, which
    // ends the window, and no daemon hears how it ended.
    rig.kill_daemon();
    rig.wait_for_daemon_exit(secs(10))?;
    let root = rig.close_keeping_the_root()?;
    let rig = Rig::start(
        config()
            .var("CF_TEST_ANSWER_MS", "3000")
            .existing_root(root),
    )?;
    let resumed = rig.page("project.resume", json!({ "project": id }))?;
    if resumed["ok"] != true {
        return Err(Error::Daemon(format!("project.resume: {resumed}")).into());
    }
    let project = Project::new(&rig, id);
    rig.wait_for("the chief's window to be opened again", secs(60), || {
        Ok(!windows_of(&rig, &project).is_empty())
    })?;
    let again = windows_of(&rig, &project).remove(0);
    assert_eq!(resumed_on(&again), Some(session.as_str()));
    wait_for_the_chief(&rig, &project)?;
    let (notes, reached) = told_cut(&rig, &project, &session)?;
    assert!(
        notes.is_empty(),
        "the chief was told its turn was cut: {notes:?}"
    );
    assert!(!reached, "the note reached the chief's window");
    // Each message reaches the chief once the one before is answered: the notification the
    // program before had queued holds none of them.
    answers(&rig, &project, &session, "Reply with exactly: AFTER")?;
    answers(&rig, &project, &session, "Reply with exactly: AGAIN")?;
    rig.close()?;
    Ok(())
}

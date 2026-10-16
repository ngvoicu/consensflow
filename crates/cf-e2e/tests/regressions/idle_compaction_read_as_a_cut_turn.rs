//! A Resume of 2026-10-15, ConsensFlow 3.0.4 on the owner's Mac, a Claude chief:
//! **the chief was told its turn was cut, and went to work, though it had been at
//! rest.**
//!
//! What the owner saw: at the Resume, the chief started working and nothing on
//! the board said why. ConsensFlow had sent it m-2063, "ConsensFlow stopped in the
//! middle of your turn, which was cut. Go on where you stopped", the first thing
//! the window was given; the chief answered that the note was false, the second
//! time (m-2048 on 2026-10-14 was the first). Its Claude log held the reason: its
//! last turn had ended (`end_turn`), and Claude Code had compacted the
//! conversation while it was idle, which writes a `compact_boundary` and the
//! summary as a user's record (`isCompactSummary`), and runs no turn. Read before
//! the window opened, the summary looked like a prompt never answered. The report
//! is the brain's `issues/idle-compaction-read-as-a-cut-turn.md`.
//!
//! What should have happened: a chief at rest is given no note at a Resume,
//! whatever Claude Code wrote of its own while it was idle.
//!
//! The case is that shape on the rig: the chief answers a turn, its stand-in's
//! record is compacted while it is idle (the two records written as Claude Code
//! 2.1 writes them), the human closes and resumes the project, and the window
//! opened again is given nothing.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;

use cf_e2e::rig::{Project, Rig};
use serde_json::json;

use crate::resume_of_a_chief_with_no_turn::{
    after, closed_and_resumed, stays_open, wait_for_the_chief, windows_of,
};
use crate::{config, secs, Outcome};

/// What a compaction while idle writes at the end of the record of `session`, in
/// the Claude configuration `config`: the boundary, whose chain starts afresh, and
/// the summary under it.
pub(crate) fn compacted_while_idle(config: &str, session: &str) -> std::io::Result<()> {
    let boundary = json!({
        "type": "system", "subtype": "compact_boundary", "uuid": "compact-boundary",
        "parentUuid": null, "logicalParentUuid": "the-turn-before",
        "content": "Conversation compacted", "isMeta": false, "level": "info",
        "sessionId": session, "isSidechain": false,
        "timestamp": "2026-10-14T16:43:55.179Z",
    });
    let summary = json!({
        "type": "user", "uuid": "compact-summary", "parentUuid": "compact-boundary",
        "message": { "role": "user", "content": "This session is being continued from a previous conversation that ran out of context." },
        "isCompactSummary": true, "isVisibleInTranscriptOnly": true,
        "sessionId": session, "isSidechain": false,
        "timestamp": "2026-10-14T16:43:55.176Z",
    });
    let file = Path::new(config)
        .join("projects")
        .join("integration")
        .join(format!("{session}.jsonl"));
    let mut record = OpenOptions::new().append(true).open(file)?;
    writeln!(record, "{boundary}")?;
    writeln!(record, "{summary}")
}

#[test]
fn a_claude_chief_whose_conversation_was_compacted_while_idle_is_given_no_note_at_a_resume(
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
    wait_for_the_chief(&rig, &project)?;
    let claude = rig
        .var("CLAUDE_CONFIG_DIR")
        .ok_or("the rig gives Claude no configuration")?
        .to_owned();
    compacted_while_idle(&claude, &session)?;

    let (_, again) = closed_and_resumed(&rig, &project)?;
    stays_open(&rig, &project, &again)?;

    let told: Vec<String> = project
        .inbox("chief")?
        .iter()
        .filter(|message| message["kind"] == "note")
        .filter_map(|message| message["body"].as_str().map(str::to_owned))
        .collect();
    assert!(
        !told
            .iter()
            .any(|body| body.contains("in the middle of your turn")),
        "the chief was told its turn was cut: {told:?}"
    );
    assert!(
        !rig.transcript(&session)
            .contains("in the middle of your turn"),
        "the note reached the chief's window"
    );
    rig.close()?;
    Ok(())
}

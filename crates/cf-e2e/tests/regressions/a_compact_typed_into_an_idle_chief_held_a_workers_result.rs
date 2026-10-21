//! A `/compact` typed into an idle Claude chief held a worker's result.
//!
//! Seen on 2026-10-19, on main before 3.0.5, in the Mac's two-hour live long run (Claude
//! Code 2.1.303, `claude-opus-5` at high effort, the chief): the chief ended its turn and
//! was idle, and the run typed `/compact` into its window, as a human may. Claude compacted
//! and went back to its prompt. Its record ends on what Claude Code 2.1.303 writes of that:
//! the bare `/compact` as it was typed, the boundary and the summary, the words it writes
//! before a command, the command, and what the command printed (which held the output of a
//! `PostCompact` hook of the owner's), then attachments. ConsensFlow read the chief at work
//! from then to the end of the run, 80 minutes: T-13's result, done four minutes later, was
//! never handed to it, and the project stalled with 5 of 13 tasks open. Claude's reader took
//! each of those user's records for a prompt, and nothing after them ended the turn they
//! began. The long run's dry run had played the same `/compact`, on a stand-in that wrote
//! only the boundary and the summary (the shape of 2026-10-15's idle compaction), and
//! passed. The report is the issue `issues/a-compact-typed-into-an-idle-chief-read-as-working.md`
//! in consensflow-sme.
//!
//! What should have been seen: the chief at rest once Claude is back at its prompt, and the
//! worker's result handed to it.
//!
//! The cases are that shape on the rig: the chief hands a worker a turn of 45 s and ends its
//! own; `/compact` is typed into its idle window, and its stand-in writes what Claude Code
//! 2.1.303 writes (`fake_agent`'s `compact`). In the second, `/compact` is typed again, which
//! finds nothing to compact, and the stand-in writes what Claude Code 2.1.304 writes of that
//! (the live run of 2026-10-20: the words before the command, the command, and what it printed
//! as the system's record, which ConsensFlow read as a turn that never ended too). The worker's
//! result must reach the chief. Each case holds one way the record ends a command's turn: the
//! second's own end would hide the first's.

use std::thread::sleep;
use std::time::Duration;

use cf_e2e::rig::{Project, Rig};
use serde_json::{json, Value};

use crate::resume_of_a_chief_with_no_turn::{after, wait_for_the_chief, windows_of};
use crate::{config, secs, Outcome};

/// The tier the worker was given, as a word the chief's brief names.
fn tier(added: &Value) -> &str {
    added["member"]["tier"].as_str().unwrap_or_default()
}

/// The keys of `/compact`, and the Enter that sends them, as the long run types them.
const COMPACT: &[u8] = b"/compact";
const ENTER: &[u8] = b"\r";

/// The human types `/compact` into the chief's window, a key at a time.
fn type_compact(rig: &Rig, project: &Project) -> Outcome {
    let pane = project.lane("chief")?["pane"].clone();
    for keys in [COMPACT, ENTER] {
        let typed = rig.host(
            "pane.input",
            json!({ "id": pane["id"], "generation": pane["generation"], "bytes": keys }),
        )?;
        assert_eq!(typed["ok"], true, "{typed}");
        sleep(Duration::from_millis(600));
    }
    Ok(())
}

/// The chief's result, once it is given one: its id, and whether it is delivered.
fn result(project: &Project) -> cf_e2e::Result<Option<(i64, bool)>> {
    Ok(project
        .inbox("chief")?
        .iter()
        .find(|message| message["kind"] == "result")
        .map(|message| {
            (
                message["id"].as_i64().unwrap_or_default(),
                message["state"] == "delivered",
            )
        }))
}

#[test]
fn a_claude_chief_given_a_compact_while_idle_is_at_rest_after_it_and_is_handed_its_workers_result(
) -> Outcome {
    compacted_and_handed_its_workers_result(false)
}

#[test]
fn a_claude_chief_given_a_compact_with_nothing_to_compact_is_at_rest_after_it_and_is_handed_its_workers_result(
) -> Outcome {
    compacted_and_handed_its_workers_result(true)
}

/// A chief whose worker is at work is given `/compact` while idle, and `/compact` again once
/// it compacted where `again`; then the worker's result must reach it.
fn compacted_and_handed_its_workers_result(again: bool) -> Outcome {
    let rig = Rig::start(config())?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    let session = windows_of(&rig, &project)
        .first()
        .and_then(|first| after(first, "--session-id"))
        .ok_or("the chief's window has no session")?
        .to_owned();
    rig.tell(
        project.id(),
        &format!(
            "DISPATCH --tier {} SLEEP 45 Reply with exactly: WORKER_OK",
            tier(&added)
        ),
    )?;
    rig.wait_for("T-1 to be at work", secs(60), || {
        Ok(project.task(1)?["state"] == "working")
    })?;
    wait_for_the_chief(&rig, &project)?;

    // The human types /compact into the chief's idle window, and maybe again once it
    // compacted: nothing is left to compact then.
    type_compact(&rig, &project)?;
    rig.wait_for("the chief's record to say it compacted", secs(30), || {
        Ok(rig.transcript(&session).contains("<local-command-stdout>"))
    })?;
    if again {
        type_compact(&rig, &project)?;
        rig.wait_for(
            "the chief's record to say it found nothing to compact",
            secs(30),
            || {
                Ok(rig
                    .transcript(&session)
                    .contains("\"subtype\":\"local_command\""))
            },
        )?;
    }
    assert_eq!(
        project.task(1)?["state"],
        "working",
        "the worker's result came before the compactions: the case holds nothing"
    );

    // The worker answers, and its result goes into the chief's window.
    rig.wait_for("T-1 to be done", secs(90), || {
        Ok(project.task(1)?["state"] == "done")
    })?;
    rig.wait_for(
        "the worker's result to be handed to the chief",
        secs(60),
        || {
            Ok(result(&project)?.is_some_and(|(id, delivered)| {
                delivered
                    && rig
                        .transcript(&session)
                        .contains(&format!("[ConsensFlow m-{id} "))
            }))
        },
    )?;
    wait_for_the_chief(&rig, &project)?;
    rig.close()?;
    Ok(())
}

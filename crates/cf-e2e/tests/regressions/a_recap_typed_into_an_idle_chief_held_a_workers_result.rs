//! A `/recap` typed into an idle Claude chief held a worker's result (field, 2026-11-27, Claude
//! Code 2.1.332): the chief's window read at work from the command's echo on, and three results
//! waited two hours. `/recap`'s rows are the words before a command, the command as a user's
//! record, and what it printed as the system's (`local_command`); no model is asked
//! (`typed-recap-after-a-compaction.jsonl`). The reader ends a typed command at its output for
//! the names in its closed list (`QUIET_COMMAND`); `recap` is one (a stopgap: the rule for any
//! typed command is built apart).
//!
//! The case: the chief hands a worker a turn of 15 s and ends its own; `/recap` is typed into
//! its idle window and the stand-in prints it 40 s after the echo (`CF_TEST_RECAP_MS`), as the
//! field's printed 3 s after. The worker's result is done and queued before the output. The
//! count is of the result's marker in the chief's record: none before the output (no try into a
//! window that is mid-command), exactly one after it.

use std::thread::sleep;
use std::time::Duration;

use cf_e2e::rig::{Project, Rig};
use serde_json::{json, Value};

use crate::a_compact_typed_into_an_idle_chief_held_a_workers_result::tier;
use crate::resume_of_a_chief_with_no_turn::{after, wait_for_the_chief, windows_of};
use crate::{config, secs, Outcome};

/// How many of the chief's user's records hold the header of the message `id`: the times it was
/// given the message (the record's queue rows hold it too, and are no try).
fn given(rig: &Rig, session: &str, id: i64) -> usize {
    let marker = format!("[ConsensFlow m-{id} ");
    rig.transcript(session)
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|record| record["type"] == "user")
        .map(|record| {
            record["message"]["content"]
                .to_string()
                .matches(&marker)
                .count()
        })
        .sum()
}

/// The human types `/recap` into the chief's window, a key at a time, as the page types: the
/// host counts them as the person's (`pane.typed`), so nothing of ConsensFlow's is pasted
/// over them. Typed as the daemon's keys (`pane.input`), a note due at the chief's rest went in
/// between `/recap` and its Enter and was sent with it (CI runs 46103990194, 46118208589).
fn type_recap(rig: &Rig, project: &Project) -> Outcome {
    let pane = project.lane("chief")?["pane"].clone();
    for keys in [&b"/recap"[..], &b"\r"[..]] {
        rig.types(&pane, keys)?;
        sleep(Duration::from_millis(600));
    }
    Ok(())
}

/// What a wait on the chief that gave up saw: the last user's rows of its record (where `/recap`
/// went, alone or with something else) and the end of its window's screen.
fn what_the_chief_shows(rig: &Rig, project: &Project, session: &str) -> String {
    let transcript = rig.transcript(session);
    let rows: Vec<String> = transcript
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|record| record["type"] == "user")
        .map(|record| {
            record["message"]["content"]
                .to_string()
                .chars()
                .take(160)
                .collect()
        })
        .collect();
    let screen = project
        .lane("chief")
        .ok()
        .and_then(|lane| lane["pane"]["id"].as_str().map(|id| rig.screen(id)))
        .unwrap_or_default();
    let tail: Vec<&str> = screen.lines().rev().take(8).collect();
    format!(
        "the chief's record, its last user's rows: {:?}\nthe chief's screen, its last lines: {:?}",
        &rows[rows.len().saturating_sub(3)..],
        tail.into_iter().rev().collect::<Vec<_>>()
    )
}

#[test]
fn a_claude_chief_given_a_recap_while_idle_is_handed_its_workers_result_once_after_the_output(
) -> Outcome {
    let rig = Rig::start(config().var("CF_TEST_RECAP_MS", "40000"))?;
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
            "DISPATCH --tier {} SLEEP 15 Reply with exactly: WORKER_OK",
            tier(&added)
        ),
    )?;
    rig.wait_for("T-1 to be at work", secs(60), || {
        Ok(project.task(1)?["state"] == "working")
    })?;
    wait_for_the_chief(&rig, &project)?;

    type_recap(&rig, &project)?;
    rig.wait_for("the chief's record to hold the echo", secs(30), || {
        Ok(rig.transcript(&session).contains("<command-name>/recap"))
    })
    .map_err(|failed| {
        format!(
            "{failed}\n{}",
            what_the_chief_shows(&rig, &project, &session)
        )
    })?;
    assert!(
        !rig.transcript(&session).contains("<local-command-stdout>"),
        "the output came with the echo: the case holds nothing"
    );

    // The worker answers while the command is still printing: the result is queued.
    rig.wait_for("T-1 to be done", secs(60), || {
        Ok(project.task(1)?["state"] == "done")
    })?;
    let mut queued = None;
    rig.wait_for("the result to be queued", secs(30), || {
        queued = project
            .inbox("chief")?
            .iter()
            .find(|message| message["kind"] == "result")
            .and_then(|message| message["id"].as_i64());
        Ok(queued.is_some())
    })?;
    let id = queued.ok_or("no result")?;
    // Ten seconds of looks before the output: no try.
    sleep(Duration::from_secs(10));
    assert!(
        !rig.transcript(&session).contains("<local-command-stdout>"),
        "the output came before the look: the case holds nothing"
    );
    assert_eq!(
        given(&rig, &session, id),
        0,
        "no try before the command's output"
    );

    rig.wait_for("the result to be handed to the chief", secs(90), || {
        Ok(given(&rig, &session, id) > 0)
    })?;
    wait_for_the_chief(&rig, &project)?;
    sleep(Duration::from_secs(5));
    assert_eq!(
        given(&rig, &session, id),
        1,
        "exactly one try after the output"
    );
    rig.close()?;
    Ok(())
}

//! A result that starts with the path of an image was pasted three times into a Claude
//! chief, and then failed.
//!
//! Seen on 2026-10-21, on ConsensFlow 3.0.5, in the owner's `sites` project (Claude
//! Code 2.1.306, the chief): the image designer's role asks its result to begin with the
//! absolute path of each file it saved, and Claude Code turns a pasted line that is the
//! path of an image into an attachment. The chief's record then held the result as
//! `[Image #4][ConsensFlow m-2619 · T-45 · result from @pygmalion-jolly-brook]` and the
//! words after it, the path gone, with the picture's source in a record of its own. The
//! header no longer began the item, so the paste was never confirmed: it was pasted again
//! twice ("the harness record never showed it in the 60 s its window was at rest") and
//! then failed, and the chief had read each result three times. Seeing the picture is
//! right and stays. The report is T-215 on the board; no issue in consensflow-sme holds it
//! yet.
//!
//! What should have been seen: the chief given the result once, with its picture, and the
//! result delivered.
//!
//! The chief is the stand-in Claude, whose input box makes of a paste what Claude Code
//! 2.1.308 does (`fake_agent`'s `images`, shown by `tests/live_image_paste.rs`); the worker
//! is the stand-in too, which saves a picture and answers with its path first (`SAVED`). On
//! the code before the fix the case takes about four minutes to fail, three arrival windows
//! of a minute.
//!
//! Claude Code on Windows does read a `C:\` path as an image, in every shape the Mac has (the
//! zeewin run of 2026-10-22, T-265, `WINDOWS_CONVERTS` of `tests/live_image_paste.rs`), so the
//! stand-in does not unescape a path on Windows, and the case runs there too.

use cf_e2e::rig::{Project, Rig};
use serde_json::{json, Value};

use crate::resume_of_a_chief_with_no_turn::{after, wait_for_the_chief, windows_of};
use crate::{config, secs, Outcome};

/// The picture the worker saves, and the words after its path.
const PICTURE: &str = "desk-illustration.png";
const WORDS: &str = "Warm editorial desk scene.";

/// The tier the worker was given, as a word the chief's brief names.
fn tier(added: &Value) -> &str {
    added["member"]["tier"].as_str().unwrap_or_default()
}

/// The worker's result, as the chief's inbox holds it.
fn result(project: &Project) -> cf_e2e::Result<Option<Value>> {
    Ok(project
        .inbox("chief")?
        .into_iter()
        .find(|message| message["kind"] == "result"))
}

/// What a user's record of the chief says in words: a text, or the text parts of a
/// message that has parts.
fn words_of(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|part| part["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// How many of the chief's user's records hold the header of the message `id`: the
/// times it was given the message. A companion record's `[Image: source: …]` holds none.
fn given(rig: &Rig, session: &str, id: i64) -> usize {
    let marker = format!("[ConsensFlow m-{id} ");
    rig.transcript(session)
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|record| record["type"] == "user")
        .map(|record| {
            words_of(&record["message"]["content"])
                .matches(&marker)
                .count()
        })
        .sum()
}

#[test]
fn a_result_that_starts_with_the_path_of_an_image_is_given_to_a_claude_chief_once_and_delivered(
) -> Outcome {
    let rig = Rig::start(config())?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    let session = windows_of(&rig, &project)
        .first()
        .and_then(|first| after(first, "--session-id"))
        .ok_or("the chief's window has no session")?
        .to_owned();
    wait_for_the_chief(&rig, &project)?;

    // The chief hands a designer's turn to the worker, which saves the picture and
    // answers with its path on the first line.
    rig.tell(
        project.id(),
        &format!("DISPATCH --tier {} SAVED {PICTURE} {WORDS}", tier(&added)),
    )?;
    rig.wait_for("T-1 to be done", secs(90), || {
        Ok(project.task(1)?["state"] == "done")
    })?;
    rig.wait_for("the result to be settled", secs(420), || {
        Ok(result(&project)?
            .is_some_and(|message| message["state"] == "delivered" || message["state"] == "failed"))
    })?;
    let settled = result(&project)?.ok_or("the chief has no result")?;
    let id = settled["id"].as_i64().ok_or("the result has no id")?;
    wait_for_the_chief(&rig, &project)?;

    let transcript = rig.transcript(&session);
    assert!(
        transcript.contains(&format!("[Image #1][ConsensFlow m-{id} ")),
        "the chief's window did not make an attachment of the path: the case holds nothing: {transcript}"
    );
    assert_eq!(
        settled["state"],
        "delivered",
        "the result (m-{id}) went into the chief's window {} times: {settled}",
        given(&rig, &session, id)
    );
    assert_eq!(
        given(&rig, &session, id),
        1,
        "the result (m-{id}) went into the chief's window more than once"
    );
    rig.close()?;
    Ok(())
}

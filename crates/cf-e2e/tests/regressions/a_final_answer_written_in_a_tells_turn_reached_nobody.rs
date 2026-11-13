//! A worker's complete answer, written in the turn that answered the chief's tell, reached
//! nobody.
//!
//! Seen by the chief on 2026-10-24, on 3.0.5 (T-429), and by the tester on 2026-10-25, on
//! 3.0.6-alpha.2 (T-534, on Claude Code and Codex): the chief ran `cf tell T-1 "…"`,
//! which pauses the task and puts the words to its window as a question. The worker answered
//! it with `cf answer`, went straight on in the same turn, and ended it with its full result.
//! The task was paused, so that result was no result, and no message carried it either. The
//! chief's `cf task resume` began a turn whose short follow-up ("already finished") became
//! the task's result, and the full one had to be asked for again. The issue is T-442 of the
//! board.
//!
//! The note still holds the final words, even if they differ from the answer to the tell.
//! When that tell was answered on the board, the chief gets the answer in its window and
//! the note is shown, not typed; while still paused, the answer gives the two ways on.
//! A resume before the answer reaches the chief keeps the shown note and makes it say the
//! task has moved on. When there is no board answer, the note remains the typed carrier of
//! the final words, including if the chief resumes before the note is delivered.
//!
//! The stand-in writes the shape: its record holds the question, then the final text after
//! the answer (`REPLY … THEN …` of the fake agent).

use cf_e2e::rig::{Project, Rig, OPEN};
use serde_json::{json, Value};

use crate::a_workers_still_going_was_given_as_its_result::{tier, told};
use crate::resume_of_a_chief_with_no_turn::{after, windows_of};
use crate::{config, secs, Outcome};

/// What the worker answers the tell with.
const ANSWER: &str = "TOLD-ANSWER-K3";

/// What the worker ends the turn of the tell with.
const REPORT: &str = "FINAL-RESULT-ALPHA-9Z";

/// The messages of T-1 the chief was given whose words hold `word`.
fn holding(project: &Project, word: &str) -> cf_e2e::Result<Vec<(String, String)>> {
    Ok(told(project)?
        .into_iter()
        .filter(|(_, body)| body.contains(word))
        .collect())
}

/// The chief's recorded conversation, where typed messages can be distinguished from
/// messages only present in its inbox.
fn chief_session(rig: &Rig, project: &Project) -> cf_e2e::Result<String> {
    windows_of(rig, project)
        .first()
        .and_then(|frame| after(frame, "--session-id"))
        .map(str::to_owned)
        .ok_or_else(|| cf_e2e::Error::Daemon("the chief has no session".to_owned()))
}

/// A user prompt in the chief's record containing `words`, including a carried message.
fn prompt_with(rig: &Rig, session: &str, words: &str) -> Option<String> {
    rig.transcript(session)
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|row| row["type"] == "user")
        .map(|row| row.to_string())
        .find(|row| row.contains(words))
}

/// The note containing the final words, whether typed or shown.
fn note(project: &Project) -> cf_e2e::Result<Option<Value>> {
    Ok(project.inbox("chief")?.into_iter().find(|message| {
        message["kind"] == "note"
            && message["body"]
                .as_str()
                .is_some_and(|body| body.contains(REPORT))
    }))
}

#[test]
fn the_answer_is_typed_with_ways_on_and_the_distinct_final_words_are_shown_and_can_be_finished(
) -> Outcome {
    let rig = Rig::start(config())?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    rig.tell(
        project.id(),
        &format!(
            "DISPATCH --tier {} SLEEP 15 Reply with exactly: ONE",
            tier(&added)
        ),
    )?;
    rig.wait_for("T-1 to be working", secs(30), || {
        Ok(project.task(1)?["state"] == "working")
    })?;

    let chief = rig.open_frame(&format!("p{}-chief", project.id()), OPEN)?;
    let session = chief_session(&rig, &project)?;
    let words = format!("Stop now. REPLY {ANSWER} THEN SLEEP 2 Reply with exactly: {REPORT}");
    let tell = rig.cf_in_window(&chief, ["tell", "T-1", words.as_str()])?;
    assert_eq!(tell.code, Some(0), "{tell}");

    // The window answers the tell, then ends with different final words.
    rig.wait_for(
        "the answer to the tell to reach the chief",
        secs(60),
        || {
            Ok(holding(&project, ANSWER)?
                .iter()
                .any(|(kind, _)| kind == "answer"))
        },
    )?;
    rig.wait_for(
        "the words the turn ended with to reach the chief",
        secs(60),
        || {
            Ok(holding(&project, REPORT)?
                .iter()
                .any(|(kind, _)| kind == "note"))
        },
    )?;
    let notes: Vec<String> = holding(&project, REPORT)?
        .into_iter()
        .filter(|(kind, _)| kind == "note")
        .map(|(_, body)| body)
        .collect();
    assert_eq!(notes.len(), 1, "one note in the inbox: {notes:?}");
    let shown = note(&project)?.expect("the note is in the inbox");
    let note_id = shown["id"].as_i64().expect("the note has an id");
    assert_eq!(shown["state"], "read", "{shown}");
    assert!(!shown["receipt"]["shown"].is_null(), "{shown}");
    let answer = project
        .inbox("chief")?
        .into_iter()
        .find(|message| {
            message["kind"] == "answer"
                && message["body"]
                    .as_str()
                    .is_some_and(|body| body.contains(ANSWER))
        })
        .expect("the board answer is in the inbox");
    assert!(answer["id"].as_i64().is_some(), "{answer}");
    rig.wait_for(
        "the answer to be typed while the task is paused",
        secs(90),
        || Ok(prompt_with(&rig, &session, ANSWER).is_some()),
    )?;
    let record = rig.transcript(&session);
    let prompt = prompt_with(&rig, &session, ANSWER).expect("the answer was typed");
    assert!(
        prompt.contains("cf task finish T-1") && prompt.contains("cf task resume T-1"),
        "{prompt}"
    );
    assert!(
        !record.contains(&format!("[ConsensFlow m-{note_id} ")),
        "{record}"
    );
    assert!(
        !record.contains(REPORT),
        "the distinct final words were typed: {record}"
    );
    assert!(
        notes[0].starts_with("T-1 is paused, and @")
            && notes[0].contains("cf task finish T-1")
            && notes[0].contains("cf task resume T-1"),
        "{notes:?}"
    );

    // The task is the chief's to go on with or to finish: paused, and no result.
    assert_eq!(project.task(1)?["state"], "paused");
    let results = |project: &Project| -> cf_e2e::Result<Vec<Value>> {
        Ok(project
            .inbox("chief")?
            .into_iter()
            .filter(|message| message["kind"] == "result")
            .collect())
    };
    assert_eq!(results(&project)?, Vec::<Value>::new());

    let finished = rig.cf_in_window(&chief, ["task", "finish", "T-1"])?;
    assert_eq!(finished.code, Some(0), "{finished}");
    assert_eq!(project.task(1)?["state"], "done");
    rig.wait_for("the result to reach the chief", secs(30), || {
        Ok(!results(&project)?.is_empty())
    })?;
    let result = &results(&project)?[0];
    assert!(
        result["body"]
            .as_str()
            .is_some_and(|body| body.starts_with(REPORT)),
        "the answer after the tell is the result: {result}"
    );
    rig.close()?;
    Ok(())
}

#[test]
fn an_answered_tells_note_stays_shown_when_the_chief_resumes_before_the_answer_is_typed() -> Outcome
{
    let rig = Rig::start(config())?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    rig.tell(
        project.id(),
        &format!(
            "DISPATCH --tier {} SLEEP 15 Reply with exactly: ONE",
            tier(&added)
        ),
    )?;
    rig.wait_for("T-1 to be working", secs(30), || {
        Ok(project.task(1)?["state"] == "working")
    })?;

    let chief = rig.open_frame(&format!("p{}-chief", project.id()), OPEN)?;
    let session = chief_session(&rig, &project)?;
    let words = format!("Stop now. REPLY {ANSWER} THEN SLEEP 2 Reply with exactly: {REPORT}");
    let tell = rig.cf_in_window(&chief, ["tell", "T-1", words.as_str()])?;
    assert_eq!(tell.code, Some(0), "{tell}");
    // The chief is at work on a turn of its own: nothing is given it until that is over.
    rig.tell(project.id(), "SLEEP 40 Reply with exactly: BUSY")?;

    rig.wait_for("the answered tell note to be shown", secs(60), || {
        Ok(note(&project)?.is_some())
    })?;
    let waiting = note(&project)?.unwrap_or_default();
    assert_eq!(waiting["state"], "read", "{waiting}");
    assert!(!waiting["receipt"]["shown"].is_null(), "{waiting}");
    assert!(
        waiting["body"]
            .as_str()
            .is_some_and(|body| body.starts_with("T-1 is paused, and @")),
        "{waiting}"
    );

    // The human resumes the task before the queued answer reaches the chief.
    let resumed = rig.page("task.resume", json!({ "project": project.id(), "task": 1 }))?;
    assert_eq!(resumed["ok"], true, "{resumed}");
    let kept = note(&project)?.unwrap_or_default();
    assert_eq!(kept["id"], waiting["id"], "the same note");
    assert_eq!(
        kept["state"], "read",
        "the shown note remains shown: {kept}"
    );
    assert!(!kept["receipt"]["shown"].is_null(), "{kept}");
    assert!(!kept["receipt"]["over"].is_null(), "{kept}");
    let body = kept["body"].as_str().unwrap_or_default();
    assert!(
        body.starts_with("T-1, paused when @")
            && body.contains("has moved on since")
            && body.ends_with(REPORT),
        "{kept}"
    );

    // The chief gets the board answer, not a second typed message for the note.
    let note_id = waiting["id"].as_i64().expect("the note has an id");
    let answer = project
        .inbox("chief")?
        .into_iter()
        .find(|message| {
            message["kind"] == "answer"
                && message["body"]
                    .as_str()
                    .is_some_and(|body| body.contains(ANSWER))
        })
        .expect("the board answer is in the inbox");
    assert!(answer["id"].as_i64().is_some(), "{answer}");
    rig.wait_for("the answer to reach the chief", secs(90), || {
        Ok(prompt_with(&rig, &session, ANSWER).is_some())
    })?;
    let record = rig.transcript(&session);
    assert!(
        !record.contains(&format!("[ConsensFlow m-{note_id} ")),
        "{record}"
    );
    assert!(
        !record.contains("T-1, paused when @"),
        "the note was carried into the chief's window: {record}"
    );
    assert!(
        rig.events().iter().all(|event| {
            event["kind"] != "delivery.begun" || event["data"]["message"] != note_id
        }),
        "the shown note was pasted"
    );
    rig.close()?;
    Ok(())
}

/// With no board answer, the note remains the only typed carrier of the final words.
#[test]
fn an_unanswered_tells_note_is_typed_after_the_chief_resumes() -> Outcome {
    let rig = Rig::start(config())?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    rig.tell(
        project.id(),
        &format!(
            "DISPATCH --tier {} SLEEP 15 Reply with exactly: ONE",
            tier(&added)
        ),
    )?;
    rig.wait_for("T-1 to be working", secs(30), || {
        Ok(project.task(1)?["state"] == "working")
    })?;
    let chief = rig.open_frame(&format!("p{}-chief", project.id()), OPEN)?;
    let session = chief_session(&rig, &project)?;
    let words = format!("Stop now. SLEEP 2 Reply with exactly: {REPORT}");
    let tell = rig.cf_in_window(&chief, ["tell", "T-1", words.as_str()])?;
    assert_eq!(tell.code, Some(0), "{tell}");
    rig.tell(project.id(), "SLEEP 40 Reply with exactly: BUSY")?;
    rig.wait_for("the unanswered tell note to wait", secs(60), || {
        Ok(note(&project)?.is_some())
    })?;
    let waiting = note(&project)?.expect("the note is in the inbox");
    assert_eq!(waiting["state"], "queued", "{waiting}");
    assert!(waiting["receipt"]["shown"].is_null(), "{waiting}");
    assert!(
        waiting["body"]
            .as_str()
            .is_some_and(|body| body.starts_with("T-1 is paused, and @")),
        "{waiting}"
    );
    let resumed = rig.page("task.resume", json!({ "project": project.id(), "task": 1 }))?;
    assert_eq!(resumed["ok"], true, "{resumed}");
    let kept = note(&project)?.expect("the same note remains");
    assert_eq!(kept["id"], waiting["id"], "{kept}");
    assert_eq!(kept["state"], "queued", "{kept}");
    assert!(kept["receipt"]["shown"].is_null(), "{kept}");
    let body = kept["body"].as_str().unwrap_or_default();
    assert!(
        body.starts_with("T-1, paused when @")
            && body.contains("has moved on since")
            && body.ends_with(REPORT),
        "{kept}"
    );
    rig.wait_for(
        "the unanswered tell note to reach the chief",
        secs(90),
        || {
            Ok(prompt_with(&rig, &session, "T-1, paused when @").is_some()
                && note(&project)?.is_some_and(|message| {
                    matches!(message["state"].as_str(), Some("delivered" | "read"))
                }))
        },
    )?;
    let prompt = prompt_with(&rig, &session, "T-1, paused when @")
        .expect("the note was typed, alone or in a carrier");
    assert!(prompt.contains(REPORT), "{prompt}");
    rig.close()?;
    Ok(())
}

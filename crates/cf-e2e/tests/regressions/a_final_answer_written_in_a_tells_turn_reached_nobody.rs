//! A worker's complete answer, written in the turn that answered the chief's tell, reached
//! nobody.
//!
//! Seen by the chief on 2026-10-24, on 3.0.5 (T-429), and by the tester on 2026-10-25, on
//! 3.0.6-alpha.2 (T-534, on Claude Code, Codex and OpenCode): the chief ran `cf tell T-1 "…"`,
//! which pauses the task and puts the words to its window as a question. The worker answered
//! it with `cf answer`, went straight on in the same turn, and ended it with its full result.
//! The task was paused, so that result was no result, and no message carried it either. The
//! chief's `cf task resume` began a turn whose short follow-up ("already finished") became
//! the task's result, and the full one had to be asked for again. The issue is T-442 of the
//! board.
//!
//! What the chief should have seen: the answer to the tell, as a message; then, when the
//! worker's turn is over, a note that holds the words it ended with, once; the task still
//! paused, with no result; and `cf task finish T-1` makes those words the result.
//!
//! The chief may resume before it has read that note (round 2, the chief's review of the
//! first fix): its answer to the tell tells it to, and the resume withdrew every note of the
//! pause it was not given yet, the one that holds the answer with them. The note stays, in
//! words true of a task that has moved on, and reaches the chief.
//!
//! The stand-in writes the shape: its record holds the question, then the final text after
//! the answer (`REPLY … THEN …` of the fake agent).

use cf_e2e::rig::{Project, Rig, OPEN};
use serde_json::{json, Value};

use crate::a_workers_still_going_was_given_as_its_result::{tier, told};
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

#[test]
fn the_final_answer_a_worker_ends_the_turn_of_a_tell_with_reaches_the_chief_and_finish_makes_it_the_result(
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
    let words = format!("Stop now. REPLY {ANSWER} THEN SLEEP 2 Reply with exactly: {REPORT}");
    let tell = rig.cf_in_window(&chief, ["tell", "T-1", words.as_str()])?;
    assert_eq!(tell.code, Some(0), "{tell}");

    // The window answers the tell, and goes on to its end; the chief has both.
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
    assert_eq!(notes.len(), 1, "told once: {notes:?}");
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
fn a_resume_before_the_chief_read_the_note_keeps_it_in_words_true_of_the_resumed_task_and_it_reaches_the_chief(
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
    let words = format!("Stop now. REPLY {ANSWER} THEN SLEEP 2 Reply with exactly: {REPORT}");
    let tell = rig.cf_in_window(&chief, ["tell", "T-1", words.as_str()])?;
    assert_eq!(tell.code, Some(0), "{tell}");
    // The chief is at work on a turn of its own: nothing is given it until that is over.
    rig.tell(project.id(), "SLEEP 40 Reply with exactly: BUSY")?;

    let note = |project: &Project| -> cf_e2e::Result<Option<Value>> {
        Ok(project.inbox("chief")?.into_iter().find(|message| {
            message["kind"] == "note"
                && message["body"]
                    .as_str()
                    .is_some_and(|body| body.contains(REPORT))
        }))
    };
    rig.wait_for("the note to wait for the busy chief", secs(60), || {
        Ok(note(&project)?.is_some())
    })?;
    let waiting = note(&project)?.unwrap_or_default();
    assert_eq!(waiting["state"], "queued", "{waiting}");
    assert!(
        waiting["body"]
            .as_str()
            .is_some_and(|body| body.starts_with("T-1 is paused, and @")),
        "{waiting}"
    );

    // The human resumes the task from its drawer before the chief has read the note.
    let resumed = rig.page("task.resume", json!({ "project": project.id(), "task": 1 }))?;
    assert_eq!(resumed["ok"], true, "{resumed}");
    let kept = note(&project)?.unwrap_or_default();
    assert_eq!(kept["id"], waiting["id"], "the same note");
    assert_eq!(
        kept["state"], "queued",
        "the resume does not take it back: {kept}"
    );
    let body = kept["body"].as_str().unwrap_or_default();
    assert!(
        body.starts_with("T-1, paused when @")
            && body.contains("has moved on since")
            && body.ends_with(REPORT),
        "{kept}"
    );

    // The chief's turn ends, and it is given the words the window ended its turn with.
    rig.wait_for("the chief to be given the note", secs(90), || {
        Ok(note(&project)?.is_some_and(|message| message["state"] == "delivered"))
    })?;
    rig.close()?;
    Ok(())
}

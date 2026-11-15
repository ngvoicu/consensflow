//! A task resumed after a tell read `queued` while its window worked on it.
//!
//! Seen by the owner on another board on 2026-11-02, on the build of the day (T-1139, field
//! bug 8): the chief stopped T-10 with `cf tell T-10 "…"` (now `cf task pause T-10 "…"`), which pauses the task and puts the
//! words to its window. The window answered ("ok") and went straight on with its work in the
//! same turn. The chief ran `cf task resume T-10 "…"`, and its words waited behind that turn,
//! which nothing is typed into while it is at work. The window's transcript showed tool calls
//! (certificate checks, `cargo metadata`), yet `cf task list` still read
//! `T-10 [queued] @sepa-mellow-thistle` at 22:48: only the paste of the resume's words moved
//! a task to working. The board under-reported work, and together with a task shown working
//! whose window never began (T-1130) neither label could be trusted without the transcript.
//!
//! What the chief should have seen: the task `working` as soon as its window's record shows
//! the window acting after the resume, with the resume's words still waiting for the window's
//! rest; when the turn on the tell ends, that turn is not the task's result (the note of a task
//! that has moved on holds what it ended with, shown when the tell was answered on the
//! board), the resume's words go in, and the answer to them is the result.
//!
//! The stand-in answers the tell with `cf answer` and goes on working through twelve steps,
//! three seconds apart (`REPLY … THEN PACED …`), writing a step into its record at each.

use cf_e2e::rig::{Project, Rig, OPEN};
use serde_json::{json, Value};

use crate::a_workers_still_going_was_given_as_its_result::{tier, told};
use crate::{config, secs, Outcome};

/// What the worker answers the tell with.
const ANSWER: &str = "TOLD-ANSWER-R7";

/// What the worker ends the turn of the tell with.
const REPORT: &str = "TELL-TURN-REPORT-Q4";

/// What the worker answers the resume's words with.
const FOLLOW_UP: &str = "FOLLOW-UP-DONE-M2";

/// The messages of T-1 the chief was given whose words hold `word`.
fn holding(project: &Project, word: &str) -> cf_e2e::Result<Vec<(String, String)>> {
    Ok(told(project)?
        .into_iter()
        .filter(|(_, body)| body.contains(word))
        .collect())
}

/// The last task message the window was sent about T-1: the words that resumed it.
fn resume_words(project: &Project) -> cf_e2e::Result<Option<Value>> {
    Ok(project.task(1)?["messages"]
        .as_array()
        .and_then(|messages| {
            messages
                .iter()
                .rfind(|message| {
                    message["kind"] == "task"
                        && message["body"]
                            .as_str()
                            .is_some_and(|body| body.starts_with("Resumed:"))
                })
                .cloned()
        }))
}

#[test]
fn a_task_resumed_after_a_tell_reads_working_while_its_window_goes_on_with_the_turn_and_its_words_go_in_after(
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

    // The chief tells the window; it answers, and goes on with twelve steps of work.
    let chief = rig.open_frame(&format!("p{}-chief", project.id()), OPEN)?;
    let words = format!("Stop now. REPLY {ANSWER} THEN PACED 12 3 Reply with exactly: {REPORT}");
    let tell = rig.cf_in_window(&chief, ["task", "pause", "T-1", words.as_str()])?;
    assert_eq!(tell.code, Some(0), "{tell}");
    rig.wait_for(
        "the answer to the tell to reach the chief",
        secs(60),
        || {
            Ok(holding(&project, ANSWER)?
                .iter()
                .any(|(kind, _)| kind == "answer"))
        },
    )?;
    assert_eq!(project.task(1)?["state"], "paused");

    // The chief resumes it at once, as the answer it was given tells it to.
    let said = format!("Go on. Reply with exactly: {FOLLOW_UP}");
    let resumed = rig.cf_in_window(&chief, ["task", "resume", "T-1", said.as_str()])?;
    assert_eq!(resumed.code, Some(0), "{resumed}");

    // The window is at work on it: the board says so, though the words wait for its rest.
    rig.wait_for(
        "T-1 to read working while its window works on it",
        secs(30),
        || Ok(project.task(1)?["state"] == "working"),
    )?;
    let waiting = resume_words(&project)?.unwrap_or_default();
    assert_eq!(
        waiting["state"], "queued",
        "the resume's words wait for the window's rest: {waiting}"
    );
    let list = rig.cf_in_window(&chief, ["task", "list"])?;
    assert!(
        list.stdout.contains("T-1 [working]") && !list.stdout.contains("T-1 [queued]"),
        "what the chief reads: {list}"
    );
    assert!(
        holding(&project, REPORT)?.is_empty(),
        "the turn on the tell has not ended"
    );

    // The tell was answered; the note is shown when the turn ends after the resume.
    rig.wait_for(
        "the words the turn ended with to be in the inbox",
        secs(90),
        || {
            Ok(holding(&project, REPORT)?
                .iter()
                .any(|(kind, _)| kind == "note"))
        },
    )?;
    let note = project
        .inbox("chief")?
        .into_iter()
        .find(|message| {
            message["kind"] == "note"
                && message["body"]
                    .as_str()
                    .is_some_and(|body| body.contains(REPORT))
        })
        .expect("the note is in the inbox");
    assert_eq!(note["state"], "read", "{note}");
    assert!(!note["receipt"]["shown"].is_null(), "{note}");
    assert!(!note["receipt"]["over"].is_null(), "{note}");
    let note_id = note["id"].as_i64().expect("the note has an id");
    let results = |project: &Project| -> cf_e2e::Result<Vec<String>> {
        Ok(project
            .inbox("chief")?
            .into_iter()
            .filter(|message| message["kind"] == "result")
            .filter_map(|message| message["body"].as_str().map(str::to_owned))
            .collect())
    };
    assert!(
        results(&project)?.iter().all(|body| !body.contains(REPORT)),
        "the turn before the resume's words is no result"
    );

    // The words go in once the window rests, and the answer to them is the result.
    rig.wait_for("the resume's words to go in", secs(60), || {
        Ok(resume_words(&project)?.is_some_and(|message| message["state"] == "delivered"))
    })?;
    rig.wait_for("the result to reach the chief", secs(60), || {
        Ok(!results(&project)?.is_empty())
    })?;
    let results = results(&project)?;
    assert!(
        results.len() == 1 && results[0].starts_with(FOLLOW_UP),
        "the answer to the resume's words is the result: {results:?}"
    );
    assert_eq!(project.task(1)?["state"], "done");
    assert!(
        rig.events().iter().all(|event| {
            event["kind"] != "delivery.begun" || event["data"]["message"] != note_id
        }),
        "the shown note was pasted"
    );
    rig.close()?;
    Ok(())
}

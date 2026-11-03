//! The words a worker wrote in reply to the chief's tell, before it ran anything, reached
//! nobody.
//!
//! Seen on 2026-11-02 by the live sweep (T-1131, `live tell devin` on the Mac, Devin
//! 3000.11.234, in two runs) and found by T-1151: the chief ran `cf tell T-1 "…"`, which pauses
//! the task and puts the words to its window as a question. The words asked for one line as
//! text, then a short command, then a last line. Devin wrote the first line, ran the command
//! (and `cf answer m-2` with the last line, in the second run), and ended its turn with the last
//! line. The chief was given the last line, in a note (T-442), and nothing carried the first:
//! the note took what follows the turn's last tool call and called the rest progress, and the
//! answer to a question is the first thing a window writes. The records of the sweep's Claude
//! Code, Codex and Muse runs hold the same shape (text, tool, tool, `cf answer`, text); theirs
//! reached the chief only because the model put the line into its `cf answer` or wrote it again
//! at the end.
//!
//! What the chief should see: a note with what the window wrote first, before any tool, and
//! what it said last, once; the task still paused, with no result; and `cf task finish T-1`
//! making the last words the result, not both.
//!
//! The stand-in writes the shape: `SAID <words> THEN <line>` is a message that says the words
//! and calls a tool, and the rest of the line after it (`REPLY <words> THEN …` answers with `cf
//! answer` first).

use cf_e2e::rig::{Project, Rig, OPEN};
use serde_json::{json, Value};

use crate::a_workers_still_going_was_given_as_its_result::{tier, told};
use crate::{config, secs, Outcome};

/// What the worker writes first, as text.
const WRITTEN: &str = "TOLD-ANSWER-K3";

/// What the worker ends the turn of the tell with.
const REPORT: &str = "FINAL-RESULT-ALPHA-9Z";

/// The messages of T-1 the chief was given whose words hold `word`.
fn holding(project: &Project, word: &str) -> cf_e2e::Result<Vec<(String, String)>> {
    Ok(told(project)?
        .into_iter()
        .filter(|(_, body)| body.contains(word))
        .collect())
}

/// A rig with T-1 working in a worker's window, and the chief's frame; the chief tells
/// the window `words`, and the note of the turn's end is waited for. The project, the chief's
/// frame and the note's words.
fn tell_and_wait_for_the_note<'a>(
    rig: &'a Rig,
    words: &str,
) -> cf_e2e::Result<(Project<'a>, Value, String)> {
    let project = Project::open(rig, "chief", json!({}))?;
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
    let tell = rig.cf_in_window(&chief, ["tell", "T-1", words])?;
    assert_eq!(tell.code, Some(0), "{tell}");
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
    Ok((project, chief, notes.into_iter().next().unwrap_or_default()))
}

/// The note holds what the worker wrote first, then what it said last, and says how to go on.
fn assert_the_note_holds_both(note: &str) {
    assert!(
        note.starts_with("T-1 is paused, and @")
            && note.contains("What it wrote first, before any tool:")
            && note.contains("cf task finish T-1")
            && note.contains("cf task resume T-1"),
        "{note}"
    );
    let (first, last) = (note.find(WRITTEN), note.find(REPORT));
    assert!(
        first.is_some() && last.is_some() && first < last,
        "the first words, then the last: {note}"
    );
}

/// The task is the chief's to go on with or to finish: paused, with no result; `cf task finish`
/// takes the last words, and only them, as the result.
fn assert_finish_takes_the_last_words(rig: &Rig, project: &Project, chief: &Value) -> Outcome {
    assert_eq!(project.task(1)?["state"], "paused");
    let results = |project: &Project| -> cf_e2e::Result<Vec<Value>> {
        Ok(project
            .inbox("chief")?
            .into_iter()
            .filter(|message| message["kind"] == "result")
            .collect())
    };
    assert_eq!(results(project)?, Vec::<Value>::new());
    let finished = rig.cf_in_window(chief, ["task", "finish", "T-1"])?;
    assert_eq!(finished.code, Some(0), "{finished}");
    assert_eq!(project.task(1)?["state"], "done");
    rig.wait_for("the result to reach the chief", secs(30), || {
        Ok(!results(project)?.is_empty())
    })?;
    let result = &results(project)?[0];
    let body = result["body"].as_str().unwrap_or_default();
    assert!(
        body.starts_with(REPORT) && !body.contains(WRITTEN),
        "the last words are the result: {result}"
    );
    Ok(())
}

#[test]
fn the_words_a_worker_writes_before_its_tools_in_the_turn_of_a_tell_reach_the_chief_beside_its_last_words(
) -> Outcome {
    // Devin's first run: the line as text, a command, the last line; no `cf answer`.
    let rig = Rig::start(config())?;
    let words = format!("Stop now. SAID {WRITTEN} THEN SLEEP 2 Reply with exactly: {REPORT}");
    let (project, chief, note) = tell_and_wait_for_the_note(&rig, &words)?;
    assert_the_note_holds_both(&note);
    assert!(
        !holding(&project, WRITTEN)?
            .iter()
            .any(|(kind, _)| kind == "answer"),
        "it ran no `cf answer`, so the note is the only place the chief has the first line"
    );
    assert_finish_takes_the_last_words(&rig, &project, &chief)?;
    rig.close()?;
    Ok(())
}

#[test]
fn the_words_a_worker_writes_before_its_tools_reach_the_chief_though_it_answered_the_tell_with_other_words(
) -> Outcome {
    // Devin's second run, which T-1131 reran alone: `cf answer m-2` carried the last line.
    let rig = Rig::start(config())?;
    let words = format!(
        "Stop now. SAID {WRITTEN} THEN REPLY {REPORT} THEN SLEEP 2 Reply with exactly: {REPORT}"
    );
    let (project, chief, note) = tell_and_wait_for_the_note(&rig, &words)?;
    assert_the_note_holds_both(&note);
    let answers: Vec<(String, String)> = told(&project)?
        .into_iter()
        .filter(|(kind, _)| kind == "answer")
        .collect();
    assert_eq!(
        answers
            .iter()
            .map(|(_, body)| body.as_str())
            .collect::<Vec<_>>(),
        [REPORT],
        "`cf answer` carried the last line alone"
    );
    assert_finish_takes_the_last_words(&rig, &project, &chief)?;
    rig.close()?;
    Ok(())
}

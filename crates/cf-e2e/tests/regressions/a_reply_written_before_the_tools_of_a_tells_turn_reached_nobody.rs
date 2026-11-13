//! The words a worker wrote in reply to the chief's tell, before it ran anything, reached
//! nobody.
//!
//! Seen on 2026-11-02 by the live sweep (T-1131, `live tell devin` on the Mac, Devin
//! 3000.11.234, in two runs) and found by T-1151: the chief ran `cf tell T-1 "…"` (now `cf task pause T-1 "…"`), which pauses
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
//! The note still holds the first and last words, and Finish takes only the last as the
//! result. Without a board answer the note is typed. If the window answered on the board,
//! the note is shown instead, even when the first words differ from that answer. The
//! answered case below checks both the note's contents and what the chief's window receives.
//!
//! The stand-in writes the shape: `SAID <words> THEN <line>` is a message that says the words
//! and calls a tool, and the rest of the line after it (`REPLY <words> THEN …` answers with `cf
//! answer` first).

use cf_e2e::rig::{Project, Rig, OPEN};
use serde_json::{json, Value};

use crate::a_workers_still_going_was_given_as_its_result::{tier, told};
use crate::resume_of_a_chief_with_no_turn::{after, windows_of};
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
    let tell = rig.cf_in_window(&chief, ["task", "pause", "T-1", words])?;
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

/// The chief's recorded conversation and the note's message, so an inbox entry is not
/// mistaken for a message that was typed into its window.
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

fn note_message(project: &Project) -> cf_e2e::Result<Value> {
    project
        .inbox("chief")?
        .into_iter()
        .find(|message| {
            message["kind"] == "note"
                && message["body"]
                    .as_str()
                    .is_some_and(|body| body.contains(REPORT))
        })
        .ok_or_else(|| cf_e2e::Error::Daemon("the note is not in the inbox".to_owned()))
}

#[test]
fn without_a_board_answer_the_first_and_last_words_are_typed_to_the_chief() -> Outcome {
    // Devin's first run: the line as text, a command, the last line; no `cf answer`.
    let rig = Rig::start(config())?;
    let words = format!("Stop now. SAID {WRITTEN} THEN SLEEP 2 Reply with exactly: {REPORT}");
    let (project, chief, note) = tell_and_wait_for_the_note(&rig, &words)?;
    assert_the_note_holds_both(&note);
    let message = note_message(&project)?;
    assert!(message["receipt"]["shown"].is_null(), "{message}");
    assert!(message["id"].as_i64().is_some(), "{message}");
    let session = chief_session(&rig, &project)?;
    rig.wait_for("the unanswered note to be typed", secs(90), || {
        Ok(prompt_with(&rig, &session, WRITTEN).is_some())
    })?;
    let prompt = prompt_with(&rig, &session, WRITTEN).expect("the note was typed");
    assert!(prompt.contains(REPORT), "{prompt}");
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
fn first_words_absent_from_the_board_answer_are_shown_but_not_typed() -> Outcome {
    // Devin's second run, which T-1131 reran alone: `cf answer m-2` carried the last line.
    let rig = Rig::start(config())?;
    let words = format!(
        "Stop now. SAID {WRITTEN} THEN REPLY {REPORT} THEN SLEEP 2 Reply with exactly: {REPORT}"
    );
    let (project, chief, note) = tell_and_wait_for_the_note(&rig, &words)?;
    assert_the_note_holds_both(&note);
    let message = note_message(&project)?;
    assert_eq!(message["state"], "read", "{message}");
    assert!(!message["receipt"]["shown"].is_null(), "{message}");
    let note_id = message["id"].as_i64().expect("the note has an id");
    let answer = project
        .inbox("chief")?
        .into_iter()
        .find(|message| message["kind"] == "answer" && message["body"] == REPORT)
        .expect("the answer is in the inbox");
    assert!(answer["id"].as_i64().is_some(), "{answer}");
    let session = chief_session(&rig, &project)?;
    rig.wait_for("the board answer to be typed", secs(90), || {
        Ok(prompt_with(&rig, &session, REPORT).is_some())
    })?;
    let record = rig.transcript(&session);
    assert!(
        !record.contains(&format!("[ConsensFlow m-{note_id} ")),
        "{record}"
    );
    assert!(
        !record.contains(WRITTEN),
        "the distinct first words were typed: {record}"
    );
    assert!(
        rig.events().iter().all(|event| {
            event["kind"] != "delivery.begun" || event["data"]["message"] != note_id
        }),
        "the shown note was pasted"
    );
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

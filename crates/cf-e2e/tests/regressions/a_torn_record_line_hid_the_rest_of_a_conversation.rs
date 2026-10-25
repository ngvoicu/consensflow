//! Found on 2026-10-22, on 3.0.6-alpha.1, by T-187 and Calliope's review of it (T-213):
//! **a line of a harness's record torn for good made the daemon blind to the whole
//! conversation, and said nothing.**
//!
//! How a line tears: a short write (a full disk, which this Mac had on 2026-10-16, or
//! a process killed in a large write) leaves part of a record, and the harness's next
//! append makes one line of it and itself, which is not JSON for good. Claude Code
//! goes on past such a line. The reader failed every look at the record from then on
//! ("malformed JSONL at record N"), as Node's did, so the engine read nothing of it:
//! no delivery confirmed, no turn's end was seen, no result was taken, and the copy
//! of the conversation stopped. The report is the brain's
//! `issues/a-torn-transcript-line-stops-the-daemon-copying-that-conversation.md`.
//!
//! What should happen (the chief's decision, T-223): the torn line is passed over once
//! a whole record follows it, with a line in the daemon's log that names the file and
//! the record, never the text; the rest of the conversation is read as if it were not
//! there.
//!
//! The case is that shape on the rig: a Claude worker's turn tears its record as it
//! works (the stand-in's `TORN`), and answers. The task is done with its answer; a
//! follow-up goes to the same window, past the torn line, and is done with its own;
//! the copy of the worker's conversation holds both answers. T-1's result ends with
//! a line that says a record of its turn was lost (T-248); T-2's, handed over after
//! the line, does not.

use cf_e2e::rig::{Project, Rig, OPEN};
use cf_e2e::Error;
use serde_json::{json, Value};

use crate::{config, secs, Outcome};

/// What the daemon's log says of a torn line it passed over.
const PASSED_OVER: &str = "passed over a torn line of ";

/// `cf` as the chief runs it in its window: a `cf` that fails is an error.
fn cf(rig: &Rig, chief: &Value, words: &[&str]) -> cf_e2e::Result<()> {
    let ran = rig.cf_in_window(chief, words)?;
    if ran.code == Some(0) {
        Ok(())
    } else {
        Err(Error::Daemon(format!("cf {}: {ran}", words.join(" "))))
    }
}

/// The bodies of the results the chief has been given or is to be given, oldest first.
fn results(project: &Project) -> cf_e2e::Result<Vec<String>> {
    let mut results: Vec<Value> = project
        .inbox("chief")?
        .into_iter()
        .filter(|message| message["kind"] == "result")
        .collect();
    results.sort_by_key(|message| message["id"].as_i64().unwrap_or_default());
    Ok(results
        .iter()
        .map(|message| message["body"].as_str().unwrap_or_default().to_owned())
        .collect())
}

/// Whether the task `number` is done.
fn done(project: &Project, number: i64) -> cf_e2e::Result<bool> {
    Ok(project.task(number)?["state"] == "done")
}

/// The assistant's words in ConsensFlow's copy of the conversations of the member `handle`.
fn copied(rig: &Rig, project: &Project, handle: &str) -> cf_e2e::Result<Vec<String>> {
    let read = rig.page(
        "history.get",
        json!({ "project": project.id(), "handle": handle, "tools": true }),
    )?;
    Ok(read["history"]["conversations"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|conversation| {
            conversation["items"]
                .as_array()
                .cloned()
                .unwrap_or_default()
        })
        .filter(|item| item["role"] == "assistant")
        .filter_map(|item| item["text"].as_str().map(str::to_owned))
        .collect())
}

#[test]
fn a_worker_whose_record_tore_a_line_is_read_past_it_its_results_taken_and_its_conversation_copied(
) -> Outcome {
    let rig = Rig::start(config())?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    let tier = added["member"]["tier"].as_str().unwrap_or_default();
    let chief = rig.open_frame(&format!("p{}-chief", project.id()), OPEN)?;

    // T-1's turn tears the worker's record as it works, then answers.
    cf(
        &rig,
        &chief,
        &[
            "task",
            "add",
            "--tier",
            tier,
            "TORN Reply with exactly: ONE",
        ],
    )?;
    rig.wait_for("T-1 to be done", secs(90), || done(&project, 1))?;
    let worker = project.task(1)?["assignee"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    // The worker's window is the one whose record T-1 was pasted into: the stand-ins write
    // their starts as each comes up, so under load the chief's can come last (the gate of
    // T-494).
    let session = rig
        .started()
        .into_iter()
        .map(|started| started.session)
        .find(|session| {
            rig.transcript(session)
                .contains("TORN Reply with exactly: ONE")
        })
        .ok_or("no window was given T-1")?;
    let record = rig.transcript(&session);
    let torn = record
        .lines()
        .position(|line| serde_json::from_str::<Value>(line).is_err())
        .ok_or("the worker's record holds no torn line")?;
    assert!(
        record
            .lines()
            .skip(torn + 1)
            .any(|line| line.contains(r#""text":"ONE""#)),
        "the answer is after the torn line"
    );
    rig.wait_for("T-1's result to reach the chief", secs(30), || {
        Ok(!results(&project)?.is_empty())
    })?;

    // A follow-up goes to the same window: its delivery is read past the torn line,
    // and its turn's end and its answer.
    cf(
        &rig,
        &chief,
        &[
            "task",
            "add",
            "--tier",
            tier,
            "--after",
            "T-1",
            "Reply with exactly: TWO",
        ],
    )?;
    rig.wait_for("T-2 to be done", secs(90), || done(&project, 2))?;
    assert_eq!(
        project.task(2)?["assignee"].as_str(),
        Some(worker.as_str()),
        "T-2 went to T-1's window"
    );
    rig.wait_for("both results to reach the chief", secs(30), || {
        Ok(results(&project)?.len() == 2)
    })?;
    // T-1's turn lost a record, which its result says; T-2 was handed over after it.
    let results = results(&project)?;
    assert_eq!(results[1], "TWO");
    let (answer, lost) = results[0]
        .split_once("\n\n")
        .ok_or("T-1's result says nothing of the line it lost")?;
    assert_eq!(answer, "ONE");
    assert!(
        lost.starts_with(&format!("A line of @{worker}'s record was lost (record "))
            && lost.ends_with("); this result may miss part of the turn."),
        "{lost}"
    );

    // The copy of the conversation went on past it.
    rig.wait_for("both answers to be copied", secs(30), || {
        let said = copied(&rig, &project, &worker)?;
        Ok(["ONE", "TWO"]
            .iter()
            .all(|word| said.iter().any(|text| text == word)))
    })?;

    // The log says it once, by the file and the record, and nothing of its text.
    let log = rig.log();
    let lines: Vec<&str> = log
        .lines()
        .filter(|line| line.contains(PASSED_OVER))
        .collect();
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(
        lines[0].contains(&format!("{session}.jsonl: not JSON, at record ")),
        "{}",
        lines[0]
    );
    assert!(!lines[0].contains("tool_use"), "{}", lines[0]);
    rig.close()?;
    Ok(())
}

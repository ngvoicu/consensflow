//! A chief as a bare harness: no ConsensFlow, the same workspace, prompt, model
//! and effort as the other arms, the owner answering in its terminal. With nothing
//! of ConsensFlow in the window, its session is found in the harness's own store
//! ([`cf_e2e::live::ask::newest_session`], which asks the harness code of the
//! product): Claude and Pi open on an id given to them; Codex, Devin and OpenCode
//! name theirs only in their stores, by folder.
//!
//! The numbers of a bare run are taken from the chief's own record, shaped as the
//! other arms' so that the same expectations read them: no board, so no tasks,
//! notes or board questions; what the owner was asked is its turns' ends.

use std::collections::HashSet;

use cf_e2e::live::ask::Item;
use serde_json::{json, Value};

use crate::js;
use crate::measure::{count_questions, owner_questions, owner_questions_value, Metrics};

/// How much of the chief's last words a report keeps.
const LAST_WORDS: usize = 1500;

/// A message ended a turn when its record says so, or when the window sat idle
/// after it: without ConsensFlow, Devin's and OpenCode's records never mark the
/// message that ends a turn, so the runner keeps the ids it saw at rest.
fn ends_turn(item: &Item, ends: &HashSet<String>) -> bool {
    item.complete || ends.contains(&item.id)
}

/// The run's numbers from the chief's own record: `files_changed` are the files
/// of the fixture it changed, and `ends` the ids of the messages the window rested
/// on.
pub fn bare_metrics(items: &[Item], files_changed: Vec<String>, ends: &HashSet<String>) -> Metrics {
    let assistant: Vec<&Item> = items
        .iter()
        .filter(|item| item.role == "assistant")
        .collect();
    let turn_ends: Vec<String> = assistant
        .iter()
        .filter(|item| ends_turn(item, ends))
        .map(|item| item.text.clone())
        .collect();
    let count = |n: usize| i64::try_from(n).unwrap_or(i64::MAX);
    Metrics {
        chief: "chief".to_owned(),
        owner_messages: items
            .iter()
            .filter(|item| item.role == "user")
            .map(|item| count(js::length(&item.text)))
            .collect(),
        chief_turns: count(assistant.len()),
        chief_edits: None,
        chief_last_words: assistant
            .last()
            .map(|item| js::first(&item.text, LAST_WORDS))
            .unwrap_or_default(),
        owner_questions: owner_questions(&turn_ends, 0),
        files_changed,
        ..Metrics::default()
    }
}

/// The chief's newest message that ended a turn, if it asked the owner anything.
pub fn asking_turn_end<'a>(items: &'a [Item], ends: &HashSet<String>) -> Option<&'a Item> {
    items
        .iter()
        .rfind(|item| item.role == "assistant" && ends_turn(item, ends))
        .filter(|end| count_questions(&end.text) > 0)
}

/// The numbers of a bare run as its report keeps them: those a bare run has, in
/// the order the reports of the other arms have them, the files a scenario looks at
/// too ([`Metrics::with_files`]).
pub fn report_value(metrics: &Metrics) -> Value {
    metrics.with_files(json!({
        "chief": metrics.chief,
        "tasks": [],
        "parallel": 0,
        "advice": 0,
        "reviews": 0,
        "questionsOnBoard": 0,
        "askRefused": 0,
        "notesToHuman": [],
        "notesText": "",
        "longestResult": { "worker": 0, "advisor": 0, "reviewer": 0 },
        "ownerMessages": metrics.owner_messages,
        "taskCount": 0,
        "chiefTurns": metrics.chief_turns,
        "chiefEdits": metrics.chief_edits,
        "chiefLastWords": metrics.chief_last_words,
        "ownerQuestions": owner_questions_value(&metrics.owner_questions),
        "filesChanged": metrics.files_changed,
    }))
}

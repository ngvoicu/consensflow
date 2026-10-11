//! What the model's answers read as. A final answer of Codex's holds markup
//! for Codex's own screen (a file citation, a suggested follow-up), which
//! whoever reads it here, a chief given a worker's result, reads as text: the
//! path, the label. The turn is still proven by the rollout's own words, which
//! `task_complete` names, and only what a look hands out is rendered. Every
//! path here is made up.

use super::*;

/// The model's message, `phase` its kind of answer.
fn model(id: &str, words: &str, phase: &str) -> Value {
    response(json!({
        "type": "message", "role": "assistant", "id": id, "phase": phase,
        "content": [{ "type": "output_text", "text": words }],
        "internal_chat_message_metadata_passthrough": { "turn_id": "t1" },
    }))
}

/// The same message as Codex's own item of it.
fn codex(id: &str, words: &str, phase: &str) -> Value {
    event(json!({
        "type": "item_completed", "turn_id": "t1",
        "item": { "type": "AgentMessage", "id": id, "phase": phase,
                  "content": [{ "type": "Text", "text": words }] },
    }))
}

fn finished(words: &str) -> Value {
    event(json!({ "type": "task_complete", "turn_id": "t1", "last_agent_message": words }))
}

/// The assistant's items of what `records` say: what each reads as.
fn assistant(record: &Record) -> Vec<(&str, &str)> {
    record
        .items
        .iter()
        .filter(|item| item.role == Role::Assistant)
        .map(|item| (&*item.id, &*item.text))
        .collect()
}

const RAW: &str = "The report is in :codex-file-citation{path=\"/srv/demo/out/report.pdf\" purpose=\"source\"}.\n\n:codex-followup[Check the totals]{prompt=\"Check the totals against the ledger\"}";
const READ: &str = "The report is in /srv/demo/out/report.pdf.\n\nCheck the totals";

#[test]
fn the_markup_of_a_final_answer_reads_as_its_path_and_its_label_and_the_turn_is_still_settled() {
    // As Codex writes the answer: the model's message and its own item of it,
    // and the end of the turn naming the words as the rollout holds them.
    let record = read(&[
        started(json!("t1")),
        model("msg_z", RAW, "final_answer"),
        codex("msg_z", RAW, "final_answer"),
        finished(RAW),
    ])
    .unwrap();
    assert_eq!(assistant(&record), [("msg_z", READ)]);
    assert!(record.items[0].complete);
    assert_eq!(
        record.settlement,
        Settlement::Settled,
        "the final answer is proven by the words the rollout holds, not by what they read as"
    );

    // Codex's own item alone, which is what proves a final answer.
    let record = read(&[
        started(json!("t1")),
        codex("item_z", RAW, "final_answer"),
        finished(RAW),
    ])
    .unwrap();
    assert_eq!(assistant(&record), [("item_z", READ)]);
    assert_eq!(record.settlement, Settlement::Settled);

    // The model's message alone reads as text too, and proves nothing, as it never did.
    let record = read(&[
        started(json!("t1")),
        model("msg_z", RAW, "final_answer"),
        finished(RAW),
    ])
    .unwrap();
    assert_eq!(assistant(&record), [("msg_z", READ)]);
    assert_eq!(record.settlement, Settlement::Unknown);
}

#[test]
fn the_progress_notes_of_a_turn_read_as_text_too_and_stay_marked_as_notes() {
    let record = read(&[
        started(json!("t1")),
        codex(
            "item_a",
            "I'll check :codex-file-citation{path=\"/srv/demo/src/lib.rs\"} first.",
            "commentary",
        ),
        codex("item_z", RAW, "final_answer"),
        finished(RAW),
    ])
    .unwrap();
    assert_eq!(
        assistant(&record),
        [
            ("item_a", "I'll check /srv/demo/src/lib.rs first."),
            ("item_z", READ)
        ]
    );
    assert!(record.items[0].commentary && !record.items[1].commentary);
}

#[test]
fn what_is_not_the_models_stays_as_it_was_written() {
    // The brief a worker was given, and what a tool printed, may hold the
    // markup as text: neither is the model's answer.
    let brief = "[ConsensFlow m-1 · T-1 · task from @chief]\nExplain :codex-file-citation{path=\"/srv/demo/a.rs\"}";
    let record = read(&[
        started(json!("t1")),
        response(json!({
            "type": "message", "role": "user", "id": "msg_in",
            "content": [{ "type": "input_text", "text": brief }],
            "internal_chat_message_metadata_passthrough": { "turn_id": "t1" },
        })),
        response(json!({ "type": "function_call", "call_id": "c1", "name": "shell" })),
        response(json!({
            "type": "function_call_output", "call_id": "c1", "id": "out-c1",
            "output": ":codex-file-citation{path=\"/srv/demo/a.rs\"}",
        })),
        codex("item_z", "Done.", "final_answer"),
        finished("Done."),
    ])
    .unwrap();
    let texts: Vec<(Role, &str)> = record
        .items
        .iter()
        .map(|item| (item.role, &*item.text))
        .collect();
    assert_eq!(
        texts,
        [
            (Role::User, brief),
            (Role::Tool, ":codex-file-citation{path=\"/srv/demo/a.rs\"}"),
            (Role::Assistant, "Done."),
        ]
    );
}

#[test]
fn an_answer_that_quotes_the_markup_as_code_keeps_it_so() {
    let quoted = "Write `:codex-file-citation{path=\"/srv/demo/a.rs\"}` to cite a file.";
    let record = read(&[
        started(json!("t1")),
        codex("item_z", quoted, "final_answer"),
        finished(quoted),
    ])
    .unwrap();
    assert_eq!(assistant(&record), [("item_z", quoted)]);
    assert_eq!(record.settlement, Settlement::Settled);
}

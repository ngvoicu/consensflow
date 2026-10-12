//! The question door, live: a worker asks through its harness's own question tool;
//! the door puts the question in the chief's inbox; the chief answers with `cf
//! answer`; the door hands the answer back and the worker finishes. The native
//! daemon's receipt (design §1.2, §1.6) is held to as well: the answer was claimed
//! by the harness's door and acknowledged `received: true`, which is what the receipt
//! `{"door": true}` is, and never pasted; its task waited, then worked, then ended.

use std::time::Instant;

use cf_e2e::live::agents;
use cf_e2e::live::members::{moves, tier_flag, waited_then_worked, Seen};
use cf_e2e::Result;
use serde_json::{json, Value};

use crate::baseline::Worker;
use crate::bench::{first_chars, seconds_since, took, Bench};

/// What the human types to the chief to have it give a worker a task that asks a
/// question with the worker's tool, and to answer it with a colour of its own
/// choosing.
pub fn question_brief(tier: &str, tool: &str) -> String {
    format!(
        "Run exactly this command in your shell, then reply with one line:\ncf task add {} \"Use your {tool} to ask which colour to use, with the options red and blue. Once it is answered, reply with exactly one line: COLOUR=<the answer>\"\nWhen its question reaches you, choose a colour yourself and answer it with cf answer.",
        tier_flag(tier).join(" ")
    )
}

/// Whether an answer was received at its door: the receipt says so, the door
/// claimed it, nothing pasted it and it took no attempt, and its task waited and then
/// worked. What was seen, as a detail.
pub fn received_at_its_door(answer: &Value, seen: &Seen, moved: &[String]) -> (bool, Value) {
    let ok = answer["receipt"]["door"] == true
        && !seen.claimed.is_empty()
        && seen.begun.is_empty()
        && answer["attempts"] == 0
        && waited_then_worked(moved);
    (
        ok,
        json!({
            "receipt": answer["receipt"],
            "claimed": seen.claimed.len(),
            "pasted": seen.begun.len(),
            "attempts": answer["attempts"],
            "task": moved.join(" → "),
        }),
    )
}

/// Runs the question door of `worker`, which has a tool to ask with.
pub fn run(bench: &mut Bench, worker: &Worker) -> Result<()> {
    let Some(tool) = agents::question_tool(worker.harness) else {
        return Ok(());
    };
    let (name, agent) = (worker.name(), worker.agent());
    let started = Instant::now();
    let told = bench.tell(&question_brief(&worker.tier, tool));
    let question = if told {
        bench.wait(&format!("{name}'s question in the chief's inbox"), || {
            Ok(bench.chief_inbox()?.into_iter().find(|message| {
                message["kind"] == "question"
                    && message["sender"]
                        .as_str()
                        .is_some_and(|sender| sender.starts_with(&agent))
                    && !message["questions"].is_null()
            }))
        })?
    } else {
        None
    };
    let detail = match &question {
        Some(question) => took(
            seconds_since(started),
            json!({ "options": question["questions"][0]["options"]
                .as_array()
                .map(|options| options.iter().map(|option| option["label"].clone()).collect::<Vec<_>>()) }),
        ),
        None => took(
            seconds_since(started),
            json!({
                "chief": bench.chief_activity()?,
                "worker": bench.members.lane(&agent)?["activity"],
                "output": bench.output_of(&agent, 1_200)?,
            }),
        ),
    };
    bench.record(
        &format!("{name}-question-asked"),
        question.is_some(),
        &detail,
    );
    let Some(question) = question else {
        return Ok(());
    };
    let sender = question["sender"].as_str().unwrap_or_default().to_owned();
    let answer = bench.wait("the chief's answer to be read at the door", || {
        Ok(bench
            .members
            .inbox(Some(&sender))?
            .into_iter()
            .find(|message| message["replyTo"] == question["id"])
            .filter(|found| found["state"] == "read"))
    })?;
    let detail = match &answer {
        Some(answer) => took(
            seconds_since(started),
            json!({ "choices": answer["choices"], "from": answer["sender"] }),
        ),
        None => took(
            seconds_since(started),
            json!({ "chief": bench.chief_activity()?, "output": bench.chief_output(1_200) }),
        ),
    };
    bench.record(
        &format!("{name}-question-answered-by-chief"),
        answer.is_some(),
        &detail,
    );

    let task_number = question["taskNumber"].as_i64().unwrap_or_default();
    let done = bench.wait(&format!("{name}'s task to be done"), || {
        Ok(bench
            .task_of(&agent, task_number)?
            .filter(|task| task["state"] == "done"))
    })?;
    let result = if done.is_some() {
        bench
            .chief_inbox()?
            .into_iter()
            .find(|message| message["kind"] == "result" && message["taskNumber"] == task_number)
    } else {
        None
    };
    let label = answer
        .as_ref()
        .and_then(|answer| answer["choices"][0][0].as_str())
        .map(str::to_owned);
    let body = result
        .as_ref()
        .and_then(|result| result["body"].as_str())
        .unwrap_or_default()
        .to_owned();
    let returned = label.as_deref().is_some_and(|label| {
        body.to_lowercase()
            .contains(&format!("colour={}", label.to_lowercase()))
    });
    let detail = if result.is_some() {
        took(
            seconds_since(started),
            json!({ "result": first_chars(&body, 80), "label": label }),
        )
    } else {
        took(
            seconds_since(started),
            json!({
                "state": bench.task_of(&agent, task_number)?.map(|task| task["state"].clone()),
                "output": bench.output_of(&agent, 1_200)?,
            }),
        )
    };
    bench.record(
        &format!("{name}-question-returned-to-worker"),
        result.is_some() && returned,
        &detail,
    );

    // The native daemon's receipt: the answer was claimed by the harness's door and
    // acknowledged, and never pasted; its task waited, then worked, then ended.
    if let Some(answer) = answer {
        let events = bench.members.events();
        let seen = Seen::of(&events, answer["id"].as_i64().unwrap_or(-1));
        let moved = moves(&events, task_number);
        let (ok, detail) = received_at_its_door(&answer, &seen, &moved);
        bench.record(&format!("{name}-answer-received-at-its-door"), ok, &detail);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn events() -> Vec<Value> {
        vec![
            json!({ "kind": "task.state", "data": { "task": 4, "to": "working" } }),
            json!({ "kind": "task.state", "data": { "task": 4, "to": "waiting" } }),
            json!({ "kind": "delivery.claimed", "data": { "message": 9 } }),
            json!({ "kind": "message.read", "data": { "message": 9 } }),
            json!({ "kind": "task.state", "data": { "task": 4, "to": "working" } }),
        ]
    }

    fn answer() -> Value {
        json!({ "id": 9, "receipt": { "door": true }, "attempts": 0 })
    }

    #[test]
    fn the_human_has_the_chief_give_a_task_that_asks_with_the_workers_tool_and_answer_it() {
        let brief = question_brief("light", "question tool");
        assert_eq!(
            brief,
            "Run exactly this command in your shell, then reply with one line:\n\
             cf task add --tier light \"Use your question tool to ask which colour to use, with the options red and blue. Once it is answered, reply with exactly one line: COLOUR=<the answer>\"\n\
             When its question reaches you, choose a colour yourself and answer it with cf answer."
        );
    }

    #[test]
    fn an_answer_is_received_at_its_door_when_claimed_acknowledged_never_pasted_and_the_task_went_on(
    ) {
        let events = events();
        let seen = Seen::of(&events, 9);
        let moved = moves(&events, 4);
        let (ok, detail) = received_at_its_door(&answer(), &seen, &moved);
        assert!(ok, "{detail}");
        assert_eq!(
            detail,
            json!({ "receipt": { "door": true }, "claimed": 1, "pasted": 0, "attempts": 0,
                    "task": "working → waiting → working" })
        );
    }

    #[test]
    fn an_answer_that_was_pasted_took_an_attempt_or_had_no_door_receipt_was_not_received_there() {
        let events = events();
        let seen = Seen::of(&events, 9);
        let moved = moves(&events, 4);
        let with = |change: &dyn Fn(&mut Value)| {
            let mut answer = answer();
            change(&mut answer);
            received_at_its_door(&answer, &seen, &moved).0
        };
        assert!(!with(&|answer| answer["receipt"] = Value::Null));
        assert!(!with(&|answer| answer["attempts"] = json!(1)));
        // Pasted: the daemon began a delivery of it.
        let mut pasted = events.clone();
        pasted.push(json!({ "kind": "delivery.begun", "data": { "message": 9 } }));
        assert!(!received_at_its_door(&answer(), &Seen::of(&pasted, 9), &moved).0);
        // Never claimed, or a task that never waited.
        let unclaimed: Vec<Value> = events
            .iter()
            .filter(|event| event["kind"] != "delivery.claimed")
            .cloned()
            .collect();
        assert!(!received_at_its_door(&answer(), &Seen::of(&unclaimed, 9), &moved).0);
        let never_waited = ["working".to_owned()];
        assert!(!received_at_its_door(&answer(), &seen, &never_waited).0);
    }
}

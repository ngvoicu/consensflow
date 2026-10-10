//! A native question, end to end through the real pane host (TEST-CF1-15): a
//! fake Claude worker asks through its question tool, the hook in its settings
//! file puts the question on the board, the chief answers it with `cf answer`
//! as its inbox told it to, and the hook hands the answer back into the tool
//! call, so the worker goes on and finishes its task. Nothing is pasted into
//! the worker's window for that.

use cf_e2e::rig::Project;
use serde_json::{json, Value};

use crate::{rig, secs, Outcome};

#[test]
fn a_workers_question_with_options_goes_to_the_chiefs_inbox_and_its_answer_returns_through_the_hook(
) -> Outcome {
    let rig = rig()?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    let questions = json!([{
        "question": "Which colour?",
        "header": "Colour",
        "options": [{ "label": "red" }, { "label": "blue", "description": "REPLY blue" }],
        "multiSelect": false,
    }]);
    rig.tell(
        project.id(),
        &format!(
            "DISPATCH --tier {} ASK {questions}",
            added["member"]["tier"].as_str().unwrap_or_default()
        ),
    )?;

    // The task waits only until the chief answers, which the fake chief does at
    // once, so the proof is the thread the ledger kept, not a glimpse of the state.
    rig.wait_for(
        "the question to be delivered to the chief",
        secs(30),
        || {
            Ok(project
                .inbox("chief")?
                .iter()
                .any(|m| m["kind"] == "question" && m["state"] == "delivered"))
        },
    )?;
    let question = project
        .inbox("chief")?
        .into_iter()
        .find(|m| m["kind"] == "question")
        .unwrap_or(Value::Null);
    assert!(
        question["sender"]
            .as_str()
            .is_some_and(|sender| sender.starts_with("worker-")),
        "the worker's session asked: {}",
        question["sender"]
    );
    assert_eq!(
        [
            question["taskNumber"].clone(),
            question["questions"][0]["options"][1]["label"].clone()
        ],
        [json!(1), json!("blue")]
    );
    assert_eq!(
        question["body"],
        "Colour: Which colour?\n- red\n- blue: REPLY blue"
    );

    rig.wait_for("the worker's task to be done", secs(30), || {
        Ok(project.lane("worker")?["tasks"][0]["state"] == "done")
    })?;
    let session = project.lane("worker")?["participant"]["handle"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    let answer = project
        .inbox(&session)?
        .into_iter()
        .find(|m| m["kind"] == "answer")
        .unwrap_or(Value::Null);
    assert_eq!(
        [
            answer["state"].clone(),
            answer["choices"].clone(),
            answer["sender"].clone(),
            answer["body"].clone()
        ],
        [
            json!("read"),
            json!([["blue"]]),
            json!("chief"),
            json!("Colour: blue")
        ],
        "the answer is collected by the hook, never delivered as text"
    );
    // What read it: the hook's own receipt once it handed the answer over (a hook
    // that says so before its worker's turn goes on).
    assert_eq!(answer["receipt"], json!({ "door": true }));
    let task = project.task(1)?;
    let result = task["messages"]
        .as_array()
        .and_then(|messages| messages.iter().find(|m| m["kind"] == "result"))
        .cloned()
        .unwrap_or(Value::Null);
    assert_eq!(result["body"], "answered: blue");
    rig.wait_for("the result to be delivered to the chief", secs(30), || {
        Ok(project
            .inbox("chief")?
            .iter()
            .any(|m| m["kind"] == "result" && m["state"] == "delivered"))
    })?;
    rig.close()?;
    Ok(())
}

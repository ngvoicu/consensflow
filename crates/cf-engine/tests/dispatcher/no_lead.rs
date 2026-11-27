//! What the dispatcher does in a project with one chief and no lead, pinned before leads
//! are built (the leads design, step 0): where a question recognised at a turn's end
//! goes, what main does with a chief's window that exits by itself with a task in hand,
//! and who is told when a message's arrival cannot be proved. A later step must leave
//! every one of these as it is.

use cf_engine::delivery_text::marker_of;
use cf_engine::testing::Context;
use cf_harness::records::Role;
use cf_ledger::NewTask;
use serde_json::{json, Value};

/// The kit's arrival window, and a moment past it.
const PAST_ARRIVAL: i64 = 31_000;

const QUESTION: &str =
    "Should I continue with the live runs, or stop here so you can review the findings first?";

/// The ledger's events of `kind`, as it logged their data.
fn logged(context: &Context, kind: &str) -> Vec<Value> {
    context
        .recorder
        .events()
        .into_iter()
        .filter(|event| event["seam"] == "event" && event["event"]["kind"] == kind)
        .map(|event| event["event"]["data"].clone())
        .collect()
}

/// What ConsensFlow itself said to `handle`: the notes with no sender, oldest first.
fn told(context: &Context, project: i64, handle: &str) -> Vec<String> {
    let mut said: Vec<_> = context
        .inbox(context.id(project, handle))
        .into_iter()
        .filter(|message| message.sender.is_none() && message.kind == "note")
        .collect();
    said.sort_by_key(|message| message.id);
    said.into_iter().map(|message| message.body).collect()
}

/// A task the human gave `to` by name, as the page gives one.
fn human_gives(context: &Context, project: i64, to: &str, body: &str) -> i64 {
    context
        .create_task(
            project,
            NewTask {
                from: "human".to_owned(),
                to: Some(to.to_owned()),
                body: body.to_owned(),
                ..NewTask::default()
            },
        )
        .task
        .number
}

#[test]
fn a_question_a_turn_ends_on_goes_to_the_chief_whoever_gave_the_task() {
    for requester in ["chief", "human"] {
        let context = Context::new();
        let project = context.with_staff(&["zeus"]);
        context.pass().unwrap();
        let number = if requester == "human" {
            human_gives(&context, project.id, "zeus", "Parser")
        } else {
            context.give(project.id, "zeus", "Parser").task.number
        };
        context.pass().unwrap();
        context.pass().unwrap();
        context.adapter.answer("zeus", QUESTION);
        context.pass().unwrap();
        assert_eq!(context.task(project.id, number).task.state, "waiting");
        let questions = |handle: &str| {
            context
                .inbox(context.id(project.id, handle))
                .into_iter()
                .filter(|message| message.kind == "question")
                .count()
        };
        assert_eq!(
            (questions("chief"), questions("human")),
            (1, 0),
            "{requester}"
        );
    }
}

#[test]
fn a_chief_whose_window_exits_by_itself_with_the_humans_task_at_work_has_its_last_words_taken_as_the_result(
) {
    // Main's behaviour, kept on purpose for the chief: the exit's collector finds the task in
    // hand of any participant, the chief included (`dispatcher.rs`, `try_collect_at_exit`), and
    // collects a settled answer as that task's result. The design keeps this for the chief and
    // adds only a lead to its guard; it is pinned here as it is, a fault or not.
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    let number = human_gives(&context, project.id, "chief", "Plan the week");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.task(project.id, number).task.state, "working");
    context.adapter.answer("chief", "The plan is ready.");
    context.host.exits_showing(Some(0), None, Some(&["done"]));
    context.exit("chief");
    let thread = context.task(project.id, number);
    assert_eq!(thread.task.state, "done");
    let results: Vec<_> = thread
        .messages
        .iter()
        .filter(|message| message.kind == "result")
        .map(|message| (message.recipient.as_str(), message.body.as_str()))
        .collect();
    assert_eq!(results, [("human", "The plan is ready.")]);
    assert_eq!(
        context.project(project.id).state,
        "suspended",
        "a chief's exit closes the project"
    );
}

/// The window of `handle` shows nothing it is pasted, and a user item quotes message `id`'s
/// header after the hand-over: the proof of its arrival cannot be made, as a record that
/// holds the header behind words gives it.
fn header_behind_words(context: &Context, handle: &str, id: i64) {
    let words = format!("Why is \"{}· note from @x]\" still queued?", marker_of(id));
    let said = context.adapter.item(Role::User, &words);
    context.adapter.with(handle, |agent| agent.items.push(said));
}

/// Passes until the message's arrival has been given up as unproven.
fn past_the_arrival(context: &Context) {
    context.pass().unwrap();
    context.advance(PAST_ARRIVAL);
    context.pass().unwrap();
    context.pass().unwrap();
}

const COULDNT: &str = "'s arrival couldn't be proven from the record; it was not pasted again.";
const COULDNT_YOURS: &str =
    "'s arrival couldn't be proven from your window's record; it was not pasted again.";
// The human's brief to a member's window is not pinned here: a fresh window's first brief
// arrives with its launch, and a follow-up for a session whose window closed waits paused,
// so no stand-in reaches an unproven brief to a member (reported with the baselines).

/// Notes of `handle` that say an arrival was not proved, in order.
fn unproven_notes(context: &Context, project: i64, handle: &str) -> Vec<String> {
    told(context, project, handle)
        .into_iter()
        .filter(|body| body.contains(COULDNT) || body.contains(COULDNT_YOURS))
        .collect()
}

#[test]
fn a_brief_the_human_gave_the_chief_that_arrived_unproven_tells_the_human_once_and_not_the_chief() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    context.adapter.with("chief", |agent| agent.arrive = false);
    let number = human_gives(&context, project.id, "chief", "Plan the week");
    context.pass().unwrap();
    let brief = context.task(project.id, number).messages[0].id;
    assert_eq!(context.message(brief).state, "delivering");
    header_behind_words(&context, "chief", brief);
    past_the_arrival(&context);
    assert_eq!(context.message(brief).receipt["unproven"], json!(true));
    assert_eq!(
        unproven_notes(&context, project.id, "human"),
        [format!(
            "m-{brief}{COULDNT} @chief can read it with `cf inbox read m-{brief}`."
        )],
        "its requester is told, and once"
    );
    assert_eq!(
        unproven_notes(&context, project.id, "chief"),
        Vec::<String>::new()
    );
}

#[test]
fn a_result_to_the_chief_that_arrived_unproven_tells_the_chief_once_and_no_one_else() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    context.adapter.with("chief", |agent| agent.arrive = false);
    context.adapter.answer("zeus", "Parsed");
    context.pass().unwrap();
    let thread = context.task(project.id, 1);
    let result = thread
        .messages
        .iter()
        .find(|message| message.kind == "result")
        .expect("a result")
        .id;
    context.pass().unwrap();
    assert_eq!(context.message(result).state, "delivering");
    header_behind_words(&context, "chief", result);
    past_the_arrival(&context);
    assert_eq!(context.message(result).receipt["unproven"], json!(true));
    assert_eq!(
        unproven_notes(&context, project.id, "chief"),
        [format!(
            "m-{result}{COULDNT} @chief can read it with `cf inbox read m-{result}`."
        )]
    );
    assert_eq!(
        unproven_notes(&context, project.id, "human"),
        Vec::<String>::new()
    );
    assert_eq!(
        unproven_notes(&context, project.id, "zeus"),
        Vec::<String>::new()
    );
    assert_eq!(logged(&context, "delivery.unproven").len(), 1);
}

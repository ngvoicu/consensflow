//! A task message that arrived, and that its window then took back out of its
//! conversation (Claude, stopped before a word of its answer), was not
//! received: it is kept for the window again, as a task paused before its
//! brief arrived keeps the brief, and the words that resume the task carry it
//! with what its paste carried, whichever came first.

use cf_ledger::LedgerError;
use serde_json::{json, Value};

use crate::fixture::{ids, world, World};

const REASON: &str =
    "@zeus stopped before a word of its answer and took it back out of its conversation";

/// A task given to zeus whose brief arrived and which was then paused: its
/// number and its brief's id.
fn paused_after_its_brief(w: &mut World) -> (i64, i64) {
    let created = w.give("zeus", "Parser");
    let (task, brief) = (created.task.number, created.message.expect("a brief").id);
    w.deliver(brief);
    assert_eq!(w.state(task), "working");
    w.pause(task);
    (task, brief)
}

/// The code the ledger refused with.
fn refused(result: Result<cf_ledger::MessageView, LedgerError>) -> Option<&'static str> {
    result.expect_err("a refusal").code()
}

#[test]
fn a_brief_its_window_took_back_is_kept_again_and_the_words_that_resume_the_task_carry_it() {
    let mut w = world();
    let (task, brief) = paused_after_its_brief(&mut w);

    let taken = w.ledger.take_back(brief, REASON).expect("taken back");
    assert_eq!(
        taken.state, "queued",
        "kept for its window, as a brief paused before it arrived is"
    );
    assert_eq!(taken.reason.as_deref(), Some(REASON));
    assert_eq!(
        taken.receipt,
        Value::Null,
        "what its record showed is no receipt"
    );
    assert_eq!(taken.delivered_at, None);
    assert_eq!(w.state(task), "paused", "the task is not moved by it");
    assert_eq!(
        w.next("zeus"),
        None,
        "a paused task's window is given nothing, the brief included"
    );

    let carrier = w.resume(task, "Carry on").message.expect("its words");
    assert_eq!(
        carrier.body, "Resumed: Carry on",
        "the brief is a row the window keeps: the body does not rebuild it"
    );
    assert_eq!(w.next("zeus"), Some(carrier.id), "one paste, the carrier's");
    let begun = w.begin(carrier.id);
    assert_eq!(ids(&begun.carried), [brief], "and the brief rides in it");
    w.confirm(carrier.id);
    assert_eq!(w.states(&[brief, carrier.id]), ["delivered", "delivered"]);
    assert_eq!(w.receipt(brief), json!({ "carrier": carrier.id }));
    assert_eq!(
        w.message(brief).reason.as_deref(),
        Some(REASON),
        "and the thread keeps why it came a second time"
    );
    assert_eq!(
        w.state(task),
        "working",
        "received with the words that carried it"
    );
}

#[test]
fn words_that_resumed_the_task_before_the_stop_was_paid_take_the_message_in_when_it_is_taken_back()
{
    let mut w = world();
    let (task, brief) = paused_after_its_brief(&mut w);
    // The human resumed while the window was still being stopped: the brief
    // was in the window's conversation as far as anyone knew.
    let carrier = w.resume(task, "Carry on").message.expect("its words");
    assert_eq!(carrier.body, "Resumed: Carry on");
    assert_eq!(w.state(task), "queued");

    w.ledger.take_back(brief, REASON).expect("taken back");
    assert_eq!(
        w.next("zeus"),
        Some(carrier.id),
        "it joined the words that wait: they are the head, and it rides in them"
    );
    let begun = w.deliver(carrier.id);
    assert_eq!(ids(&begun.carried), [brief]);
    assert_eq!(w.states(&[brief, carrier.id]), ["delivered", "delivered"]);
    assert_eq!(w.state(task), "working");
}

#[test]
fn a_resume_taken_back_with_what_it_carried_is_kept_again_with_it_and_the_next_words_carry_each_once(
) {
    let mut w = world();
    let created = w.give("zeus", "Parser");
    let (task, brief) = (created.task.number, created.message.expect("a brief").id);
    w.deliver(brief);
    let question = w.ask("zeus", task, "Which format?");
    w.pause(task);
    let answer = w.answer(question.id, "JSON");
    let first = w.resume(task, "Carry on").message.expect("its words");
    let begun = w.deliver(first.id);
    assert_eq!(ids(&begun.carried), [answer.id]);
    assert_eq!(w.state(task), "working");
    w.pause(task);

    // The window took the whole paste back: the words and the answer in them.
    let taken = w.ledger.take_back(first.id, REASON).expect("taken back");
    assert_eq!(taken.state, "queued");
    assert_eq!(w.states(&[first.id, answer.id]), ["queued", "queued"]);
    assert_eq!(w.receipt(answer.id), Value::Null);
    assert_eq!(
        w.states(&[brief]),
        ["delivered"],
        "the brief was in the conversation before it, and is still"
    );

    let second = w.resume(task, "Use JSON").message.expect("its words");
    assert_eq!(
        second.body, "Resumed: Use JSON",
        "the window has the brief: only the rows it lost ride in the paste"
    );
    let begun = w.deliver(second.id);
    assert_eq!(
        ids(&begun.carried),
        [answer.id, first.id],
        "each once: the chief's words and the answer, in the order of their ids"
    );
    assert_eq!(
        w.states(&[answer.id, first.id, second.id]),
        ["delivered", "delivered", "delivered"]
    );
    assert_eq!(w.state(task), "working");
}

#[test]
fn the_daemons_fixed_words_taken_back_are_superseded_by_the_next_and_what_they_carried_goes_on() {
    let mut w = world();
    let created = w.give("zeus", "Parser");
    let (task, brief) = (created.task.number, created.message.expect("a brief").id);
    w.deliver(brief);
    let question = w.ask("zeus", task, "Which format?");
    w.hold(task);
    let answer = w.answer(question.id, "JSON");
    let first = w.daemon_resumes(task).message.expect("its words");
    w.deliver(first.id);
    w.hold(task);

    w.ledger.take_back(first.id, REASON).expect("taken back");
    let second = w.daemon_resumes(task).message.expect("its words");
    assert_eq!(
        w.states(&[first.id]),
        ["cancelled"],
        "two sets of the same words say what one does"
    );
    assert_eq!(
        w.message(first.id).reason.as_deref(),
        Some(format!("superseded by m-{}", second.id).as_str())
    );
    let begun = w.deliver(second.id);
    assert_eq!(ids(&begun.carried), [answer.id]);
    assert_eq!(w.state(task), "working");
}

#[test]
fn only_a_task_message_that_arrived_on_its_own_can_be_taken_back_and_with_a_reason() {
    let mut w = world();
    let created = w.give("zeus", "Parser");
    let (task, brief) = (created.task.number, created.message.expect("a brief").id);
    assert_eq!(
        refused(w.ledger.take_back(brief, REASON)),
        Some("invalid-transition"),
        "a message that did not arrive was not received"
    );
    w.deliver(brief);
    assert!(
        w.ledger.take_back(brief, "  ").is_err(),
        "and the thread says why, or it is not told"
    );
    assert_eq!(
        w.states(&[brief]),
        ["delivered"],
        "a refusal leaves it as it was"
    );

    // An answer that arrived is not a task message.
    let question = w.ask("zeus", task, "Which format?");
    let answer = w.answer(question.id, "JSON");
    w.deliver(answer.id);
    assert_eq!(
        refused(w.ledger.take_back(answer.id, REASON)),
        Some("invalid-transition")
    );

    // Words that arrived in the paste of other words are not a message of
    // their own: the paste is what the window took back.
    w.pause(task);
    let older = w.resume(task, "One").message.expect("its words");
    // The human's pause withdraws nothing of the chief's words.
    w.ledger
        .pause_task(w.project, task, Some("human"), None)
        .expect("paused");
    let newer = w.resume(task, "Two").message.expect("its words");
    w.deliver(newer.id);
    assert_eq!(
        w.states(&[older.id]),
        ["delivered"],
        "folded into the newer, and delivered with it"
    );
    assert_eq!(
        refused(w.ledger.take_back(older.id, REASON)),
        Some("invalid-transition")
    );
}

#[test]
fn what_is_taken_back_is_in_the_log_with_the_rows_kept_with_it() {
    let mut w = world();
    let created = w.give("zeus", "Parser");
    let (task, brief) = (created.task.number, created.message.expect("a brief").id);
    w.deliver(brief);
    let question = w.ask("zeus", task, "Which format?");
    w.pause(task);
    let answer = w.answer(question.id, "JSON");
    let first = w.resume(task, "Carry on").message.expect("its words");
    w.deliver(first.id);
    w.pause(task);
    w.ledger.take_back(first.id, REASON).expect("taken back");

    let said: Vec<(i64, String, Vec<i64>)> = w
        .ledger
        .events(w.project, 0, 500)
        .expect("the log")
        .iter()
        .filter(|event| event.kind == "delivery.taken-back")
        .map(|event| {
            (
                event.data["message"].as_i64().expect("a message"),
                event.data["reason"].as_str().expect("a reason").to_owned(),
                event.data["kept"]
                    .as_array()
                    .expect("the rows kept")
                    .iter()
                    .map(|id| id.as_i64().expect("an id"))
                    .collect(),
            )
        })
        .collect();
    assert_eq!(said, [(first.id, REASON.to_owned(), vec![answer.id])]);
}

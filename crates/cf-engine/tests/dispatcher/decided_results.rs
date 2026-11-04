//! A result the chief decides on before it is given it is never pasted: the
//! chief's turn of 2026-10-08 read T-9's result, which waited behind that
//! turn, accepted the task and put T-10 after it, and was then given the
//! result with "Decide with: cf task accept T-9 …". The ledger withdraws the
//! result in the decision's own transaction, so the dispatcher, which asks the
//! ledger for the head of the chief's queue before it hands anything over, has
//! nothing to paste. A result already being pasted when the decision comes is
//! in the chief's window and arrives, and one it was given stays given. Node
//! pastes the result after the decision, so none of these is held to a Node
//! trace; the ported tests it moved are named in `traces`.

use cf_engine::delivery_text::{delivery_text, marker_of};
use cf_engine::testing::Context;
use cf_harness::records::Role;
use cf_ledger::{MessageView, NewTask};
use cf_proto::cf_word::CfWord;

use crate::fixtures::{hold_notes, Tiers};

/// T-1 for zeus's window, which has taken it in.
fn working(context: &Context) -> Tiers<'_> {
    let tiers = Tiers::new(context, &["zeus"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working");
    tiers
}

/// The result of T-1, from the chief's inbox as the ledger has it now.
fn result_of(context: &Context, tiers: &Tiers<'_>) -> MessageView {
    context
        .inbox(tiers.id("chief"))
        .into_iter()
        .find(|message| message.kind == "result")
        .expect("a result for the chief")
}

/// What the window of `handle` was given as messages, oldest first.
fn given(context: &Context, handle: &str) -> Vec<String> {
    context
        .adapter
        .agent(handle)
        .items
        .iter()
        .filter(|item| item.role == Role::User)
        .map(|item| item.text.to_string())
        .collect()
}

/// The chief is in a turn of its own when zeus finishes T-1: the result waits.
fn finished_in_the_chiefs_turn(context: &Context, tiers: &Tiers<'_>) -> MessageView {
    context.adapter.busy("chief");
    context.adapter.answer("zeus", "Parser done");
    context.pass().unwrap();
    context.pass().unwrap();
    let result = result_of(context, tiers);
    assert_eq!(result.state, "queued", "a busy chief is not interrupted");
    assert_eq!(tiers.task(1).task.state, "done");
    result
}

/// The chief's turn ends, and the dispatcher looks at its window.
fn the_chiefs_turn_ends(context: &Context) {
    context.adapter.with("chief", |agent| agent.settled = true);
    context.pass().unwrap();
    context.pass().unwrap();
}

#[test]
fn pastes_no_result_into_a_chief_that_accepted_its_task_in_the_turn_the_result_waited_behind() {
    let context = Context::new();
    // The host proves each window's program gone, as the app's does: the session's next task
    // goes on with its conversation (`ownership`), as this test is about something else.
    context.host.exits_showing(Some(0), None, None);
    let tiers = working(&context);
    let result = finished_in_the_chiefs_turn(&context, &tiers);

    // In that turn it reads the result, accepts the task, and puts the next
    // task after it.
    context
        .ledger
        .borrow_mut()
        .accept_task(tiers.project.id, 1, "chief")
        .unwrap();
    context.create_task(
        tiers.project.id,
        NewTask {
            from: "chief".to_owned(),
            after: Some(1),
            body: "Now the lexer".to_owned(),
            ..NewTask::default()
        },
    );
    the_chiefs_turn_ends(&context);

    let withdrawn = context.message(result.id);
    assert_eq!(
        (withdrawn.state.as_str(), withdrawn.reason.as_deref()),
        ("cancelled", Some("T-1 was accepted"))
    );
    assert!(
        given(&context, "chief")
            .iter()
            .all(|text| !text.contains(&marker_of(result.id)) && !text.contains("Decide with")),
        "the chief is given nothing to decide again: {:?}",
        given(&context, "chief")
    );
    assert!(
        given(&context, "zeus")
            .iter()
            .any(|text| text.contains("task from @chief]\nNow the lexer")),
        "and the next task went on to its window"
    );
}

/// The same, where the host proves nothing of the exit of the window that did T-1: the result is
/// withdrawn all the same and the chief is given none to decide again, but T-2, a follow-up no
/// window had, is held for the chief's word, not given to a window of zeus's unasked.
#[test]
fn pastes_no_result_into_a_chief_that_accepted_its_task_and_holds_the_next_task_the_window_closed_with_no_proof_on(
) {
    let context = Context::new();
    let tiers = working(&context);
    let result = finished_in_the_chiefs_turn(&context, &tiers);
    context
        .ledger
        .borrow_mut()
        .accept_task(tiers.project.id, 1, "chief")
        .unwrap();
    context.create_task(
        tiers.project.id,
        NewTask {
            from: "chief".to_owned(),
            after: Some(1),
            body: "Now the lexer".to_owned(),
            ..NewTask::default()
        },
    );
    the_chiefs_turn_ends(&context);

    let withdrawn = context.message(result.id);
    assert_eq!(
        (withdrawn.state.as_str(), withdrawn.reason.as_deref()),
        ("cancelled", Some("T-1 was accepted"))
    );
    assert!(
        given(&context, "chief")
            .iter()
            .all(|text| !text.contains(&marker_of(result.id)) && !text.contains("Decide with")),
        "the chief is given nothing to decide again: {:?}",
        given(&context, "chief")
    );
    assert_eq!(
        tiers.task(2).task.state,
        "paused",
        "held for the chief's word"
    );
    assert_eq!(hold_notes(&context, tiers.project.id, 2).len(), 1);
    assert!(
        given(&context, "zeus")
            .iter()
            .all(|text| !text.contains("Now the lexer")),
        "and no window was given the next task unasked"
    );

    context.resume_task(tiers.project.id, 2, "Go on in a window of your own.");
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert!(
        given(&context, "zeus")
            .iter()
            .any(|text| text.contains("Now the lexer")),
        "sent on, it goes to a window"
    );
}

#[test]
fn pastes_no_result_into_a_chief_that_sent_its_task_back_in_the_turn_the_result_waited_behind() {
    let context = Context::new();
    let tiers = working(&context);
    let result = finished_in_the_chiefs_turn(&context, &tiers);

    context
        .ledger
        .borrow_mut()
        .reopen_task(tiers.project.id, 1, "chief", "Handle empty input too")
        .unwrap();
    the_chiefs_turn_ends(&context);

    let withdrawn = context.message(result.id);
    assert_eq!(
        (withdrawn.state.as_str(), withdrawn.reason.as_deref()),
        ("cancelled", Some("T-1 was sent back"))
    );
    assert!(
        given(&context, "chief")
            .iter()
            .all(|text| !text.contains(&marker_of(result.id)) && !text.contains("Decide with")),
        "{:?}",
        given(&context, "chief")
    );
    assert!(
        given(&context, "zeus")
            .iter()
            .any(|text| text.contains("task from @chief]\nHandle empty input too")),
        "and the work sent back went on to its window"
    );
}

#[test]
fn a_result_being_pasted_when_the_chief_accepts_its_task_arrives_all_the_same() {
    let context = Context::new();
    let tiers = working(&context);
    context.adapter.with("chief", |agent| agent.arrive = false);
    context.adapter.answer("zeus", "Parser done");
    context.pass().unwrap();
    context.pass().unwrap();
    let result = result_of(&context, &tiers);
    assert_eq!(
        result.state, "delivering",
        "handed over to the chief's window"
    );

    context
        .ledger
        .borrow_mut()
        .accept_task(tiers.project.id, 1, "chief")
        .unwrap();
    let kept = context.message(result.id);
    assert_eq!(
        (kept.state.as_str(), kept.reason.as_deref()),
        ("delivering", None),
        "it is in the window: the decision takes nothing back"
    );

    // The harness's record shows it after all, and the delivery is proved.
    let shown = context.adapter.item(
        Role::User,
        &delivery_text(&context.message(result.id), &[], &CfWord::bare()),
    );
    context
        .adapter
        .with("chief", |agent| agent.items.push(shown));
    context.pass().unwrap();
    assert_eq!(context.message(result.id).state, "delivered");
}

#[test]
fn a_result_the_chief_was_given_stays_given_when_it_accepts_or_sends_back_its_task() {
    for sends_back in [false, true] {
        let context = Context::new();
        let tiers = working(&context);
        context.adapter.answer("zeus", "Parser done");
        // The result is recorded, pasted, and proved by the chief's record.
        for _ in 0..3 {
            context.pass().unwrap();
        }
        let result = result_of(&context, &tiers);
        assert_eq!(result.state, "delivered", "to an idle chief, at once");

        let mut ledger = context.ledger.borrow_mut();
        if sends_back {
            ledger
                .reopen_task(tiers.project.id, 1, "chief", "Handle empty input too")
                .unwrap();
        } else {
            ledger.accept_task(tiers.project.id, 1, "chief").unwrap();
        }
        drop(ledger);
        context.pass().unwrap();

        let after = context.message(result.id);
        assert_eq!(
            (after.state.as_str(), after.reason.as_deref()),
            ("delivered", None),
            "sent back: {sends_back}"
        );
        assert_eq!(
            given(&context, "chief")
                .iter()
                .filter(|text| text.contains(&marker_of(result.id)))
                .count(),
            1,
            "and it was pasted once"
        );
    }
}

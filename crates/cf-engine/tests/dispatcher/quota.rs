//! The dispatcher watches quota: a member that runs out gives its work back
//! or holds it, an old refusal in the record is history, a delivery in
//! flight is queued again, and a member low on quota takes nothing new.

use cf_engine::testing::Context;
use cf_engine::ActivityState;
use cf_ledger::{NewNote, NewQuestion, NewTask};
use serde_json::json;

use crate::fixtures::{assert_match, exhausted, low, now, placed, soon, Tiers};
use crate::traces::held_to;

const SUITES: &[&str] = &["the dispatcher watches quota"];

#[test]
fn takes_a_task_back_from_a_member_that_ran_out_and_gives_it_to_another_telling_the_requester() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus", "diana"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working");
    let refused = context.host.last("zeus").unwrap().pane;
    context
        .adapter
        .quota("zeus", Some(exhausted(None, Some(&soon(&context, 2)))));
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(
        placed(&tiers.task(1).task),
        ("queued", Some("diana-brisk-birch"))
    );
    assert_match(
        &tiers.task(1).task.body,
        r"Reassigned from @zeus-amber-pine \(ran out of quota after starting\); check the working tree",
    );
    assert_eq!(
        tiers.notes("chief"),
        ["T-1 was taken back from @zeus-amber-pine (ran out of quota after starting) and waits for another standard worker."]
    );
    assert_eq!(tiers.out_until("zeus"), Some(soon(&context, 2)));
    assert!(
        tiers.participant("zeus-amber-pine").is_some(),
        "the session that ran out stays for the human; the member is out until its reset"
    );
    // A harness that waits out its limit (OpenCode) would take the task up
    // again at the reset, beside the member that has it now.
    assert_eq!(
        context.host.killed(),
        [refused],
        "the refused session's window closes with its work"
    );
    held_to(
        context.close(),
        SUITES,
        "takes a task back from a member that ran out and gives it to another, telling the requester",
    );
}

#[test]
fn keeps_a_member_out_only_until_its_reset_though_its_harness_still_shows_the_old_refusal() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus", "diana"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    let refused_at = now(&context);
    // A real transcript keeps its last record: the refusal stays in view
    // until the member gets a turn, which it cannot while it is out.
    context.adapter.quota(
        "zeus",
        Some(exhausted(Some(&refused_at), Some(&soon(&context, 1)))),
    );
    context.adapter.with("zeus", |agent| agent.settled = true);
    context.pass().unwrap();
    context.pass().unwrap();
    assert_match(&tiers.assignee(1), "^diana-");
    context.adapter.answer("diana", "done");
    context.pass().unwrap();

    context.advance(2 * 3_600_000);
    tiers.open_body("Lexer");
    context.pass().unwrap();
    assert_match(&tiers.assignee(2), "^zeus-");
    context.pass().unwrap();
    assert_eq!(
        tiers.task(2).task.state,
        "working",
        "and zeus receives again"
    );
    assert_eq!(
        context
            .dispatcher
            .activity(tiers.id(&tiers.assignee(2)))
            .state,
        ActivityState::Working
    );

    context.adapter.quota(
        "zeus",
        Some(exhausted(Some(&now(&context)), Some(&soon(&context, 1)))),
    );
    context.pass().unwrap();
    assert_eq!(tiers.task(2).task.state, "open", "a fresh refusal counts");
    assert_eq!(tiers.out_until("zeus"), Some(soon(&context, 1)));
    held_to(
        context.close(),
        SUITES,
        "keeps a member out only until its reset, though its harness still shows the old refusal",
    );
}

#[test]
fn queues_a_delivery_in_flight_at_the_refusal_again_and_keeps_a_coordinators_own_tasks_for_after_its_reset(
) {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus", "diana"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    let question = context
        .ledger
        .borrow_mut()
        .ask(
            1,
            &NewQuestion {
                from: Some("zeus-amber-pine".to_owned()),
                to: "chief".to_owned(),
                task: Some(1),
                body: Some("Which?".to_owned()),
                ..NewQuestion::default()
            },
        )
        .unwrap();
    context.adapter.with("zeus", |agent| agent.arrive = false);
    context.adapter.answer("zeus", "asked");
    context.pass().unwrap();
    context.adapter.answer("chief", "This one, replying");
    let answer = context
        .ledger
        .borrow_mut()
        .answer(
            question.id,
            question.recipient_id,
            Some(&json!("This one")),
            None,
        )
        .unwrap();
    context.pass().unwrap();
    let state_of = |message: i64| {
        context
            .ledger
            .borrow()
            .message(message)
            .unwrap()
            .unwrap()
            .state
    };
    assert_eq!(state_of(answer.id), "delivering");
    context
        .adapter
        .quota("zeus", Some(exhausted(Some(&now(&context)), None)));
    context.pass().unwrap();
    assert_eq!(
        (
            tiers.task(1).task.state,
            tiers.task(1).task.assignee,
            state_of(answer.id)
        ),
        ("open".to_owned(), None, "cancelled".to_owned()),
        "the task goes back to the board and the answer in flight goes with it"
    );
    // Not held to Node's recording: what was in flight goes with the task, in
    // the brief of the window that takes it next, once.
    assert!(
        tiers.task(1).task.body.contains(
            "Kept from before, never delivered to @zeus-amber-pine:\n\n(answer m-3 from @chief to m-2 of @zeus-amber-pine: Which?)\nThis one"
        ),
        "{}",
        tiers.task(1).task.body
    );

    // A pass runs every window at once, so the note may reach the chief on
    // this pass or the next; it answers, then takes its own work.
    context.pass().unwrap();
    context.adapter.answer("chief", "noted");
    let own = context
        .create_task(
            1,
            NewTask {
                from: "human".to_owned(),
                to: Some("chief".to_owned()),
                body: "Plan".to_owned(),
                ..NewTask::default()
            },
        )
        .task
        .number;
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(own).task.state, "working");
    context
        .adapter
        .quota("chief", Some(exhausted(Some(&now(&context)), None)));
    context.adapter.with("chief", |agent| agent.settled = true);
    context.pass().unwrap();
    assert_eq!(
        tiers.task(own).task.state,
        "working",
        "a coordinator keeps its task"
    );
    assert_eq!(
        context.dispatcher.activity(tiers.id("chief")).state,
        ActivityState::Out
    );
    let later = context
        .ledger
        .borrow_mut()
        .note(
            1,
            &NewNote {
                from: Some("zeus".to_owned()),
                to: "chief".to_owned(),
                body: "Ready".to_owned(),
                task: None,
            },
        )
        .unwrap();
    context.pass().unwrap();
    assert_eq!(state_of(later.id), "queued", "nothing reaches it while out");
    context.advance(2 * 3_600_000);
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(
        state_of(later.id),
        "delivered",
        "its queue resumes after the reset"
    );
}

#[test]
fn gives_no_new_work_to_a_member_low_on_quota_keeps_one_out_for_an_hour_when_its_reset_is_unknown_and_takes_it_again_after(
) {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus", "diana"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    context.adapter.quota("zeus", Some(low(97)));
    context.pass().unwrap();
    context.adapter.answer("zeus", "done");
    context.pass().unwrap();
    tiers.open_body("Lexer");
    context.pass().unwrap();
    assert_match(&tiers.assignee(2), "^diana-");
    tiers.open_body("Tests");
    context.pass().unwrap();
    assert_match(&tiers.assignee(3), "^diana-");
    context.advance(2 * 3_600_000);
    tiers.open_body("Docs");
    context.pass().unwrap();
    assert_match(&tiers.assignee(4), "^zeus-");
    context.pass().unwrap();
    context.adapter.quota("zeus", Some(exhausted(None, None)));
    context.pass().unwrap();
    assert_eq!(
        (tiers.task(4).task.state, tiers.out_until("zeus")),
        ("open".to_owned(), Some(soon(&context, 1))),
        "out of quota mid-task: the work goes back, zeus is out for an hour"
    );
    held_to(
        context.close(),
        SUITES,
        "gives no new work to a member low on quota, keeps one out for an hour when its reset is unknown, and takes it again after",
    );
}

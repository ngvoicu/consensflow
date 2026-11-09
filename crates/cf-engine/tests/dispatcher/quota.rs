//! The dispatcher watches quota: a member that runs out holds its work with
//! its window (`quota_waits`), an old refusal in the record is history, a
//! delivery in flight is queued again, and a member nearly out of quota
//! works on to the end of it.

use cf_engine::testing::{Context, Made};
use cf_engine::ActivityState;
use cf_ledger::{NewNote, NewQuestion, NewTask};
use serde_json::json;

use crate::fixtures::{after, assert_match, exhausted, low, now, participant, placed, soon, Tiers};
use crate::traces::held_to;

const SUITES: &[&str] = &["the dispatcher watches quota"];

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
    assert_eq!(
        placed(&tiers.task(1).task),
        ("paused", Some("zeus-amber-pine")),
        "held with its window, though diana is free"
    );

    context.advance(2 * 3_600_000);
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(
        tiers.task(1).task.state,
        "working",
        "and zeus receives again"
    );
    assert_eq!(
        context
            .dispatcher
            .activity(tiers.id(&tiers.assignee(1)))
            .state,
        ActivityState::Working
    );

    context.adapter.quota(
        "zeus",
        Some(exhausted(Some(&now(&context)), Some(&soon(&context, 1)))),
    );
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "paused", "a fresh refusal counts");
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
        (
            "paused".to_owned(),
            Some("zeus-amber-pine".to_owned()),
            "queued".to_owned()
        ),
        "the task waits with its window, and the answer in flight with it"
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

/// A member of `harness` is given work at 96 percent of its quota, runs out mid-task, and goes
/// on after the reset. Codex and Muse report their usage ahead of a refusal (`Level::Low` from
/// 95 percent) and the others only once they refuse: a member of the first two was the one the
/// hold at 95 percent kept from work, and every harness is held for being out alone.
fn works_to_the_end_of_its_quota(harness: &str) {
    {
        let context = Context::made(Made {
            harnesses: vec!["claude-code", "pi", "codex", "muse"],
            ..Made::default()
        });
        let project = context
            .open_project(json!({
                "directory": "/work/app",
                "name": "app",
                "chief": { "harness": "claude-code", "model": "claude-opus-5" },
                "staff": [{ "agent": "zeus", "harness": harness, "role": "worker", "tier": "standard" }],
            }))
            .unwrap()
            .unwrap();
        let give = |body: &str| {
            context
                .create_task(
                    project.id,
                    NewTask {
                        from: "chief".to_owned(),
                        pool: Some("worker".to_owned()),
                        tier: Some("standard".to_owned()),
                        body: body.to_owned(),
                        ..NewTask::default()
                    },
                )
                .task
                .number
        };
        let task = |number: i64| context.task(project.id, number).task;
        let first = give("Write the parser");
        context.pass().unwrap();
        context.pass().unwrap();
        assert_eq!(task(first).state, "working", "{harness}");

        // 96 percent of its allowance is used, and the harness says so: nothing holds zeus.
        context.adapter.quota("zeus", Some(low(96)));
        context.pass().unwrap();
        context.adapter.answer("zeus", "done");
        context.pass().unwrap();
        let second = give("Write the lexer");
        context.pass().unwrap();
        context.pass().unwrap();
        let session = task(second)
            .assignee
            .unwrap_or_else(|| panic!("{harness}: T-{second} waits for a member at 96 percent"));
        assert_match(&session, "^zeus-");
        assert_eq!(task(second).state, "working", "{harness}");
        assert_eq!(
            participant(&context, project.id, "zeus").unwrap().out_until,
            None,
            "{harness}: a quota nearly spent is no quota out"
        );

        // The rest of the allowance goes mid-task: the work waits with its window, as it does
        // for any harness, and goes on at the reset.
        let resets_at = after(&context, 3 * 3_600_000);
        context
            .adapter
            .refuse(&session, exhausted(Some(&now(&context)), Some(&resets_at)));
        context.pass().unwrap();
        context.pass().unwrap();
        let held = task(second);
        assert_eq!(
            (
                held.state.as_str(),
                held.assignee.as_deref(),
                held.held_until.as_deref()
            ),
            ("paused", Some(session.as_str()), Some(resets_at.as_str())),
            "{harness}: out of quota mid-task, the work waits with zeus"
        );
        assert_eq!(
            participant(&context, project.id, "zeus").unwrap().out_until,
            Some(resets_at.clone()),
            "{harness}"
        );
        context.advance(3 * 3_600_000 + 60_000);
        context.pass().unwrap();
        context.pass().unwrap();
        assert_eq!(
            placed(&task(second)),
            ("working", Some(session.as_str())),
            "{harness}: it goes on after the reset"
        );
    }
}

#[test]
fn a_claude_code_member_is_given_work_runs_out_mid_task_and_goes_on_after_the_reset_as_codex_and_muse_do(
) {
    works_to_the_end_of_its_quota("claude-code");
}

#[test]
fn a_codex_member_at_96_percent_of_its_quota_is_given_work_runs_out_mid_task_and_goes_on_after_the_reset(
) {
    works_to_the_end_of_its_quota("codex");
}

#[test]
fn a_muse_member_at_96_percent_of_its_quota_is_given_work_runs_out_mid_task_and_goes_on_after_the_reset(
) {
    works_to_the_end_of_its_quota("muse");
}

//! A member on Vacation takes no task (BKV-03, Gabriel on 2026-11-13: "button to send them to
//! vacation... like stop getting tasks"): none from the scheduler, its backup none for it, and
//! a task it was given whose window never opened goes back to the board. A task in its window,
//! and the words that reach that window, are untouched.

use cf_engine::testing::{Context, Gate};
use cf_ledger::{HoldKind, NewMember};

use crate::fixtures::{given, soon, Tiers};

/// A backup for `member` on the stand-in's harness.
fn backup_of(context: &Context, tiers: &Tiers<'_>, member: &str) -> String {
    let config = NewMember {
        harness: "claude-code".to_owned(),
        model: Some("gaia".to_owned()),
        effort: None,
        model_name: None,
        designer: false,
        roles: vec!["worker".to_owned()],
        tier: "standard".to_owned(),
    };
    context
        .ledger
        .borrow_mut()
        .add_backup(tiers.project.id, member, &config)
        .unwrap()
        .handle
}

/// The member whose session `number` is with.
fn with(tiers: &Tiers<'_>, number: i64) -> String {
    let session = tiers.assignee(number);
    let owner = tiers.participant(&session).unwrap().member_id.unwrap();
    tiers
        .context
        .project(tiers.project.id)
        .participants
        .into_iter()
        .find(|p| p.id == owner)
        .unwrap()
        .handle
}

#[test]
fn a_member_on_vacation_takes_no_new_task_and_the_next_member_does() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus", "diana"]);
    context.send_on_vacation(tiers.project.id, "zeus").unwrap();
    for number in 1..=3 {
        tiers.open_body(&format!("Work {number}"));
        context.pass().unwrap();
        assert_eq!(with(&tiers, number), "diana");
    }
}

#[test]
fn the_only_member_on_vacation_leaves_the_task_waiting_and_says_why_once() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    context.send_on_vacation(tiers.project.id, "zeus").unwrap();
    tiers.open();
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(tiers.task(1).task.assignee, None);
    let said: Vec<String> = tiers
        .notes("chief")
        .into_iter()
        .filter(|note| note.contains("waits for a free"))
        .collect();
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(said[0].contains("@zeus is on Vacation"), "{said:?}");
    // Brought back, it takes the task at the next pass.
    context
        .bring_back_from_vacation(tiers.project.id, "zeus")
        .unwrap();
    context.pass().unwrap();
    assert_eq!(with(&tiers, 1), "zeus");
}

#[test]
fn a_task_given_whose_window_never_opened_goes_back_to_the_board_for_another_member() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus", "diana"]);
    tiers.open();
    let zeus = tiers.id("zeus");
    context
        .ledger
        .borrow_mut()
        .assign_task(tiers.project.id, 1, zeus)
        .unwrap();
    assert_eq!(with(&tiers, 1), "zeus");
    context.send_on_vacation(tiers.project.id, "zeus").unwrap();
    assert_eq!(tiers.task(1).task.assignee, None);
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(with(&tiers, 1), "diana");
    assert!(
        context
            .adapter
            .prepared()
            .iter()
            .all(|launch| launch["participant"]["handle"]
                .as_str()
                .is_some_and(|handle| !handle.starts_with("zeus"))),
        "no window was opened for zeus"
    );
}

#[test]
fn a_task_in_an_open_window_stays_with_it_and_goes_on() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus", "diana"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    let owner = with(&tiers, 1);
    let session = tiers.assignee(1);
    context.send_on_vacation(tiers.project.id, &owner).unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working");
    assert_eq!(tiers.assignee(1), session);
    // What is said to its window still reaches it.
    context.tell(tiers.project.id, &session, 1, "Mind the lexer");
    context.pass().unwrap();
    context.adapter.with(&session, |agent| agent.settled = true);
    context.pass().unwrap();
    context.pass().unwrap();
    assert!(
        given(&context, &session)
            .iter()
            .any(|text| text.contains("Mind the lexer")),
        "{:?}",
        given(&context, &session)
    );
}

#[test]
fn a_task_named_for_a_member_stays_and_a_new_one_named_for_it_is_refused() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus", "diana"]);
    let named = context.give(tiers.project.id, "zeus", "Parser");
    context.send_on_vacation(tiers.project.id, "zeus").unwrap();
    assert!(tiers.task(named.task.number).task.assignee.is_some());
    let refused = context.ledger.borrow_mut().create_task(
        tiers.project.id,
        &cf_ledger::NewTask {
            from: "chief".to_owned(),
            to: Some("zeus".to_owned()),
            body: "Lexer".to_owned(),
            ..cf_ledger::NewTask::default()
        },
    );
    assert!(refused.is_err());
}

#[test]
fn a_backup_takes_nothing_while_its_member_is_on_vacation_and_free() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus", "diana"]);
    let backup = backup_of(&context, &tiers, "zeus");
    context.send_on_vacation(tiers.project.id, "zeus").unwrap();
    tiers.open();
    context.pass().unwrap();
    assert_eq!(with(&tiers, 1), "diana");
    assert_ne!(with(&tiers, 1), backup);
}

#[test]
fn a_member_on_vacation_and_out_for_its_quota_keeps_its_backup_resting_too() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    backup_of(&context, &tiers, "zeus");
    let (zeus, until) = (tiers.id("zeus"), soon(&context, 3));
    context
        .ledger
        .borrow_mut()
        .mark_out_with_kind(zeus, &until, "out of quota", HoldKind::Quota)
        .unwrap();
    context.send_on_vacation(tiers.project.id, "zeus").unwrap();
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.assignee, None);
}

#[test]
fn a_backup_on_vacation_takes_no_task_and_the_wait_says_so() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    let backup = backup_of(&context, &tiers, "zeus");
    let (zeus, until) = (tiers.id("zeus"), soon(&context, 3));
    context
        .ledger
        .borrow_mut()
        .mark_out_with_kind(zeus, &until, "out of quota", HoldKind::Quota)
        .unwrap();
    context.send_on_vacation(tiers.project.id, &backup).unwrap();
    tiers.open();
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(tiers.task(1).task.assignee, None);
    let said: Vec<String> = tiers
        .notes("chief")
        .into_iter()
        .filter(|note| note.contains("waits for a free"))
        .collect();
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(
        said[0].contains(&format!("its backup @{backup} is on Vacation")),
        "{said:?}"
    );
    assert!(said[0].contains("The earliest reset is"), "{said:?}");
}

#[test]
fn a_launch_in_progress_counts_as_opened_and_the_task_stays() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    tiers.open();
    let gate = Gate::default();
    *context.host.hold.borrow_mut() = Some(gate.clone());
    let passing = context.begin_pass();
    context.settle();
    let sending = context.begin_send_on_vacation(tiers.project.id, "zeus");
    context.settle();
    gate.open();
    context.finish(passing).unwrap();
    context.finish(sending).unwrap();
    assert_eq!(with(&tiers, 1), "zeus");
    assert_ne!(tiers.task(1).task.assignee, None);
}

#[test]
fn a_task_given_with_no_pass_since_is_released_and_no_pass_after_opens_a_window_for_it() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    tiers.open();
    let zeus = tiers.id("zeus");
    context
        .ledger
        .borrow_mut()
        .assign_task(tiers.project.id, 1, zeus)
        .unwrap();
    context.send_on_vacation(tiers.project.id, "zeus").unwrap();
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.assignee, None);
    assert!(context
        .adapter
        .prepared()
        .iter()
        .all(|launch| launch["participant"]["handle"] == "chief"));
}

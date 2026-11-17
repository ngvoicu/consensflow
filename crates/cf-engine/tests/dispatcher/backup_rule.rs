//! A task its member cannot run goes to that member's backup (Gabriel on 2026-11-17: "tasks
//! that the main worker cannot do from various reasons should go to the backup; tasks should
//! never go to the backup until they fail for the main worker; new tasks are always assigned
//! to main workers if they are not in vacation"; for a task in progress: "start over if it has
//! a backup"). The task starts over on the backup in a fresh window, as a Reassign does; a task
//! given by name, a follow-up and a paused task stay; the clock is moved, the agents are
//! stand-ins.

use cf_engine::testing::Context;
use cf_harness::records::Quota;
use cf_ledger::{HoldKind, NewMember, NewTask, ParticipantView};

use crate::fixtures::{after, exhausted, now, soon, Tiers};

const NEVER: &str = "9999-12-31T23:59:59.998Z";
const MOVED: &str = "9999-12-31T23:59:59.999Z";

fn backed(context: &Context, tiers: &Tiers<'_>, member: &str) -> ParticipantView {
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
        .expect("a backup")
}

fn out(context: &Context, tiers: &Tiers<'_>, member: &str, until: &str) {
    let id = tiers.id(member);
    context
        .ledger
        .borrow_mut()
        .mark_out_with_kind(id, until, "out of quota", HoldKind::Quota)
        .expect("marked out");
}

/// The member whose session holds task `number`.
fn with(tiers: &Tiers<'_>, number: i64) -> String {
    let session = tiers.assignee(number);
    let held = tiers.participant(&session).expect("its holder");
    let Some(owner) = held.member_id else {
        return held.handle;
    };
    tiers
        .context
        .project(tiers.project.id)
        .participants
        .into_iter()
        .find(|participant| participant.id == owner)
        .expect("its member")
        .handle
}

fn went(tiers: &Tiers<'_>) -> Vec<String> {
    tiers
        .notes("chief")
        .into_iter()
        .filter(|note| note.contains("went to @") && note.contains("the backup of"))
        .collect()
}

fn passes(context: &Context, count: usize) {
    for _ in 0..count {
        context.pass().unwrap();
    }
}

#[test]
fn a_queued_task_of_a_member_refused_for_its_quota_goes_to_its_free_backup() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    let backup = backed(&context, &tiers, "zeus");
    tiers.open();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "queued");
    assert_eq!(with(&tiers, 1), "zeus");
    out(&context, &tiers, "zeus", &soon(&context, 3));
    passes(&context, 3);
    assert_eq!(with(&tiers, 1), backup.handle);
    assert_eq!(tiers.task(1).task.state, "working");
    assert_eq!(went(&tiers).len(), 1, "{:?}", tiers.notes("chief"));
}

#[test]
fn a_working_task_of_a_member_out_of_quota_starts_over_on_its_backup_in_a_fresh_window_with_the_warning(
) {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    let backup = backed(&context, &tiers, "zeus");
    tiers.open();
    passes(&context, 2);
    assert_eq!(tiers.task(1).task.state, "working");
    let first = tiers.assignee(1);
    let window = context.host.last(&first).expect("its window").pane;
    context.adapter.refuse(
        "zeus",
        exhausted(Some(&now(&context)), Some(&after(&context, 20 * 60_000))),
    );
    passes(&context, 4);
    assert_eq!(with(&tiers, 1), backup.handle);
    let second = tiers.assignee(1);
    assert_ne!(second, first);
    assert!(
        context.host.killed().contains(&window),
        "the old window closed"
    );
    assert!(
        context.host.last(&second).is_some(),
        "a fresh window opened"
    );
    let body = tiers.task(1).task.body;
    assert!(
        body.contains("check the working tree for partial changes"),
        "{body}"
    );
    assert_eq!(
        went(&tiers).len(),
        1,
        "told once: {:?}",
        tiers.notes("chief")
    );
}

#[test]
fn a_new_task_goes_to_the_backup_when_the_member_is_out_for_what_does_not_pass_by_itself() {
    for what in ["moved", "needs an edit", "account"] {
        let context = Context::new();
        let tiers = Tiers::new(&context, &["zeus"]);
        let backup = backed(&context, &tiers, "zeus");
        let zeus = tiers.id("zeus");
        match what {
            "moved" => {
                context
                    .ledger
                    .borrow_mut()
                    .mark_out_with_kind(zeus, MOVED, "model moved", HoldKind::Moved)
                    .unwrap();
            }
            "account" => {
                context
                    .ledger
                    .borrow_mut()
                    .mark_out_with_kind(zeus, NEVER, "account", HoldKind::Account)
                    .unwrap();
            }
            _ => {
                context
                    .ledger
                    .borrow()
                    .sqlite()
                    .execute("UPDATE participant SET needs_edit = 1 WHERE id = ?", [zeus])
                    .unwrap();
            }
        }
        tiers.open();
        passes(&context, 2);
        assert_eq!(with(&tiers, 1), backup.handle, "{what}");
        assert_eq!(went(&tiers).len(), 1, "{what}: {:?}", tiers.notes("chief"));
    }
}

#[test]
fn a_working_task_of_a_member_that_needs_an_edit_or_whose_model_moved_goes_to_its_backup() {
    for what in ["moved", "needs an edit"] {
        let context = Context::new();
        let tiers = Tiers::new(&context, &["zeus"]);
        let backup = backed(&context, &tiers, "zeus");
        tiers.open();
        passes(&context, 2);
        let zeus = tiers.id("zeus");
        if what == "moved" {
            context
                .ledger
                .borrow_mut()
                .mark_out_with_kind(zeus, MOVED, "model moved", HoldKind::Moved)
                .unwrap();
        } else {
            context
                .ledger
                .borrow()
                .sqlite()
                .execute("UPDATE participant SET needs_edit = 1 WHERE id = ?", [zeus])
                .unwrap();
        }
        passes(&context, 4);
        assert_eq!(with(&tiers, 1), backup.handle, "{what}");
    }
}

#[test]
fn a_task_by_name_a_follow_up_and_a_paused_task_stay_with_their_member() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    let backup = backed(&context, &tiers, "zeus");
    let project = tiers.project.id;
    tiers.open();
    passes(&context, 2);
    let session = tiers.assignee(1);
    // T-1 finishes; T-2 follows it up in the same window; T-3 is given by name; T-4 is paused.
    context
        .ledger
        .borrow_mut()
        .record_result(project, 1, "Parser done")
        .unwrap();
    context.create_task(
        project,
        NewTask {
            from: "chief".to_owned(),
            after: Some(1),
            body: "Now the lexer".to_owned(),
            ..NewTask::default()
        },
    );
    context.create_task(
        project,
        NewTask {
            from: "chief".to_owned(),
            to: Some("zeus".to_owned()),
            body: "By name".to_owned(),
            ..NewTask::default()
        },
    );
    tiers.open_body("To pause");
    passes(&context, 2);
    context.pause_task(project, 4);
    let paused_with = tiers.assignee(4);
    out(&context, &tiers, "zeus", &soon(&context, 3));
    passes(&context, 4);
    for number in [2, 3] {
        assert_eq!(with(&tiers, number), "zeus", "T-{number} stays");
    }
    assert_eq!(tiers.assignee(2), session);
    assert_eq!(tiers.assignee(4), paused_with);
    assert_eq!(tiers.task(4).task.state, "paused");
    assert_ne!(with(&tiers, 4), backup.handle);
}

#[test]
fn with_the_backup_out_too_the_task_waits_and_goes_to_the_backup_when_it_is_free_first() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    let backup = backed(&context, &tiers, "zeus");
    tiers.open();
    passes(&context, 2);
    out(&context, &tiers, "zeus", &soon(&context, 3));
    out(&context, &tiers, &backup.handle, &soon(&context, 1));
    passes(&context, 3);
    assert_eq!(with(&tiers, 1), "zeus", "waits with its member");
    assert_eq!(went(&tiers).len(), 0);
    context.advance(61 * 60_000);
    passes(&context, 4);
    assert_eq!(with(&tiers, 1), backup.handle);
    assert_eq!(went(&tiers).len(), 1);
}

#[test]
fn a_new_task_with_member_and_backup_out_waits_then_goes_to_the_backup_free_first() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    let backup = backed(&context, &tiers, "zeus");
    out(&context, &tiers, "zeus", &soon(&context, 3));
    out(&context, &tiers, &backup.handle, &soon(&context, 1));
    tiers.open();
    passes(&context, 3);
    assert_eq!(tiers.task(1).task.assignee, None);
    context.advance(61 * 60_000);
    passes(&context, 3);
    assert_eq!(with(&tiers, 1), backup.handle);
}

#[test]
fn a_member_on_vacation_keeps_its_task_and_its_backup_takes_nothing_for_it() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    backed(&context, &tiers, "zeus");
    tiers.open();
    passes(&context, 2);
    context.send_on_vacation(tiers.project.id, "zeus").unwrap();
    out(&context, &tiers, "zeus", &soon(&context, 3));
    // An edit it needs does not make it a failure its backup covers while it rests.
    let zeus = tiers.id("zeus");
    context
        .ledger
        .borrow()
        .sqlite()
        .execute("UPDATE participant SET needs_edit = 1 WHERE id = ?", [zeus])
        .unwrap();
    tiers.open_body("Lexer");
    passes(&context, 4);
    assert_eq!(with(&tiers, 1), "zeus");
    assert_eq!(tiers.task(2).task.assignee, None);
    assert_eq!(went(&tiers).len(), 0);
}

#[test]
fn a_task_whose_models_busy_tries_are_spent_goes_to_the_backup_not_before() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    let backup = backed(&context, &tiers, "zeus");
    tiers.open();
    passes(&context, 2);
    for wait in [60_000, 120_000, 240_000] {
        context.adapter.refuse(
            "zeus",
            Quota::Busy {
                at: Some(now(&context)),
                because: "Selected model is at capacity.".to_owned(),
            },
        );
        passes(&context, 2);
        assert_eq!(with(&tiers, 1), "zeus", "tried again");
        context.advance(wait + 1_000);
        passes(&context, 2);
    }
    context.adapter.refuse(
        "zeus",
        Quota::Busy {
            at: Some(now(&context)),
            because: "Selected model is at capacity.".to_owned(),
        },
    );
    passes(&context, 4);
    assert_eq!(with(&tiers, 1), backup.handle);
}

#[test]
fn a_busy_model_past_its_tries_goes_to_the_backup_before_another_free_member() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus", "diana"]);
    let backup = backed(&context, &tiers, "zeus");
    tiers.open();
    passes(&context, 2);
    assert_eq!(with(&tiers, 1), "zeus");
    for wait in [60_000, 120_000, 240_000] {
        context.adapter.refuse(
            "zeus",
            Quota::Busy {
                at: Some(now(&context)),
                because: "Selected model is at capacity.".to_owned(),
            },
        );
        passes(&context, 2);
        context.advance(wait + 1_000);
        passes(&context, 2);
    }
    context.adapter.refuse(
        "zeus",
        Quota::Busy {
            at: Some(now(&context)),
            because: "Selected model is at capacity.".to_owned(),
        },
    );
    passes(&context, 4);
    assert_eq!(with(&tiers, 1), backup.handle, "its backup, not diana");
}

#[test]
fn with_no_member_able_a_new_task_goes_to_the_backup_of_the_one_that_does_not_come_back() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus", "diana"]);
    let first = backed(&context, &tiers, "zeus");
    let second = backed(&context, &tiers, "diana");
    out(&context, &tiers, "zeus", &soon(&context, 3));
    let diana = tiers.id("diana");
    context
        .ledger
        .borrow()
        .sqlite()
        .execute(
            "UPDATE participant SET needs_edit = 1 WHERE id = ?",
            [diana],
        )
        .unwrap();
    tiers.open();
    passes(&context, 2);
    assert_eq!(
        with(&tiers, 1),
        second.handle,
        "an edit is the latest of resets"
    );
    assert_ne!(with(&tiers, 1), first.handle);
}

#[test]
fn a_member_on_vacation_that_needs_an_edit_leaves_a_new_task_waiting_and_its_backup_rests() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    backed(&context, &tiers, "zeus");
    context.send_on_vacation(tiers.project.id, "zeus").unwrap();
    let zeus = tiers.id("zeus");
    context
        .ledger
        .borrow()
        .sqlite()
        .execute("UPDATE participant SET needs_edit = 1 WHERE id = ?", [zeus])
        .unwrap();
    tiers.open();
    passes(&context, 3);
    assert_eq!(tiers.task(1).task.assignee, None);
    assert_eq!(went(&tiers).len(), 0);
}

fn busy_refusal(context: &Context) -> Quota {
    Quota::Busy {
        at: Some(now(context)),
        because: "Selected model is at capacity.".to_owned(),
    }
}

#[test]
fn a_busy_model_moves_only_the_task_whose_own_tries_are_spent_not_the_one_at_work() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    let backup = backed(&context, &tiers, "zeus");
    tiers.open();
    tiers.open_body("Lexer");
    passes(&context, 3);
    let (first, second) = (tiers.assignee(1), tiers.assignee(2));
    assert_ne!(first, second, "two sessions of zeus");
    assert_eq!(with(&tiers, 1), "zeus");
    assert_eq!(with(&tiers, 2), "zeus");
    for wait in [60_000, 120_000, 240_000] {
        context.adapter.refuse(&first, busy_refusal(&context));
        passes(&context, 2);
        context.advance(wait + 1_000);
        passes(&context, 2);
    }
    context.adapter.refuse(&first, busy_refusal(&context));
    passes(&context, 4);
    assert_eq!(with(&tiers, 1), backup.handle, "its tries are spent");
    assert_eq!(with(&tiers, 2), "zeus", "the window at work keeps its task");
    assert_eq!(tiers.assignee(2), second);
}

#[test]
fn the_old_window_is_retired_even_one_the_human_reopened() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    let backup = backed(&context, &tiers, "zeus");
    tiers.open();
    passes(&context, 2);
    let session = tiers.assignee(1);
    let window = context.host.last(&session).expect("its window").pane;
    context.open_window(tiers.project.id, &session).unwrap();
    // A member that needs an edit is no one whose window closes of itself: only the move does.
    let zeus = tiers.id("zeus");
    context
        .ledger
        .borrow()
        .sqlite()
        .execute("UPDATE participant SET needs_edit = 1 WHERE id = ?", [zeus])
        .unwrap();
    passes(&context, 4);
    assert_eq!(with(&tiers, 1), backup.handle);
    assert!(
        context.host.killed().contains(&window),
        "its window was stopped"
    );
}

#[test]
fn the_giver_is_told_once_in_one_line_when_a_task_moves_to_the_backup() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    let backup = backed(&context, &tiers, "zeus");
    tiers.open();
    passes(&context, 2);
    out(&context, &tiers, "zeus", &soon(&context, 3));
    passes(&context, 4);
    assert_eq!(with(&tiers, 1), backup.handle);
    let about: Vec<String> = tiers
        .notes("chief")
        .into_iter()
        .filter(|note| {
            note.contains("T-1 ") && (note.contains("went to") || note.contains("taken back"))
        })
        .collect();
    assert_eq!(about.len(), 1, "{about:?}");
    assert!(about[0].contains("the backup of @zeus"), "{about:?}");
}

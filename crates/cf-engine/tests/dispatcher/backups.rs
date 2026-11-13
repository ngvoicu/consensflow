//! A member's backup takes a task when no member of its role and tier is free, and only when
//! its member is out for its quota or its login (BKV-03, Gabriel on 2026-11-13: "one backup per
//! member and multiple members can be added per tier and role"). A free member is always
//! picked first; a member that is busy, moved, needs an edit or is on Vacation has a backup
//! that takes nothing; a task already in a window of a member that goes out stays held there.

use cf_engine::testing::{Context, Made};
use cf_ledger::{HoldKind, NewMember, ParticipantView};

use crate::fixtures::{after, exhausted, now, soon, Tiers};

/// The time a member out for its account is marked until, and the one for a moved model.
const NEVER: &str = "9999-12-31T23:59:59.998Z";
const MOVED: &str = "9999-12-31T23:59:59.999Z";

/// What the giver is told once a task goes to a backup whose member is out for its quota.
fn went_to(task: i64, backup: &str, member: &str, until: &str) -> String {
    format!(
        "T-{task} went to @{backup}, the backup of @{member}, which is out of quota until {until}."
    )
}

/// A backup for `member`, on `harness` and `model`.
fn backed(
    context: &Context,
    project: i64,
    member: &str,
    harness: &str,
    model: &str,
) -> ParticipantView {
    let config = NewMember {
        harness: harness.to_owned(),
        model: Some(model.to_owned()),
        effort: None,
        model_name: None,
        designer: false,
        roles: vec!["worker".to_owned()],
        tier: "standard".to_owned(),
    };
    context
        .ledger
        .borrow_mut()
        .add_backup(project, member, &config)
        .expect("a backup")
}

/// `member` out of its quota until `until`, as its window's refusal marks it.
fn out(context: &Context, tiers: &Tiers<'_>, member: &str, until: &str) {
    let id = tiers.id(member);
    context
        .ledger
        .borrow_mut()
        .mark_out_with_kind(id, until, "out of quota", HoldKind::Quota)
        .expect("marked out");
}

/// The member whose session holds task `number`.
fn taker(tiers: &Tiers<'_>, number: i64) -> String {
    let session = tiers.assignee(number);
    let owner = tiers
        .participant(&session)
        .and_then(|session| session.member_id)
        .expect("a session of a member");
    tiers
        .context
        .project(tiers.project.id)
        .participants
        .into_iter()
        .find(|participant| participant.id == owner)
        .expect("its member")
        .handle
}

/// The notes ConsensFlow wrote the chief of T-`number` going to a backup.
fn told_of_a_backup(tiers: &Tiers<'_>) -> Vec<String> {
    tiers
        .notes("chief")
        .into_iter()
        .filter(|note| note.contains("the backup of"))
        .collect()
}

#[test]
fn a_task_goes_to_the_backup_of_the_only_member_when_it_is_out_for_its_quota_and_its_giver_is_told_once(
) {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    let backup = backed(&context, tiers.project.id, "zeus", "claude-code", "gaia");
    let until = soon(&context, 3);
    out(&context, &tiers, "zeus", &until);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();

    assert_eq!(taker(&tiers, 1), backup.handle);
    assert_eq!(tiers.task(1).task.state, "working");
    assert_eq!(
        told_of_a_backup(&tiers),
        [went_to(1, &backup.handle, "zeus", &until)]
    );
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(told_of_a_backup(&tiers).len(), 1, "told once");
}

#[test]
fn a_free_member_takes_the_task_before_a_backup_whatever_it_has_taken() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus", "diana"]);
    backed(&context, tiers.project.id, "zeus", "claude-code", "gaia");
    out(&context, &tiers, "zeus", &soon(&context, 3));
    tiers.open();
    context.pass().unwrap();
    assert_eq!(taker(&tiers, 1), "diana");
    assert_eq!(told_of_a_backup(&tiers), Vec::<String>::new());
}

#[test]
fn a_backup_takes_nothing_while_its_member_is_free() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    backed(&context, tiers.project.id, "zeus", "claude-code", "gaia");
    for number in 1..=3 {
        tiers.open_body(&format!("Work {number}"));
        context.pass().unwrap();
        assert_eq!(taker(&tiers, number), "zeus");
    }
}

#[test]
fn with_two_members_out_the_backup_of_the_one_whose_reset_is_latest_takes_the_task() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus", "diana"]);
    let first = backed(&context, tiers.project.id, "zeus", "claude-code", "gaia");
    let second = backed(&context, tiers.project.id, "diana", "claude-code", "rhea");
    out(&context, &tiers, "zeus", &soon(&context, 2));
    out(&context, &tiers, "diana", &soon(&context, 5));
    tiers.open();
    context.pass().unwrap();
    assert_eq!(taker(&tiers, 1), second.handle);
    // The other way round: diana's reset is the sooner.
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus", "diana"]);
    let first_again = backed(&context, tiers.project.id, "zeus", "claude-code", "gaia");
    backed(&context, tiers.project.id, "diana", "claude-code", "rhea");
    out(&context, &tiers, "zeus", &soon(&context, 6));
    out(&context, &tiers, "diana", &soon(&context, 5));
    tiers.open();
    context.pass().unwrap();
    assert_eq!(taker(&tiers, 1), first_again.handle);
    let _ = first;
}

#[test]
fn only_a_hold_that_is_a_quota_or_an_account_gives_the_task_to_the_backup() {
    // Busy, moved, needing an edit: its backup takes nothing, and the task waits.
    for what in ["busy", "moved", "needs an edit"] {
        let context = Context::new();
        let tiers = Tiers::new(&context, &["zeus"]);
        backed(&context, tiers.project.id, "zeus", "claude-code", "gaia");
        let zeus = tiers.id("zeus");
        match what {
            "busy" => context.mark_out_busy(zeus, &soon(&context, 1)),
            "moved" => {
                context
                    .ledger
                    .borrow_mut()
                    .mark_out_with_kind(zeus, MOVED, "model moved", HoldKind::Moved)
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
        context.pass().unwrap();
        context.pass().unwrap();
        assert_eq!(tiers.task(1).task.state, "open", "{what}");
        assert_eq!(tiers.task(1).task.assignee, None, "{what}");
    }
}

#[test]
fn a_member_out_for_its_account_gives_the_task_to_its_backup_too() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    let backup = backed(&context, tiers.project.id, "zeus", "claude-code", "gaia");
    let zeus = tiers.id("zeus");
    context
        .ledger
        .borrow_mut()
        .mark_out_with_kind(zeus, NEVER, "account unavailable", HoldKind::Account)
        .unwrap();
    tiers.open();
    context.pass().unwrap();
    assert_eq!(taker(&tiers, 1), backup.handle);
    assert_eq!(
        told_of_a_backup(&tiers),
        [format!(
            "T-1 went to @{}, the backup of @zeus, whose account is unavailable (its billing or login was refused).",
            backup.handle
        )]
    );
}

#[test]
fn a_hold_nobody_typed_never_gives_the_task_to_a_backup() {
    // A timed hold from before the kind was kept: unknown, not a quota.
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    backed(&context, tiers.project.id, "zeus", "claude-code", "gaia");
    let zeus = tiers.id("zeus");
    context
        .ledger
        .borrow_mut()
        .mark_out(zeus, &soon(&context, 2), "an old reason")
        .unwrap();
    tiers.open();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "open");
}

#[test]
fn a_hold_whose_reset_has_passed_is_no_hold_and_the_member_takes_the_task() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    backed(&context, tiers.project.id, "zeus", "claude-code", "gaia");
    out(&context, &tiers, "zeus", &soon(&context, 1));
    context.advance(61 * 60_000);
    tiers.open();
    context.pass().unwrap();
    assert_eq!(taker(&tiers, 1), "zeus");
    assert_eq!(told_of_a_backup(&tiers), Vec::<String>::new());
}

#[test]
fn a_busy_hold_stays_a_busy_hold_after_a_restart_and_its_backup_takes_nothing() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    backed(&context, tiers.project.id, "zeus", "claude-code", "gaia");
    context.mark_out_busy(tiers.id("zeus"), &soon(&context, 1));
    // The engine that marked it is gone: its memory of why is gone with it.
    let restarted = context.make();
    tiers.open();
    restarted.pass().unwrap();
    restarted.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "open");
    assert_eq!(told_of_a_backup(&tiers), Vec::<String>::new());
}

#[test]
fn a_wait_after_a_restart_still_says_the_model_is_busy_and_not_that_the_quota_is_out() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    backed(&context, tiers.project.id, "zeus", "claude-code", "gaia");
    let until = soon(&context, 1);
    context.mark_out_busy(tiers.id("zeus"), &until);
    let restarted = context.make();
    tiers.open();
    restarted.pass().unwrap();
    assert!(
        tiers
            .notes("chief")
            .iter()
            .any(|note| note.contains(&format!(
                "@zeus's model is busy (ConsensFlow tries it again at {until})"
            ))),
        "{:?}",
        tiers.notes("chief")
    );
}

/// A project whose only member `zeus` is out for its quota, with a backup that has taken T-1 and
/// whose window is open.
fn backup_at_work(context: &Context) -> (Tiers<'_>, ParticipantView, cf_harness::contract::Pane) {
    let tiers = Tiers::new(context, &["zeus"]);
    let backup = backed(context, tiers.project.id, "zeus", "claude-code", "gaia");
    out(context, &tiers, "zeus", &soon(context, 3));
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    let session = tiers.assignee(1);
    assert_eq!(taker(&tiers, 1), backup.handle);
    let pane = context.host.last(&session).expect("its window").pane;
    (tiers, backup, pane)
}

#[test]
fn a_member_who_leaves_takes_its_backup_and_the_backups_work_with_it() {
    let context = Context::new();
    let (tiers, backup, pane) = backup_at_work(&context);
    let removed = context.remove_member(tiers.project.id, "zeus").unwrap();
    assert!(context.host.killed().contains(&pane), "its window closed");
    let staff = context.project(tiers.project.id).participants;
    assert!(
        staff.iter().all(|p| p.handle != backup.handle),
        "the backup left with it"
    );
    let _ = removed;
}

/// The ledger's half of the same removal (T-1619): the backup's task is cancelled with the
/// member's own, and the requester told in the same note.
#[test]
fn a_member_who_leaves_has_its_backups_task_cancelled_with_its_own() {
    let context = Context::new();
    let (tiers, _backup, _pane) = backup_at_work(&context);
    let removed = context.remove_member(tiers.project.id, "zeus").unwrap();
    assert_eq!(removed.cancelled, [1]);
    assert_eq!(tiers.task(1).task.state, "cancelled");
}

#[test]
fn a_backup_taken_off_the_staff_closes_its_window() {
    let context = Context::new();
    let (tiers, _backup, pane) = backup_at_work(&context);
    context.remove_backup(tiers.project.id, "zeus").unwrap();
    assert!(context.host.killed().contains(&pane), "its window closed");
}

/// The ledger's half (T-1619): a removed backup's task is cancelled as a member's own.
#[test]
fn a_backup_taken_off_the_staff_has_its_task_cancelled() {
    let context = Context::new();
    let (tiers, _backup, _pane) = backup_at_work(&context);
    context.remove_backup(tiers.project.id, "zeus").unwrap();
    assert_eq!(tiers.task(1).task.state, "cancelled");
}

#[test]
fn a_backup_replaced_closes_the_old_ones_window_and_the_new_one_takes_work() {
    let context = Context::new();
    let (tiers, old, pane) = backup_at_work(&context);
    let config = NewMember {
        harness: "claude-code".to_owned(),
        model: Some("rhea".to_owned()),
        effort: None,
        model_name: None,
        designer: false,
        roles: vec!["worker".to_owned()],
        tier: "standard".to_owned(),
    };
    let new = context
        .change_backup(tiers.project.id, "zeus", config)
        .unwrap();
    assert_ne!(new.handle, old.handle);
    assert!(context.host.killed().contains(&pane), "its window closed");
    tiers.open_body("Lexer");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(taker(&tiers, 2), new.handle);
}

#[test]
fn the_member_back_takes_the_next_task_and_the_backup_finishes_what_it_has() {
    let context = Context::new();
    let (tiers, backup, _pane) = backup_at_work(&context);
    context.advance(4 * 3_600_000);
    tiers.open_body("Lexer");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(taker(&tiers, 2), "zeus");
    // The backup's task goes on in its own window, to its end.
    assert_eq!(taker(&tiers, 1), backup.handle);
    assert_eq!(tiers.task(1).task.state, "working");
}

#[test]
fn a_busy_task_in_a_window_is_not_released_onto_a_backup() {
    // A member out for its quota whose own task is held with its window: the backup, free, is
    // not "someone else" for the release (T-1356 keeps the work where it is).
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    let backup = backed(&context, tiers.project.id, "zeus", "claude-code", "gaia");
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    context.adapter.refuse(
        "zeus",
        exhausted(Some(&now(&context)), Some(&after(&context, 20 * 60_000))),
    );
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(taker(&tiers, 1), "zeus", "held with its window");
    assert_ne!(taker(&tiers, 1), backup.handle);
}

#[test]
fn each_harness_works_as_a_backup_and_as_a_member() {
    for harness in ["claude-code", "codex", "pi", "devin", "muse"] {
        let context = Context::made(Made {
            harnesses: vec!["claude-code", "codex", "pi", "devin", "muse"],
            codex: true,
            ..Made::default()
        });
        let tiers = Tiers::new(&context, &["zeus"]);
        let backup = backed(&context, tiers.project.id, "zeus", harness, "gaia");
        out(&context, &tiers, "zeus", &soon(&context, 3));
        tiers.open();
        context.pass().unwrap();
        context.pass().unwrap();
        assert_eq!(taker(&tiers, 1), backup.handle, "{harness} as a backup");
    }
}

#[test]
fn a_task_released_and_taken_again_by_a_backup_is_told_to_its_giver_once() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    let backup = backed(&context, tiers.project.id, "zeus", "claude-code", "gaia");
    out(&context, &tiers, "zeus", &soon(&context, 3));
    tiers.open();
    context.pass().unwrap();
    assert_eq!(taker(&tiers, 1), backup.handle);
    // The task is taken back before its window opened, and goes to the backup again.
    context
        .ledger
        .borrow_mut()
        .release_task(tiers.project.id, 1, "by @human")
        .unwrap();
    assert_eq!(tiers.task(1).task.assignee, None);
    context.pass().unwrap();
    assert_eq!(taker(&tiers, 1), backup.handle);
    assert_eq!(
        told_of_a_backup(&tiers).len(),
        1,
        "{:?}",
        told_of_a_backup(&tiers)
    );
}

#[test]
fn a_busy_refusal_of_its_window_keeps_its_backup_from_the_next_task_after_a_restart_too() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    backed(&context, tiers.project.id, "zeus", "claude-code", "gaia");
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    let session = tiers.assignee(1);
    context.adapter.refuse(
        &session,
        cf_harness::records::Quota::Busy {
            at: Some(now(&context)),
            because: "Selected model is at capacity.".to_owned(),
        },
    );
    context.pass().unwrap();
    context.pass().unwrap();
    assert!(
        tiers.out_until("zeus").is_some(),
        "its model is marked busy"
    );
    let restarted = context.make();
    tiers.open_body("Lexer");
    restarted.pass().unwrap();
    restarted.pass().unwrap();
    assert_eq!(tiers.task(2).task.assignee, None);
    assert_eq!(told_of_a_backup(&tiers), Vec::<String>::new());
}

#[test]
fn a_backups_window_still_opening_is_waited_for_before_it_is_closed_for_a_member_who_leaves() {
    // A member's own in-flight launch is waited for by `forget`, which closes each leaving
    // record under that record's own hold; the backup's is forgotten the same way.
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    backed(&context, tiers.project.id, "zeus", "claude-code", "gaia");
    out(&context, &tiers, "zeus", &soon(&context, 3));
    tiers.open();
    let gate = cf_engine::testing::Gate::default();
    *context.host.hold.borrow_mut() = Some(gate.clone());
    let passing = context.begin_pass();
    let removing = context.begin_remove_member(tiers.project.id, "zeus");
    context.settle();
    assert!(
        context.host.killed().is_empty(),
        "the launch is still in progress"
    );
    gate.open();
    context.finish(passing).unwrap();
    context.finish(removing).unwrap();
    let session = tiers.assignee(1);
    let pane = context.host.last(&session).expect("its window").pane;
    assert!(
        context.host.killed().contains(&pane),
        "its window closed once opened"
    );
}

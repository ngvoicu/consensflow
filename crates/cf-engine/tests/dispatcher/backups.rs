//! A member's backup takes a task when no member of its role and tier is free, and only when
//! its member is out for its quota or its login (BKV-03, Gabriel on 2026-11-13: "one backup per
//! member and multiple members can be added per tier and role"). A free member is always
//! picked first; a member that is busy, moved, needs an edit or is on Vacation has a backup
//! that takes nothing; a task already in a window of a member that goes out stays held there.

use cf_engine::testing::{Context, Made};
use cf_harness::records::Quota;
use cf_ledger::{HoldKind, NewMember, ParticipantView};

use crate::fixtures::{after, exhausted, now, participant, soon, Tiers};

/// The time a member out for its account is marked until, and the one for a moved model.
const NEVER: &str = "9999-12-31T23:59:59.998Z";
const MOVED: &str = "9999-12-31T23:59:59.999Z";

/// What the giver is told once a task goes to a backup whose member is out for its quota.
fn went_to(task: i64, backup: &str, member: &str, until: &str) -> String {
    format!("T-{task} went to @{backup}, the backup of @{member}, which is out of quota until {until}.")
}

/// A backup for `member`, on `harness` and `model`.
fn backed(context: &Context, project: i64, member: &str, harness: &str, model: &str) -> ParticipantView {
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
fn a_task_goes_to_the_backup_of_the_only_member_when_it_is_out_for_its_quota_and_its_giver_is_told_once()
{
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
            .any(|note| note.contains(&format!("@zeus's model is busy (ConsensFlow tries it again at {until})"))),
        "{:?}",
        tiers.notes("chief")
    );
}

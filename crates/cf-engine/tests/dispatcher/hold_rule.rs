//! Member holds: shared-login return, edits and reset precedence.
use super::fixtures::{after, now, soon, Tiers};
use cf_engine::testing::Context;
use cf_harness::records::Quota;
use cf_ledger::{HoldKind, MemberEdit, NewMember, ParticipantView};

fn passes(c: &Context, n: usize) {
    for _ in 0..n {
        c.pass().unwrap();
    }
}
#[test]
fn try_again_now_releases_members_on_the_same_login() {
    let c = Context::new();
    c.adapter.one_account.set(true);
    let t = Tiers::new(&c, &["zeus", "diana"]);
    let far = soon(&c, 72);
    for name in ["zeus", "diana"] {
        let id = t.id(name);
        c.ledger
            .borrow_mut()
            .mark_out_with_kind(id, &far, "held", HoldKind::Quota)
            .unwrap();
    }
    c.back_from_quota(t.project.id, "zeus").unwrap();
    assert_eq!(t.out_until("zeus"), None);
    assert_eq!(t.out_until("diana"), None, "the shared login is back");
}

#[test]
fn editing_the_model_ends_its_quota_hold() {
    let c = Context::new();
    let t = Tiers::new(&c, &["zeus"]);
    let far = soon(&c, 72);
    let id = t.id("zeus");
    c.ledger
        .borrow_mut()
        .mark_out_with_kind(id, &far, "held", HoldKind::Quota)
        .unwrap();
    let edited = c
        .ledger
        .borrow_mut()
        .edit_member(
            t.project.id,
            "zeus",
            &MemberEdit {
                harness: "claude-code".into(),
                model: Some("new-model".into()),
                effort: None,
                model_name: None,
                tier: "standard".into(),
            },
        )
        .unwrap();
    assert_eq!(edited.out_until, None);
}

#[test]
fn editing_a_model_ends_a_legacy_quota_hold_with_no_stored_kind() {
    let c = Context::new();
    let t = Tiers::new(&c, &["zeus"]);
    let id = t.id("zeus");
    let until = after(&c, 30 * 60_000);
    c.ledger
        .borrow()
        .sqlite()
        .execute(
            "UPDATE participant SET out_until=?1, out_kind=NULL WHERE id=?2",
            (&until, id),
        )
        .unwrap();
    let edited = c
        .ledger
        .borrow_mut()
        .edit_member(
            t.project.id,
            "zeus",
            &MemberEdit {
                harness: "claude-code".into(),
                model: Some("new-model".into()),
                model_name: None,
                effort: None,
                tier: "standard".into(),
            },
        )
        .unwrap();
    assert_eq!(
        edited.out_until, None,
        "legacy quota holds respond to an edit too"
    );
}

#[test]
fn editing_a_model_does_not_end_a_legacy_busy_hold_with_no_stored_kind() {
    let c = Context::new();
    let t = Tiers::new(&c, &["zeus"]);
    let id = t.id("zeus");
    let until = after(&c, 60_000);
    c.ledger
        .borrow_mut()
        .mark_out_with_kind(id, &until, "model busy", HoldKind::Busy)
        .unwrap();
    c.ledger
        .borrow()
        .sqlite()
        .execute("UPDATE participant SET out_kind=NULL WHERE id=?", [id])
        .unwrap();
    let edited = c
        .ledger
        .borrow_mut()
        .edit_member(
            t.project.id,
            "zeus",
            &MemberEdit {
                harness: "claude-code".into(),
                model: Some("new-model".into()),
                model_name: None,
                effort: None,
                tier: "standard".into(),
            },
        )
        .unwrap();
    assert_eq!(
        edited.out_until.as_deref(),
        Some(until.as_str()),
        "a busy hold keeps its backoff"
    );
}

#[test]
fn editing_only_the_harness_ends_a_quota_or_account_hold() {
    use cf_ledger::{HoldKind, MemberEdit};
    for kind in [HoldKind::Quota, HoldKind::Account] {
        let c = Context::new();
        let t = Tiers::new(&c, &["zeus"]);
        let id = t.id("zeus");
        let member = t.participant("zeus").unwrap();
        assert_eq!(member.harness.as_deref(), Some("claude-code"));
        c.ledger
            .borrow_mut()
            .mark_out_with_kind(id, "9999-12-31T23:59:59.998Z", "fixture hold", kind)
            .unwrap();
        let edited = c
            .ledger
            .borrow_mut()
            .edit_member(
                t.project.id,
                "zeus",
                &MemberEdit {
                    harness: "codex".into(),
                    model: member.model.clone(),
                    effort: member.effort.clone(),
                    model_name: member.model_name.clone(),
                    tier: "standard".into(),
                },
            )
            .unwrap();
        assert_eq!(edited.out_until, None, "editing the harness ends its hold");
    }
}

#[test]
fn later_refusal_with_an_earlier_reset_shortens_the_quota_hold() {
    let c = Context::new();
    let t = Tiers::new(&c, &["zeus"]);
    let id = t.id("zeus");
    let later = soon(&c, 72);
    let earlier = soon(&c, 12);
    c.ledger
        .borrow_mut()
        .mark_out_with_kind(id, &later, "out of quota", HoldKind::Quota)
        .unwrap();
    c.advance(1000);
    c.ledger
        .borrow_mut()
        .mark_out_with_kind(id, &earlier, "out of quota", HoldKind::Quota)
        .unwrap();
    assert_eq!(t.out_until("zeus").as_deref(), Some(earlier.as_str()));
}

#[test]
fn long_reset_is_not_capped_at_thirty_minutes() {
    let c = Context::new();
    let t = Tiers::new(&c, &["zeus"]);
    t.open();
    passes(&c, 4);
    c.advance(1000);
    let reset = soon(&c, 72);
    c.adapter.refuse(
        &t.assignee(1),
        Quota::Exhausted {
            at: Some(now(&c)),
            resets_at: Some(reset.clone()),
        },
    );
    passes(&c, 4);
    c.advance(31 * 60_000);
    passes(&c, 4);
    assert_eq!(t.out_until("zeus").as_deref(), Some(reset.as_str()));
    assert_eq!(t.task(1).task.state, "paused");
}

#[test]
fn an_earlier_report_also_shortens_the_original_tasks_wait() {
    let c = Context::new();
    let t = Tiers::new(&c, &["zeus"]);
    t.open();
    c.give(t.project.id, "zeus", "Another window");
    passes(&c, 5);
    let first = t.assignee(1);
    let second = t.assignee(2);
    c.advance(1000);
    let later = soon(&c, 72);
    c.adapter.refuse(
        &first,
        Quota::Exhausted {
            at: Some(now(&c)),
            resets_at: Some(later.clone()),
        },
    );
    passes(&c, 4);
    assert_eq!(t.task(1).task.held_until.as_deref(), Some(later.as_str()));
    c.advance(1000);
    let earlier = soon(&c, 12);
    c.adapter.refuse(
        &second,
        Quota::Exhausted {
            at: Some(now(&c)),
            resets_at: Some(earlier.clone()),
        },
    );
    passes(&c, 4);
    assert_eq!(t.out_until("zeus").as_deref(), Some(earlier.as_str()));
    assert_eq!(t.task(1).task.held_until.as_deref(), Some(earlier.as_str()));
    let aside = t.task(1).task.aside.unwrap_or_default();
    assert!(
        aside.contains(&earlier),
        "the task line names its current reset: {aside}"
    );
    c.advance(12 * 60 * 60_000 + 1);
    passes(&c, 6);
    assert_eq!(t.task(1).task.state, "working");
}

#[test]
fn a_later_reset_does_not_extend_an_existing_quota_hold() {
    let c = Context::new();
    let t = Tiers::new(&c, &["zeus"]);
    let id = t.id("zeus");
    let first = soon(&c, 2);
    let later = soon(&c, 72);
    c.ledger
        .borrow_mut()
        .mark_out_with_kind(id, &first, "out of quota", HoldKind::Quota)
        .unwrap();
    c.advance(1000);
    c.ledger
        .borrow_mut()
        .mark_out_with_kind(id, &later, "out of quota", HoldKind::Quota)
        .unwrap();
    assert_eq!(t.out_until("zeus").as_deref(), Some(first.as_str()));
}

const INTERVAL: i64 = 30 * 60_000;
fn backup(c: &Context, t: &Tiers, who: &str, model: &str) -> ParticipantView {
    c.host.exits_showing(Some(0), None, None);
    c.ledger
        .borrow_mut()
        .add_backup(
            t.project.id,
            who,
            &NewMember {
                harness: "claude-code".into(),
                model: Some(model.into()),
                effort: None,
                model_name: None,
                designer: false,
                roles: vec!["worker".into()],
                tier: "standard".into(),
            },
        )
        .unwrap()
}
fn owner(t: &Tiers, n: i64) -> String {
    let p = t.participant(&t.assignee(n)).unwrap();
    t.context
        .project(t.project.id)
        .participants
        .iter()
        .find(|row| Some(row.id) == p.member_id)
        .map_or(p.handle, |row| row.handle.clone())
}
fn refuse(c: &Context, s: &str) {
    c.adapter.refuse(
        s,
        Quota::Unavailable {
            at: Some(now(c)),
            because: "Sign in again".into(),
        },
    );
}
fn retry(c: &Context, t: &Tiers) {
    c.back_from_quota(t.project.id, "zeus").unwrap();
}
fn edit(c: &Context, t: &Tiers, model: &str) {
    c.ledger
        .borrow_mut()
        .edit_member(
            t.project.id,
            "zeus",
            &cf_ledger::MemberEdit {
                harness: "claude-code".into(),
                model: Some(model.into()),
                model_name: Some("Zeus".into()),
                effort: None,
                tier: "standard".into(),
            },
        )
        .unwrap();
}
fn edit_working_member(prior: bool, change_back: bool) {
    let c = Context::new();
    let t = Tiers::new(&c, &["zeus"]);
    let backup = backup(&c, &t, "zeus", "gaia");
    edit(&c, &t, "claude-fable-5");
    if prior {
        c.send_on_vacation(t.project.id, &backup.handle).unwrap();
    }
    t.open();
    passes(&c, 6);
    let s = t.assignee(1);
    if prior {
        c.advance(1000);
        refuse(&c, &s);
        passes(&c, 5);
        assert_eq!(t.task(1).task.state, "paused");
    }
    edit(&c, &t, "claude-sonnet-5");
    if change_back {
        edit(&c, &t, "claude-fable-5");
    }
    passes(&c, 6);
    if prior {
        // Match the ability to start the next real turn even on old main, where an edit alone did not release.
        if t.task(1).task.state != "working" {
            retry(&c, &t);
            passes(&c, 5);
        }
        c.bring_back_from_vacation(t.project.id, &backup.handle)
            .unwrap();
    }
    assert_eq!(t.task(1).task.state, "working");
    for _ in 0..3 {
        c.advance(1000);
        refuse(&c, &s);
        passes(&c, 6);
    }
    t.open();
    passes(&c, 6);
    for _ in 0..3 {
        c.advance(INTERVAL + 1);
        passes(&c, 6);
    }
    assert_eq!(
        owner(&t, 1),
        backup.handle,
        "the already-running task refused real work and its own backup is available"
    );
}
#[test]
fn refusal_in_working_window_after_model_edit_preserves_its_task() {
    edit_working_member(false, false);
}
#[test]
fn refusal_in_working_window_after_editing_back_to_same_model() {
    edit_working_member(false, true);
}
#[test]
fn fresh_refusal_in_window_resumed_by_a_model_edit() {
    edit_working_member(true, false);
}

#[test]
fn an_edit_returns_its_held_task_like_the_selected_button() {
    let mut outcomes = Vec::new();
    for edit in [false, true] {
        let c = Context::new();
        let t = Tiers::new(&c, &["zeus"]);
        t.open();
        passes(&c, 4);
        let id = t.id("zeus");
        let until = "9999-12-31T23:59:59.998Z";
        c.ledger
            .borrow_mut()
            .hold_task(t.project.id, 1, until, "account unavailable")
            .unwrap();
        c.ledger
            .borrow_mut()
            .mark_out_with_kind(id, until, "account unavailable", HoldKind::Account)
            .unwrap();
        if edit {
            c.ledger
                .borrow_mut()
                .edit_member(
                    t.project.id,
                    "zeus",
                    &MemberEdit {
                        harness: "claude-code".into(),
                        model: Some("new-model".into()),
                        model_name: None,
                        effort: None,
                        tier: "standard".into(),
                    },
                )
                .unwrap();
        } else {
            c.back_from_quota(t.project.id, "zeus").unwrap();
        }
        let (member_out, reason): (Option<String>, Option<String>) = c.ledger.borrow().sqlite().query_row(
            "SELECT out_until, (SELECT json_extract(data, '$.because') FROM event WHERE kind='member.back' ORDER BY id DESC LIMIT 1) FROM participant WHERE id=?",
            [id], |row| Ok((row.get(0)?, row.get(1)?)),
        ).unwrap();
        outcomes.push((member_out, t.task(1).task.held_until, reason));
    }
    assert_eq!(
        outcomes[0], outcomes[1],
        "an edit returns the hold as the button does"
    );
}

#[test]
fn an_effort_only_edit_leaves_a_quota_hold_in_place() {
    let c = Context::new();
    let t = Tiers::new(&c, &["zeus"]);
    let member = t.participant("zeus").unwrap();
    let id = t.id("zeus");
    let until = soon(&c, 72);
    c.ledger
        .borrow_mut()
        .mark_out_with_kind(id, &until, "out of quota", HoldKind::Quota)
        .unwrap();
    let edited = c
        .ledger
        .borrow_mut()
        .edit_member(
            t.project.id,
            "zeus",
            &MemberEdit {
                harness: member.harness.unwrap(),
                model: member.model,
                model_name: member.model_name,
                effort: Some("high".into()),
                tier: "standard".into(),
            },
        )
        .unwrap();
    assert_eq!(edited.out_until.as_deref(), Some(until.as_str()));
}

#[test]
fn new_typed_refusal_on_a_previously_untyped_hold_reaches_backup() {
    let c = Context::new();
    let t = Tiers::new(&c, &["zeus"]);
    let b = backup(&c, &t, "zeus", "gaia");
    t.open();
    c.give(t.project.id, "zeus", "Second old window");
    passes(&c, 6);
    let first = t.assignee(1);
    let second = t.assignee(2);
    c.advance(1000);
    c.send_on_vacation(t.project.id, &b.handle).unwrap();
    let old = soon(&c, 72);
    c.adapter.refuse(
        &second,
        Quota::Exhausted {
            at: Some(now(&c)),
            resets_at: Some(old.clone()),
        },
    );
    passes(&c, 4);
    let member = t.id("zeus");
    // 0039 backfills a genuine finite pre-0039 mark to NULL. Preserve its actual quota event.
    c.ledger
        .borrow()
        .sqlite()
        .execute("UPDATE participant SET out_kind=NULL WHERE id=?", [member])
        .unwrap();
    c.bring_back_from_vacation(t.project.id, &b.handle).unwrap();
    c.advance(1000);
    refuse(&c, &first);
    passes(&c, 7);
    let now_owner = owner(&t, 1);
    let kind = c.ledger.borrow().hold_kind(member).unwrap();
    t.open();
    passes(&c, 5);
    let fresh = t.task(3).task.assignee.map(|_| owner(&t, 3));
    for _ in 0..3 {
        c.advance(INTERVAL + 1);
        passes(&c, 5);
    }
    assert_eq!(now_owner, b.handle, "the refused task reaches its backup");
    assert_eq!(kind, Some(HoldKind::Account));
    assert_eq!(fresh.as_deref(), Some(b.handle.as_str()));
    assert_eq!(owner(&t, 1), b.handle);
}

#[test]
fn editing_does_not_end_a_typed_busy_or_moved_hold() {
    for (kind, until) in [
        (HoldKind::Busy, "2026-09-19T12:01:00.000Z"),
        (HoldKind::Moved, "9999-12-31T23:59:59.999Z"),
    ] {
        let c = Context::new();
        let t = Tiers::new(&c, &["zeus"]);
        let id = t.id("zeus");
        c.ledger
            .borrow_mut()
            .mark_out_with_kind(id, until, "fixture hold", kind)
            .unwrap();
        let edited = c
            .ledger
            .borrow_mut()
            .edit_member(
                t.project.id,
                "zeus",
                &MemberEdit {
                    harness: "claude-code".into(),
                    model: Some("new-model".into()),
                    model_name: None,
                    effort: None,
                    tier: "standard".into(),
                },
            )
            .unwrap();
        assert_eq!(edited.out_until.as_deref(), Some(until));
    }
}

const NEVER: &str = "9999-12-31T23:59:59.998Z";
const MOVED: &str = "9999-12-31T23:59:59.999Z";
fn busy(c: &Context, session: &str) {
    c.adapter.refuse(
        session,
        Quota::Busy {
            at: Some(now(c)),
            because: "Model is still at capacity".into(),
        },
    );
}
fn hold(c: &Context, t: &Tiers, who: &str, until: &str, kind: HoldKind) {
    let id = t.id(who);
    c.ledger
        .borrow_mut()
        .mark_out_with_kind(id, until, "held in review fixture", kind)
        .unwrap();
}
fn answer_from_moved_member(other: &str) {
    let c = Context::new();
    c.adapter.one_account.set(true);
    let t = Tiers::new(&c, &["zeus", "diana"]);
    t.open();
    passes(&c, 5);
    assert_eq!(owner(&t, 1), "zeus");
    let still_working = t.assignee(1);
    c.give(
        t.project.id,
        "zeus",
        "Separate named Zeus window that moves",
    );
    c.give(t.project.id, "diana", "Diana's named work");
    passes(&c, 6);
    let zeus = t.assignee(2);
    let diana = t.assignee(3);
    assert_ne!(zeus, still_working);
    c.advance(1000);
    c.adapter.refuse(
        &zeus,
        Quota::Moved {
            at: Some(now(&c)),
            because: "the harness switched this window's model".into(),
        },
    );
    passes(&c, 4);
    assert_eq!(t.out_until("zeus").as_deref(), Some(MOVED));
    c.advance(1000);
    if other == "diana" {
        busy(&c, &diana);
        passes(&c, 4);
    } else {
        hold(&c, &t, "chief", NEVER, HoldKind::Account);
    }
    let until = t.out_until(other).unwrap();
    c.advance(1000);
    c.adapter.writes(
        &still_working,
        "Progress from the sibling still working on Zeus",
        &now(&c),
    );
    passes(&c, 5);
    let end = t.out_until(other);
    assert_eq!(end, Some(until));
}
#[test]
fn moved_members_answer_preserves_another_members_busy_deadline() {
    answer_from_moved_member("diana");
}
#[test]
fn moved_members_answer_preserves_the_chiefs_login_hold() {
    answer_from_moved_member("chief");
}

#[test]
fn a_new_quota_refusal_can_shorten_an_account_hold_at_its_reported_reset() {
    let c = Context::new();
    let t = Tiers::new(&c, &["zeus"]);
    t.open();
    c.give(t.project.id, "zeus", "Another active window");
    passes(&c, 5);
    let first = t.assignee(1);
    let second = t.assignee(2);
    c.advance(1000);
    c.adapter.refuse(
        &first,
        Quota::Unavailable {
            at: Some(now(&c)),
            because: "Sign in again".into(),
        },
    );
    passes(&c, 4);
    assert_eq!(t.out_until("zeus").as_deref(), Some(NEVER));
    c.advance(1000);
    let earlier = after(&c, 60 * 60_000);
    c.adapter.refuse(
        &second,
        Quota::Exhausted {
            at: Some(now(&c)),
            resets_at: Some(earlier.clone()),
        },
    );
    passes(&c, 5);
    assert_eq!(t.out_until("zeus").as_deref(), Some(earlier.as_str()));
}

#[test]
fn editing_does_not_end_a_moved_deadline_even_if_its_kind_is_stale() {
    let c = Context::new();
    let t = Tiers::new(&c, &["zeus"]);
    let id = t.id("zeus");
    c.ledger
        .borrow_mut()
        .mark_out_with_kind(id, MOVED, "stale kind", HoldKind::Quota)
        .unwrap();
    let edited = c
        .ledger
        .borrow_mut()
        .edit_member(
            t.project.id,
            "zeus",
            &MemberEdit {
                harness: "claude-code".into(),
                model: Some("new-model".into()),
                model_name: None,
                effort: None,
                tier: "standard".into(),
            },
        )
        .unwrap();
    assert_eq!(edited.out_until.as_deref(), Some(MOVED));
}

#[test]
fn editing_an_account_deadline_returns_it_even_if_its_kind_is_stale() {
    let c = Context::new();
    let t = Tiers::new(&c, &["zeus"]);
    let id = t.id("zeus");
    c.ledger
        .borrow_mut()
        .mark_out_with_kind(id, NEVER, "stale kind", HoldKind::Busy)
        .unwrap();
    let edited = c
        .ledger
        .borrow_mut()
        .edit_member(
            t.project.id,
            "zeus",
            &MemberEdit {
                harness: "claude-code".into(),
                model: Some("new-model".into()),
                model_name: None,
                effort: None,
                tier: "standard".into(),
            },
        )
        .unwrap();
    assert_eq!(edited.out_until, None);
}

#[test]
fn a_new_quota_refusal_does_not_shorten_a_moved_deadline_with_a_stale_kind() {
    let c = Context::new();
    let t = Tiers::new(&c, &["zeus"]);
    let id = t.id("zeus");
    c.ledger
        .borrow_mut()
        .mark_out_with_kind(id, MOVED, "stale kind", HoldKind::Quota)
        .unwrap();
    let earlier = after(&c, 60_000);
    c.ledger
        .borrow_mut()
        .mark_out_with_kind(id, &earlier, "out of quota", HoldKind::Quota)
        .unwrap();
    assert_eq!(t.out_until("zeus").as_deref(), Some(MOVED));
}

#[test]
fn a_new_quota_refusal_shortens_an_account_deadline_with_a_stale_kind() {
    let c = Context::new();
    let t = Tiers::new(&c, &["zeus"]);
    let id = t.id("zeus");
    c.ledger
        .borrow_mut()
        .mark_out_with_kind(id, NEVER, "stale kind", HoldKind::Busy)
        .unwrap();
    let earlier = after(&c, 60_000);
    c.ledger
        .borrow_mut()
        .mark_out_with_kind(id, &earlier, "out of quota", HoldKind::Quota)
        .unwrap();
    assert_eq!(t.out_until("zeus").as_deref(), Some(earlier.as_str()));
}

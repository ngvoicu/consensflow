//! Hold recovery: the original deadline, selected member, login metadata and edits.
use super::fixtures::{after, now, soon, Tiers};
use cf_engine::testing::Context;
use cf_harness::records::{Level, Quota};
use cf_ledger::{HoldKind, MemberEdit, NewMember};

fn passes(c: &Context, count: usize) {
    for _ in 0..count {
        c.pass().unwrap();
    }
}

#[test]
fn try_again_now_releases_only_the_pressed_member_on_a_shared_login() {
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
    assert_eq!(t.out_until("diana").as_deref(), Some(far.as_str()));
}

#[test]
fn try_again_does_not_shorten_a_separate_busy_task_hold() {
    for duration in [60_000, 30 * 60_000] {
        let c = Context::new();
        let t = Tiers::new(&c, &["zeus"]);
        t.open();
        passes(&c, 4);
        let busy_end = after(&c, duration);
        c.ledger
            .borrow_mut()
            .hold_task(t.project.id, 1, &busy_end, "model busy")
            .unwrap();
        let id = t.id("zeus");
        let account_end = after(&c, 30 * 60_000);
        c.ledger
            .borrow_mut()
            .mark_out_with_kind(id, &account_end, "account unavailable", HoldKind::Account)
            .unwrap();
        c.back_from_quota(t.project.id, "zeus").unwrap();
        assert_eq!(
            t.task(1).task.held_until.as_deref(),
            Some(busy_end.as_str())
        );
    }
}

#[test]
fn a_different_window_answer_does_not_release_the_members_hold() {
    let c = Context::new();
    let t = Tiers::new(&c, &["zeus"]);
    t.open();
    c.give(t.project.id, "zeus", "A second window");
    passes(&c, 5);
    let first = t.assignee(1);
    let second = t.assignee(2);
    c.advance(1000);
    c.adapter.refuse(
        &second,
        Quota::Unavailable {
            at: Some(now(&c)),
            because: "Sign in again".into(),
        },
    );
    passes(&c, 3);
    let until = t.out_until("zeus").unwrap();
    c.advance(1000);
    c.adapter
        .answer(&first, "Work that began before the refusal is complete.");
    let answered_at = now(&c);
    c.adapter.with(&first, |agent| {
        agent.items.last_mut().unwrap().at = Some(serde_json::json!(answered_at));
    });
    passes(&c, 5);
    assert_eq!(t.out_until("zeus").as_deref(), Some(until.as_str()));
    assert!(
        t.task(1)
            .messages
            .iter()
            .any(|m| m.kind == "result" && m.body.contains("Work that began")),
        "the existing result still reaches the chief"
    );
}

#[test]
fn a_held_window_that_continues_on_its_own_delivers_its_result_without_releasing_the_member() {
    let c = Context::new();
    let t = Tiers::new(&c, &["zeus"]);
    t.open();
    passes(&c, 4);
    let session = t.assignee(1);
    c.advance(1_000);
    c.adapter.refuse(
        &session,
        Quota::Unavailable {
            at: Some(now(&c)),
            because: "Sign in again".into(),
        },
    );
    passes(&c, 3);
    let until = t.out_until("zeus").unwrap();
    c.advance(1_000);
    c.adapter
        .begins_its_own_turn(&session, "Continue the work", &now(&c));
    c.advance(1_000);
    c.adapter
        .ends_its_own_turn(&session, "The original work is finished.", &now(&c));
    passes(&c, 5);
    assert!(t
        .task(1)
        .messages
        .iter()
        .any(|m| m.kind == "result" && m.body.contains("The original work is finished.")));
    assert_eq!(t.out_until("zeus").as_deref(), Some(until.as_str()));
}

#[test]
fn an_account_usage_reading_does_not_release_its_hold() {
    let c = Context::new();
    let t = Tiers::new(&c, &["zeus"]);
    t.open();
    passes(&c, 4);
    let s = t.assignee(1);
    c.advance(1000);
    c.adapter.refuse(
        &s,
        Quota::Unavailable {
            at: Some(now(&c)),
            because: "Sign in again".into(),
        },
    );
    passes(&c, 3);
    let until = t.out_until("zeus").unwrap();
    c.adapter.with(&s, |agent| {
        agent.quota = Some(
            Quota::Usage {
                level: Level::Ok,
                used_percent: Some(3.into()),
                resets_at: None,
            }
            .into(),
        )
    });
    passes(&c, 4);
    assert_eq!(t.out_until("zeus").as_deref(), Some(until.as_str()));
}

#[test]
fn changed_login_metadata_ends_the_old_hold() {
    let c = Context::new();
    c.adapter.login_revision.set(Some(1));
    let t = Tiers::new(&c, &["zeus"]);
    t.open();
    passes(&c, 5);
    c.advance(1000);
    c.adapter.refuse(
        &t.assignee(1),
        Quota::Unavailable {
            at: Some(now(&c)),
            because: "Sign in again".into(),
        },
    );
    passes(&c, 4);
    assert!(t.out_until("zeus").is_some());
    c.adapter.login_revision.set(Some(2));
    c.advance(11_000);
    passes(&c, 5);
    assert_eq!(
        t.out_until("zeus"),
        None,
        "a changed login ends the old hold"
    );
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
fn an_old_models_window_cannot_hold_the_edited_member() {
    let c = Context::new();
    let t = Tiers::new(&c, &["zeus"]);
    let id = t.id("zeus");
    c.ledger
        .borrow()
        .sqlite()
        .execute(
            "UPDATE participant SET model='claude-fable-5' WHERE id=?",
            [id],
        )
        .unwrap();
    c.give(t.project.id, "zeus", "Keep the old model working");
    passes(&c, 4);
    let old = t.assignee(1);
    let edited = c
        .ledger
        .borrow_mut()
        .edit_member(
            t.project.id,
            "zeus",
            &MemberEdit {
                harness: "claude-code".into(),
                model: Some("claude-sonnet-5".into()),
                model_name: Some("Zeus".into()),
                effort: None,
                tier: "standard".into(),
            },
        )
        .unwrap();
    c.advance(1000);
    c.adapter.refuse(
        &old,
        Quota::Moved {
            at: Some(now(&c)),
            because: "The old model moved".into(),
        },
    );
    passes(&c, 4);
    assert_eq!(
        t.out_until(&edited.handle),
        None,
        "a refusal from the old model cannot hold the edited member"
    );
}

#[test]
fn a_busy_refusal_still_holds_when_login_metadata_changed() {
    let c = Context::new();
    c.adapter.login_revision.set(Some(1));
    let t = Tiers::new(&c, &["zeus"]);
    t.open();
    passes(&c, 5);
    let session = t.assignee(1);
    c.adapter.login_revision.set(Some(2));
    c.advance(1_000);
    c.adapter.refuse(
        &session,
        Quota::Busy {
            at: Some(now(&c)),
            because: "Model at capacity".into(),
        },
    );
    passes(&c, 4);
    assert_eq!(
        c.ledger.borrow().hold_kind(t.id("zeus")).unwrap(),
        Some(HoldKind::Busy),
        "login polling must not change busy handling"
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

fn add_backup(c: &Context, t: &Tiers) -> cf_ledger::ParticipantView {
    c.host.exits_showing(Some(0), None, None);
    c.ledger
        .borrow_mut()
        .add_backup(
            t.project.id,
            "zeus",
            &NewMember {
                harness: "claude-code".into(),
                model: Some("gaia".into()),
                effort: None,
                model_name: None,
                designer: false,
                roles: vec!["worker".into()],
                tier: "standard".into(),
            },
        )
        .unwrap()
}
fn owner(t: &Tiers, number: i64) -> Option<String> {
    let assignee = t.task(number).task.assignee?;
    let session = t.participant(&assignee)?;
    t.context
        .project(t.project.id)
        .participants
        .into_iter()
        .find(|m| Some(m.id) == session.member_id)
        .map(|m| m.handle)
        .or(Some(assignee))
}
fn fresh_login_refusal_after_change(prior_hold: bool) {
    let c = Context::new();
    c.adapter.login_revision.set(Some(1));
    let t = Tiers::new(&c, &["zeus"]);
    let backup = add_backup(&c, &t);
    c.send_on_vacation(t.project.id, &backup.handle).unwrap();
    t.open();
    passes(&c, 5);
    let session = t.assignee(1);
    if prior_hold {
        c.advance(1000);
        c.adapter.refuse(
            &session,
            Quota::Unavailable {
                at: Some(now(&c)),
                because: "Sign in again".into(),
            },
        );
        passes(&c, 5);
        assert_eq!(t.task(1).task.state, "paused");
    }
    c.adapter.login_revision.set(Some(2));
    c.advance(11_000);
    passes(&c, 6);
    assert_eq!(
        t.task(1).task.state,
        "working",
        "login change releases existing hold"
    );
    c.bring_back_from_vacation(t.project.id, &backup.handle)
        .unwrap();
    c.advance(1000);
    c.adapter.refuse(
        &session,
        Quota::Unavailable {
            at: Some(now(&c)),
            because: "Sign in again".into(),
        },
    );
    passes(&c, 10);
    assert!(
        t.out_until("zeus").is_some(),
        "a fresh refusal re-holds the member"
    );
    assert_eq!(
        owner(&t, 1).as_deref(),
        Some(backup.handle.as_str()),
        "the refused task goes to its backup"
    );
}
#[test]
fn fresh_refusal_after_login_recovery_reholds_and_reaches_backup() {
    fresh_login_refusal_after_change(true);
}
#[test]
fn fresh_refusal_after_login_change_during_work_reholds_and_reaches_backup() {
    fresh_login_refusal_after_change(false);
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
fn account_hold_note_says_a_login_change_can_end_it() {
    let c = Context::new();
    let t = Tiers::new(&c, &["zeus"]);
    t.open();
    passes(&c, 4);
    c.advance(1000);
    c.adapter.refuse(
        &t.assignee(1),
        Quota::Unavailable {
            at: Some(now(&c)),
            because: "Sign in again".into(),
        },
    );
    passes(&c, 4);
    let note = t.notes("chief").join("\n");
    assert!(!note.contains("nothing resets that"), "{note}");
    assert!(note.contains("login changes"), "{note}");
}

#[test]
fn pre_upgrade_hold_retains_its_end_after_restart() {
    for kind in ["quota", "account"] {
        let c = Context::new();
        let t = Tiers::new(&c, &["zeus"]);
        t.open();
        passes(&c, 4);
        let far = if kind == "account" {
            "9999-12-31T23:59:59.998Z".to_owned()
        } else {
            soon(&c, 72)
        };
        let id = t.id("zeus");
        c.ledger
            .borrow_mut()
            .hold_task(
                t.project.id,
                1,
                &far,
                if kind == "account" {
                    "account unavailable"
                } else {
                    "out of quota"
                },
            )
            .unwrap();
        c.edit_ledger(|file| {
            let db = rusqlite::Connection::open(file).unwrap();
            db.execute(
                "UPDATE participant SET out_until=?1, out_kind=?2 WHERE id=?3",
                (&far, kind, id),
            )
            .unwrap();
            for column in ["hold_login_revision", "setup_generation", "hold_generation"] {
                let sql = format!(
                    "SELECT COUNT(*) FROM pragma_table_info('participant') WHERE name='{column}'"
                );
                let count: i64 = db.query_row(&sql, [], |row| row.get(0)).unwrap();
                if count > 0 {
                    db.execute_batch(&format!("ALTER TABLE participant DROP COLUMN {column}"))
                        .unwrap();
                }
            }
            db.execute_batch("PRAGMA user_version=40").unwrap();
        });
        assert_eq!(t.out_until("zeus").as_deref(), Some(far.as_str()));
        assert_eq!(t.task(1).task.held_until.as_deref(), Some(far.as_str()));
        c.advance(31 * 60_000);
        c.edit_ledger(|_| {});
        assert_eq!(t.out_until("zeus").as_deref(), Some(far.as_str()));
        assert_eq!(t.task(1).task.held_until.as_deref(), Some(far.as_str()));
    }
}

#[test]
fn fresh_refusal_before_the_next_login_poll_keeps_the_new_hold() {
    let c = Context::new();
    c.adapter.login_revision.set(Some(1));
    let t = Tiers::new(&c, &["zeus"]);
    t.open();
    c.give(t.project.id, "zeus", "Other working window");
    passes(&c, 5);
    let first = t.assignee(1);
    let second = t.assignee(2);
    c.advance(1000);
    c.adapter.refuse(
        &first,
        Quota::Unavailable {
            at: Some(now(&c)),
            because: "old login".into(),
        },
    );
    passes(&c, 5);
    assert!(t.out_until("zeus").is_some());
    c.adapter.login_revision.set(Some(2));
    c.advance(1000);
    c.adapter.refuse(
        &second,
        Quota::Unavailable {
            at: Some(now(&c)),
            because: "new login".into(),
        },
    );
    passes(&c, 5);
    assert!(t.out_until("zeus").is_some());
    c.advance(11_000);
    passes(&c, 6);
    assert!(
        t.out_until("zeus").is_some(),
        "a refusal on the new login must survive its metadata poll"
    );
}

#[test]
fn an_earlier_refusal_shortens_a_legacy_hold_without_a_kind() {
    let c = Context::new();
    let t = Tiers::new(&c, &["zeus"]);
    let id = t.id("zeus");
    let later = soon(&c, 72);
    let earlier = soon(&c, 12);
    c.ledger
        .borrow_mut()
        .mark_out_with_kind(id, &later, "out of quota", HoldKind::Quota)
        .unwrap();
    c.ledger
        .borrow()
        .sqlite()
        .execute("UPDATE participant SET out_kind=NULL WHERE id=?", [id])
        .unwrap();
    c.advance(1000);
    c.ledger
        .borrow_mut()
        .mark_out_with_kind(id, &earlier, "out of quota", HoldKind::Quota)
        .unwrap();
    assert_eq!(t.out_until("zeus").as_deref(), Some(earlier.as_str()));
}

#[test]
fn an_inflight_old_login_look_cannot_release_a_new_hold_with_identical_times() {
    use std::{cell::Cell, rc::Rc};
    let c = Context::new();
    c.adapter.login_revision.set(Some(1));
    let t = Tiers::new(&c, &["zeus"]);
    t.open();
    passes(&c, 5);
    c.advance(1000);
    c.adapter.refuse(
        &t.assignee(1),
        Quota::Unavailable {
            at: Some(now(&c)),
            because: "old login".into(),
        },
    );
    passes(&c, 5);
    let old = t.participant("zeus").unwrap();
    let until = old.out_until.clone().unwrap();
    let seen = Rc::new(Cell::new(0));
    let count = Rc::clone(&seen);
    let gate = c.adapter.login_holds.hold(move |_| {
        count.set(count.get() + 1);
        count.get() == 2
    });
    c.advance(11_000);
    passes(&c, 2);
    let has_column: bool = c.ledger.borrow().sqlite().query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('participant') WHERE name='hold_generation')", [], |r| r.get(0)).unwrap();
    if has_column {
        assert_eq!(seen.get(), 2, "the second old metadata read is in flight");
    }
    c.back_from_quota(t.project.id, "zeus").unwrap();
    c.adapter.login_revision.set(Some(2));
    c.ledger
        .borrow_mut()
        .mark_out_with_kind(old.id, &until, "account unavailable", HoldKind::Account)
        .unwrap();
    c.ledger
        .borrow()
        .sqlite()
        .execute(
            "UPDATE participant SET out_since=?1 WHERE id=?2",
            (old.out_since.as_deref(), old.id),
        )
        .unwrap();
    if has_column {
        c.ledger
            .borrow()
            .sqlite()
            .execute(
                "UPDATE participant SET hold_login_revision='2' WHERE id=?",
                [old.id],
            )
            .unwrap();
    }
    gate.open();
    passes(&c, 5);
    assert_eq!(
        t.out_until("zeus").as_deref(),
        Some(until.as_str()),
        "the old look must not release the replacement hold"
    );
}

#[test]
fn a_legacy_busy_hold_is_not_released_by_login_metadata() {
    let c = Context::new();
    c.adapter.login_revision.set(Some(1));
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
    passes(&c, 4);
    c.adapter.login_revision.set(Some(2));
    c.advance(11_000);
    passes(&c, 5);
    assert_eq!(
        t.out_until("zeus").as_deref(),
        Some(until.as_str()),
        "a busy backoff is not a login hold"
    );
}

#[test]
fn a_refusal_on_a_changed_login_starts_a_fresh_reset_before_the_poll() {
    let c = Context::new();
    c.adapter.login_revision.set(Some(1));
    let t = Tiers::new(&c, &["zeus"]);
    t.open();
    c.give(t.project.id, "zeus", "Other window");
    passes(&c, 5);
    let first = t.assignee(1);
    let second = t.assignee(2);
    c.advance(1000);
    let old_reset = soon(&c, 2);
    c.adapter.refuse(
        &first,
        Quota::Exhausted {
            at: Some(now(&c)),
            resets_at: Some(old_reset),
        },
    );
    passes(&c, 5);
    c.adapter.login_revision.set(Some(2));
    c.advance(1000);
    let new_reset = soon(&c, 72);
    c.adapter.refuse(
        &second,
        Quota::Exhausted {
            at: Some(now(&c)),
            resets_at: Some(new_reset.clone()),
        },
    );
    passes(&c, 5);
    c.advance(11_000);
    passes(&c, 5);
    assert_eq!(
        t.out_until("zeus").as_deref(),
        Some(new_reset.as_str()),
        "the old login's earlier reset no longer applies"
    );
}

#[test]
fn another_members_answer_does_not_release_a_shared_login_hold() {
    let c = Context::new();
    c.adapter.one_account.set(true);
    let t = Tiers::new(&c, &["zeus", "diana"]);
    t.open();
    passes(&c, 4);
    let until = soon(&c, 72);
    let diana = t.id("diana");
    c.ledger
        .borrow_mut()
        .mark_out_with_kind(diana, &until, "out of quota", HoldKind::Quota)
        .unwrap();
    c.advance(1000);
    let zeus = t.assignee(1);
    c.adapter.answer(&zeus, "My work is done.");
    let answered = now(&c);
    c.adapter.with(&zeus, |agent| {
        agent.items.last_mut().unwrap().at = Some(serde_json::json!(answered))
    });
    passes(&c, 5);
    assert_eq!(
        t.out_until("diana").as_deref(),
        Some(until.as_str()),
        "a sibling's answer is not a login change"
    );
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

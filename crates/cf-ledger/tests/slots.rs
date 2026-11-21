//! The staff as slots (`architecture/staff-slots.md`): a role at a tier holds one member, and
//! the image designer's one slot has no tier. The rule on each write, the placement of a task,
//! Critical's rule by a role's effective slot, a keyed plan's saved answer, and the edit that
//! carries a member's tier to its backup's windows.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use cf_ledger::{
    open_ledger, Ledger, MemberEdit, NewChief, NewMember, NewNote, NewProject, NewTask, Options,
    PlanTask, ProjectView, TaskEdit, TaskPlaced,
};
use serde_json::json;

fn member(model: &str, roles: &[&str], tier: &str) -> NewMember {
    NewMember {
        model: None,
        effort: None,
        model_name: Some(model.into()),
        harness: "claude-code".into(),
        designer: false,
        roles: roles.iter().map(|role| (*role).to_owned()).collect(),
        tier: tier.into(),
    }
}

fn image(model: &str) -> NewMember {
    NewMember {
        harness: "codex".into(),
        designer: true,
        ..member(model, &["designer"], "light")
    }
}

fn request(staff: Vec<NewMember>) -> NewProject {
    NewProject {
        directory: "/work/app".into(),
        name: "app".into(),
        chief: NewChief {
            harness: "claude-code".into(),
            model: Some("apollo".into()),
            effort: None,
            model_name: None,
        },
        staff,
        gate: false,
        wake_up: None,
        specs: None,
        staff_rules: None,
        compaction: None,
        advisor_tool: false,
        prompt_suggestion: false,
        own_hands: false,
    }
}

fn ledger() -> (tempfile::TempDir, Ledger) {
    let dir = tempfile::tempdir().expect("a home");
    let ledger =
        open_ledger(&dir.path().join("consensflow.db"), Options::default()).expect("a ledger");
    (dir, ledger)
}

fn project(ledger: &mut Ledger, staff: Vec<NewMember>) -> ProjectView {
    ledger.create_project(&request(staff)).expect("a project")
}

/// A staff that seats several members in a slot, as the update leaves one.
fn overflowing(ledger: &mut Ledger, staff: Vec<NewMember>) -> ProjectView {
    ledger
        .create_project_overflowing(&request(staff))
        .expect("a project")
}

fn said(error: cf_ledger::LedgerError) -> (String, String) {
    match error {
        cf_ledger::LedgerError::Refused(refusal) => {
            (refusal.code.to_string(), refusal.message.clone())
        }
        other => panic!("not a refusal: {other}"),
    }
}

fn id(ledger: &Ledger, project: i64, handle: &str) -> i64 {
    ledger
        .project(project)
        .unwrap()
        .unwrap()
        .participants
        .into_iter()
        .find(|participant| participant.handle == handle)
        .unwrap_or_else(|| panic!("no @{handle}"))
        .id
}

fn tier_of(ledger: &Ledger, project: i64, handle: &str) -> Option<String> {
    ledger
        .project(project)
        .unwrap()
        .unwrap()
        .participants
        .into_iter()
        .find(|participant| participant.handle == handle)
        .and_then(|participant| participant.tier)
}

fn work(pool: &str, tier: &str, purpose: Option<&str>) -> NewTask {
    NewTask {
        from: "chief".into(),
        pool: Some(pool.into()),
        tier: Some(tier.into()),
        purpose: purpose.map(str::to_owned),
        body: "Do the work".into(),
        ..NewTask::default()
    }
}

#[test]
fn each_write_is_refused_in_the_words_of_the_rule() {
    let (_dir, mut ledger) = ledger();
    let project = project(
        &mut ledger,
        vec![
            member("zeus", &["worker"], "standard"),
            member("hera", &["reviewer"], "critical"),
            image("iris"),
        ],
    );
    // Add: a tier its role has no slot at.
    let refused = ledger
        .add_member(project.id, &member("ares", &["worker"], "critical"))
        .unwrap_err();
    assert_eq!(
        said(refused),
        (
            "tier-not-offered".to_owned(),
            "a worker is complex, standard or light, never critical: critical work is not coding. Pick one of those, or add this model again as a second member".to_owned()
        )
    );
    // Add: a slot another holds.
    let refused = ledger
        .add_member(project.id, &member("ares", &["worker"], "standard"))
        .unwrap_err();
    assert_eq!(
        said(refused),
        (
            "slot-taken".to_owned(),
            "the standard worker slot is @zeus's: a role and tier holds one member. Pick another tier".to_owned()
        )
    );
    // A second image designer.
    let refused = ledger.add_member(project.id, &image("pallas")).unwrap_err();
    assert_eq!(
        said(refused),
        (
            "slot-taken".to_owned(),
            "the image designer slot is @iris's: a project has one".to_owned()
        )
    );
    // A role gained: each at the member's tier.
    let refused = ledger
        .set_roles(project.id, "hera", &["reviewer", "worker"])
        .unwrap_err();
    assert_eq!(said(refused).0, "tier-not-offered");
    ledger
        .add_member(project.id, &member("ares", &["worker"], "light"))
        .expect("a light worker");
    let refused = ledger
        .set_roles(project.id, "ares", &["worker", "reviewer"])
        .map(|_| ())
        .unwrap_or(());
    assert_eq!(refused, (), "a light reviewer slot is free");
    // Edit to another tier: each role at the new tier.
    let edit = |tier: &str| MemberEdit {
        harness: "claude-code".into(),
        model: None,
        effort: None,
        model_name: Some("ares".into()),
        tier: tier.into(),
    };
    let refused = ledger.edit_member(project.id, "ares", &edit("standard")).unwrap_err();
    assert_eq!(said(refused).0, "slot-taken");
    let refused = ledger.edit_member(project.id, "ares", &edit("critical")).unwrap_err();
    assert_eq!(said(refused).0, "tier-not-offered");
}

#[test]
fn new_projects_list_is_checked_whole_as_slots() {
    let (_dir, mut ledger) = ledger();
    let refused = ledger
        .create_project(&request(vec![
            member("zeus", &["worker"], "standard"),
            member("diana", &["worker"], "standard"),
        ]))
        .unwrap_err();
    let (code, words) = said(refused);
    assert_eq!(code, "slot-taken");
    assert_eq!(
        words,
        "the staff names two standard workers, zeus and diana: a role and tier holds one member"
    );
    let refused = ledger
        .create_project(&request(vec![
            member("zeus", &["worker", "architect"], "critical"),
        ]))
        .unwrap_err();
    assert_eq!(said(refused).0, "tier-not-offered");
    let refused = ledger
        .create_project(&request(vec![image("iris"), image("pallas")]))
        .unwrap_err();
    assert_eq!(said(refused).0, "slot-taken");
    // A staff that names each slot once, the image designer apart, is made.
    ledger
        .create_project(&request(vec![
            member("zeus", &["worker"], "standard"),
            member("diana", &["worker"], "light"),
            member("hera", &["reviewer"], "critical"),
            image("iris"),
        ]))
        .expect("each slot once");
}

#[test]
fn a_slot_several_hold_stays_editable_and_every_way_of_meeting_the_rule_is_open() {
    let (_dir, mut ledger) = ledger();
    let project = overflowing(
        &mut ledger,
        vec![
            member("zeus", &["worker", "reviewer"], "standard"),
            member("diana", &["worker"], "standard"),
            member("hera", &["worker"], "standard"),
        ],
    );
    let edit = |model: &str, tier: &str| MemberEdit {
        harness: "claude-code".into(),
        model: None,
        effort: None,
        model_name: Some(model.into()),
        tier: tier.into(),
    };
    // A model edit that keeps the tier is no refusal.
    ledger
        .edit_member(project.id, "diana", &edit("diana", "standard"))
        .expect("the tier is kept");
    // A role removed is none either.
    ledger
        .set_roles(project.id, "zeus", &["reviewer"])
        .expect("the worker role is given up");
    let holders = |ledger: &Ledger| -> Vec<String> {
        ledger
            .project(project.id)
            .unwrap()
            .unwrap()
            .participants
            .into_iter()
            .filter(|p| p.roles.iter().any(|role| role == "worker") && p.left_at.is_none())
            .map(|p| p.handle)
            .collect()
    };
    assert_eq!(holders(&ledger), ["diana", "hera"]);
    // The last but one to a free tier ends the overflow.
    ledger
        .edit_member(project.id, "hera", &edit("hera", "light"))
        .expect("a free tier");
    assert_eq!(tier_of(&ledger, project.id, "hera").as_deref(), Some("light"));
    // A removal of one of three leaves two.
    let project = overflowing(
        &mut ledger,
        vec![
            member("a", &["advisor"], "standard"),
            member("b", &["advisor"], "standard"),
            member("c", &["advisor"], "standard"),
        ],
    );
    ledger.remove_member(project.id, "b").expect("b leaves");
    let left: Vec<String> = ledger
        .project(project.id)
        .unwrap()
        .unwrap()
        .participants
        .into_iter()
        .filter(|p| p.roles.iter().any(|role| role == "advisor") && p.left_at.is_none())
        .map(|p| p.handle)
        .collect();
    assert_eq!(left, ["a", "c"]);
}

#[test]
fn a_model_added_at_a_second_tier_is_a_second_member_in_both_orders() {
    for (first, second) in [("standard", "light"), ("light", "standard")] {
        let (_dir, mut ledger) = ledger();
        let project = project(&mut ledger, vec![member("zeus", &["worker"], first)]);
        let again = ledger
            .add_member(project.id, &member("zeus", &["worker"], second))
            .expect("the same model at another tier");
        let kept = ledger
            .project(project.id)
            .unwrap()
            .unwrap()
            .participants
            .into_iter()
            .filter(|p| p.role == "worker" && p.member_id.is_none())
            .map(|p| (p.id, p.tier.unwrap_or_default()))
            .collect::<Vec<_>>();
        assert_eq!(kept.len(), 2, "{first} then {second}");
        assert_ne!(kept[0].0, kept[1].0);
        assert_eq!(kept[0].1, first, "the first keeps its tier");
        assert_eq!(again.tier.as_deref(), Some(second));
    }
}

#[test]
fn edit_member_carries_the_new_tier_to_its_backups_open_windows() {
    let (_dir, mut ledger) = ledger();
    let project = project(&mut ledger, vec![member("zeus", &["worker"], "standard")]);
    let backup = ledger
        .add_backup(project.id, "zeus", &member("hera", &["worker"], "standard"))
        .expect("a backup");
    // The backup has a window open: a session of its own.
    ledger
        .sqlite()
        .execute(
            "INSERT INTO participant (project_id, handle, role, roles, harness, tier, created_at, member_id)
             VALUES (?, 'hera-1', 'worker', '[\"worker\"]', 'claude-code', 'standard', 'now', ?)",
            rusqlite::params![project.id, backup.id],
        )
        .unwrap();
    ledger
        .edit_member(
            project.id,
            "zeus",
            &MemberEdit {
                harness: "claude-code".into(),
                model: None,
                effort: None,
                model_name: Some("zeus".into()),
                tier: "complex".into(),
            },
        )
        .expect("the tier moves");
    let tiers: Vec<(String, Option<String>)> = ledger
        .project(project.id)
        .unwrap()
        .unwrap()
        .participants
        .into_iter()
        .filter(|p| ["zeus", "hera", "hera-1"].contains(&p.handle.as_str()))
        .map(|p| (p.handle, p.tier))
        .collect();
    assert!(
        tiers.iter().all(|(_, tier)| tier.as_deref() == Some("complex")),
        "{tiers:?}"
    );
    assert_eq!(tiers.len(), 3);
}

#[test]
fn a_purpose_is_taken_only_with_critical_at_add_as_at_edit() {
    let (_dir, mut ledger) = ledger();
    let project = project(&mut ledger, vec![member("zeus", &["worker"], "standard")]);
    let refused = ledger
        .create_task(project.id, &work("worker", "light", Some("architecture")))
        .unwrap_err();
    assert_eq!(
        said(refused),
        (
            "purpose-without-critical".to_owned(),
            "a purpose comes with --tier critical".to_owned()
        )
    );
    // A worker's task is never critical.
    let refused = ledger
        .create_task(project.id, &work("worker", "critical", Some("architecture")))
        .unwrap_err();
    assert_eq!(
        said(refused),
        (
            "tier-not-offered".to_owned(),
            "a worker's task is complex, standard or light: critical work is not coding".to_owned()
        )
    );
    // No tier at all stays an invalid tier.
    let refused = ledger
        .create_task(
            project.id,
            &NewTask {
                tier: None,
                ..work("worker", "light", None)
            },
        )
        .unwrap_err();
    assert_eq!(said(refused).0, "invalid-tier");
}

#[test]
fn critical_goes_by_the_roles_effective_slot_so_an_old_image_designer_keeps_its_drawings() {
    let (_dir, mut ledger) = ledger();
    let project = project(
        &mut ledger,
        vec![
            member("zeus", &["worker"], "standard"),
            member("hera", &["researcher"], "critical"),
            member("ares", &["advisor"], "critical"),
            image("iris"),
        ],
    );
    let backup = ledger
        .add_backup(project.id, "hera", &member("pan", &["worker"], "light"))
        .expect("a backup of the researcher");
    // Alpha.3's Add member took Critical for an image designer: the row says so, and the
    // image backup inherits that tier.
    ledger
        .sqlite()
        .execute("UPDATE participant SET tier = 'critical' WHERE handle = 'iris'", [])
        .unwrap();
    let drawing = ledger
        .create_task(
            project.id,
            &NewTask {
                from: "chief".into(),
                pool: Some("designer".into()),
                body: "A logo".into(),
                ..NewTask::default()
            },
        )
        .expect("a drawing")
        .task
        .number;
    let iris = id(&ledger, project.id, "iris");
    ledger
        .check_taker(project.id, drawing, iris)
        .expect("the image designer is no Critical member for its drawings");
    ledger
        .assign_task(project.id, drawing, iris)
        .expect("a fresh drawing reaches the designer");
    // A held drawing may start on the designer's backup, which inherited the tier.
    let spare = ledger
        .add_backup(project.id, "iris", &image("luna"))
        .map(|view| view.id)
        .unwrap_or(0);
    if spare != 0 {
        ledger
            .sqlite()
            .execute("UPDATE participant SET tier = 'critical' WHERE id = ?", [spare])
            .unwrap();
        ledger
            .check_taker(project.id, drawing, spare)
            .expect("its image backup may take the drawing");
    }
    // A tierless research task, and an advisor's at Critical with no purpose, are still refused.
    ledger
        .sqlite()
        .execute(
            "INSERT INTO task (project_id, number, title, body, requester_id, state, pool, tier, created_at, updated_at)
             SELECT ?1, 90, 'Look', 'Look', id, 'open', 'researcher', NULL, 'now', 'now'
               FROM participant WHERE project_id = ?1 AND role = 'chief'",
            [project.id],
        )
        .unwrap();
    ledger
        .sqlite()
        .execute(
            "INSERT INTO task (project_id, number, title, body, requester_id, state, pool, tier, created_at, updated_at)
             SELECT ?1, 91, 'Ask', 'Ask', id, 'open', 'advisor', 'critical', 'now', 'now'
               FROM participant WHERE project_id = ?1 AND role = 'chief'",
            [project.id],
        )
        .unwrap();
    let hera = id(&ledger, project.id, "hera");
    let ares = id(&ledger, project.id, "ares");
    assert_eq!(
        said(ledger.check_taker(project.id, 90, hera).unwrap_err()).0,
        "critical-takes-critical-work"
    );
    assert_eq!(
        said(ledger.check_taker(project.id, 91, ares).unwrap_err()).0,
        "critical-takes-critical-work"
    );
    // And the researcher's backup follows its member's tier.
    assert_eq!(
        said(ledger.check_taker(project.id, 90, backup.id).unwrap_err()).0,
        "critical-takes-critical-work"
    );
}

#[test]
fn a_task_goes_to_its_slot_else_the_nearest_held_one_never_into_critical() {
    let (_dir, mut ledger) = ledger();
    let project = project(
        &mut ledger,
        vec![
            member("zeus", &["worker"], "light"),
            member("hera", &["reviewer"], "critical"),
            member("pan", &["tester"], "standard"),
        ],
    );
    // Its own slot is empty: the nearest held one, the next up before the next down.
    let made = ledger
        .create_task(project.id, &work("tester", "complex", None))
        .unwrap();
    assert_eq!(
        (made.task.tier.as_deref(), made.asked.as_deref()),
        (Some("standard"), Some("complex"))
    );
    // A task never falls into Critical: only a reviewer at Critical is held, so it waits.
    let review = ledger
        .create_task(project.id, &work("reviewer", "standard", None))
        .unwrap();
    assert_eq!(review.task.tier.as_deref(), Some("standard"));
    assert_eq!(review.asked, None);
    let placed = ledger.retier_open_task(project.id, review.task.number).unwrap();
    assert_eq!(
        placed,
        TaskPlaced::Waits {
            critical: vec!["hera".to_owned()]
        }
    );
    // Critical takes a task added there with its purpose.
    let critical = ledger
        .create_task(
            project.id,
            &work("reviewer", "critical", Some("critical-review")),
        )
        .unwrap();
    assert_eq!(critical.task.tier.as_deref(), Some("critical"));
    assert_eq!(
        ledger.retier_open_task(project.id, critical.task.number).unwrap(),
        TaskPlaced::Stays
    );
    // With nobody at Critical, it falls to the nearest tier held, keeping its purpose.
    ledger.remove_member(project.id, "hera").unwrap();
    let fell = ledger.retier_open_task(project.id, critical.task.number).unwrap();
    assert!(matches!(fell, TaskPlaced::Waits { ref critical } if critical.is_empty()), "{fell:?}");
    let advice = ledger
        .create_task(project.id, &work("tester", "critical", Some("hard-problem")))
        .unwrap();
    assert_eq!(advice.task.tier.as_deref(), Some("standard"));
    assert_eq!(advice.task.purpose.as_deref(), Some("hard-problem"));
}

fn section(name: &str, task: NewTask) -> PlanTask {
    PlanTask {
        name: name.into(),
        task,
        needs: Vec::new(),
        before: Vec::new(),
    }
}

#[test]
fn a_plans_saved_answer_is_looked_up_before_its_sections_and_carries_the_tier_each_went_to() {
    let (_dir, mut ledger) = ledger();
    let project = project(
        &mut ledger,
        vec![
            member("zeus", &["worker"], "standard"),
            member("pan", &["tester"], "standard"),
            member("argus", &["researcher"], "standard"),
        ],
    );
    // Each section's answer carries its effective tier: a Complex request that fell to
    // Standard reads Standard, with the tier asked and who holds the one it went to.
    let plan = [section("check", work("tester", "complex", None))];
    let made = ledger.create_plan(project.id, &plan, Some("k"), "one").unwrap();
    let got = &made.tasks[0];
    assert_eq!(
        (got.tier.as_deref(), got.asked.as_deref(), got.held_by.clone()),
        (Some("standard"), Some("complex"), vec!["pan".to_owned()])
    );
    // The holder moves away; the same key replays the answer as it was given.
    ledger.remove_member(project.id, "pan").unwrap();
    ledger
        .add_member(project.id, &member("tess", &["tester"], "light"))
        .unwrap();
    let again = ledger.create_plan(project.id, &plan, Some("k"), "one").unwrap();
    assert!(again.repeated);
    assert_eq!(again.tasks, made.tasks, "the original destination, not a recomputed one");
    // An answer an earlier version kept (name and number alone) still replays, and a plan
    // that section had no tier replays instead of being refused.
    ledger
        .sqlite()
        .execute(
            "INSERT INTO task_plan_key (project_id, key, fingerprint, tasks) VALUES (?, 'old', 'f', '[[\"lookup\", 7]]')",
            [project.id],
        )
        .unwrap();
    let legacy = [section(
        "lookup",
        NewTask {
            from: "chief".into(),
            pool: Some("researcher".into()),
            tier: None,
            body: "Look it up".into(),
            ..NewTask::default()
        },
    )];
    // The fingerprint is the ledger's own: a saved key under its hash replays, whatever the
    // sections say; a fresh key refuses the file for its tier.
    let refused = ledger.create_plan(project.id, &legacy, Some("fresh"), "g").unwrap_err();
    assert_eq!(said(refused).0, "invalid-tier");
    let fingerprint = {
        use sha2::{Digest, Sha256};
        let encoded = serde_json::to_vec(&(&legacy[..], "g")).unwrap();
        format!("{:x}", Sha256::digest(encoded))
    };
    ledger
        .sqlite()
        .execute(
            "UPDATE task_plan_key SET fingerprint = ? WHERE key = 'old'",
            [fingerprint],
        )
        .unwrap();
    let replayed = ledger.create_plan(project.id, &legacy, Some("old"), "g").unwrap();
    assert!(replayed.repeated);
    assert_eq!((replayed.tasks[0].name.as_str(), replayed.tasks[0].number), ("lookup", 7));
    assert_eq!(replayed.tasks[0].tier, None);
    // A changed file under the old key is still a different plan.
    let changed = [section(
        "lookup",
        NewTask {
            body: "Another".into(),
            ..legacy[0].task.clone()
        },
    )];
    let refused = ledger.create_plan(project.id, &changed, Some("old"), "g").unwrap_err();
    assert_eq!(said(refused).0, "key-used");
    let _ = json!(null);
}

#[test]
fn a_purpose_edit_rewrites_the_critical_line_of_a_queued_and_of_a_gated_brief() {
    for gate in [false, true] {
        let (_dir, mut ledger) = ledger();
        let mut make = request(vec![
            member("zeus", &["worker"], "standard"),
            member("hera", &["reviewer"], "critical"),
        ]);
        make.gate = gate;
        let project = ledger.create_project(&make).expect("a project");
        // A review queued for its window at Critical: given by tier, then the purpose changes
        // where it keeps its tier.
        let task = ledger
            .create_task(
                project.id,
                &work("reviewer", "critical", Some("critical-review")),
            )
            .unwrap()
            .task
            .number;
        let hera = id(&ledger, project.id, "hera");
        let moved = ledger.assign_task(project.id, task, hera).unwrap();
        let message = moved.message.expect("its brief").id;
        ledger
            .edit_task_with(
                project.id,
                task,
                "chief",
                &TaskEdit {
                    tier: Some("critical".into()),
                    purpose: Some("hard-problem".into()),
                    ..TaskEdit::default()
                },
            )
            .expect("a purpose, a tier alone: taken");
        let body = ledger.message(message).unwrap().unwrap();
        assert!(
            body.body.starts_with("Critical work: hard-problem."),
            "{} (gate {gate}, {})",
            body.body,
            body.state
        );
        let _ = NewNote {
            from: None,
            to: String::new(),
            task: None,
            body: String::new(),
        };
    }
}

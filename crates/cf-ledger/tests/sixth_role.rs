//! A role is one row of the roles' table (`cf_ledger::roles`): a sixth role put in it for
//! a test, and nowhere else, is a role the ledger's model holds, a pool a task is for,
//! said by its name in the refusals and in its place in their lists, and a role with no
//! tier where its row says so. The SQLite CHECKs of `participant.role` and `task.pool`
//! still name the five roles of today (migration 0012), and a migration is what changes
//! them: a test that writes a row of the sixth role turns them off on its own connection
//! (`PRAGMA ignore_check_constraints`), and one that is refused before it writes needs not.

#![allow(clippy::expect_used)]

use cf_ledger::model::{self, parse_roles, require_fitting_roles, require_roles};
use cf_ledger::roles::seam::with_a_scout;
use cf_ledger::{
    open_ledger, Ledger, LedgerError, NewChief, NewMember, NewProject, NewTask, Options,
    ProjectView, TaskEdit,
};
use serde_json::json;

fn said(error: LedgerError) -> (String, String) {
    match error {
        LedgerError::Refused(refusal) => (refusal.code.to_string(), refusal.message.clone()),
        other => panic!("not a refusal: {other}"),
    }
}

fn member(agent: &str, roles: &[&str], tier: &str) -> NewMember {
    NewMember {
        model: None,
        effort: None,
        model_name: Some(agent.into()),
        harness: "claude-code".into(),
        designer: false,
        roles: roles.iter().map(|role| (*role).to_owned()).collect(),
        tier: tier.into(),
    }
}

fn project(ledger: &mut Ledger, staff: Vec<NewMember>) -> Result<ProjectView, LedgerError> {
    ledger.create_project(&NewProject {
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
    })
}

fn for_scouts(body: &str) -> NewTask {
    NewTask::from_json(&json!({ "from": "chief", "pool": "scout", "body": body }))
        .expect("a scout's task takes no tier")
}

#[test]
fn a_role_in_the_table_alone_is_one_a_member_holds_and_the_refusals_list_it_in_its_place() {
    with_a_scout(|| {
        assert_eq!(
            require_roles(&["scout", "worker"], String::new).expect("roles"),
            ["scout", "worker"]
        );
        assert_eq!(
            parse_roles(Some(&json!(["scout"]))).expect("roles"),
            ["scout"]
        );
        assert_eq!(
            said(require_roles(&["captain"], || "captain".to_owned()).unwrap_err()).1,
            "a member is one or more of worker, advisor, reviewer, scout, tester, designer, not captain"
        );
        assert_eq!(
            said(
                NewTask::from_json(&json!({ "from": "chief", "pool": "captain", "body": "A" }))
                    .unwrap_err()
            )
            .1,
            "a pool is worker, advisor, reviewer, scout, tester, designer, not captain"
        );
        // Any agent may scout but an image agent, which is nothing but an image designer.
        assert!(require_fitting_roles("hera", false, &["scout"]).is_ok());
        assert_eq!(
            said(require_fitting_roles("pygmalion", true, &["scout"]).unwrap_err()).1,
            "pygmalion is an image agent, which can only be an image designer"
        );
        assert!(model::fits_role(false, "scout"));
    });
    // Without its row it is no role at all.
    assert_eq!(
        said(require_roles(&["scout"], || "scout".to_owned()).unwrap_err()).1,
        "a member is one or more of worker, frontend, architect, researcher, advisor, reviewer, tester, integrator, designer, not scout"
    );
}

#[test]
fn a_task_for_a_role_nobody_holds_is_refused_by_the_roles_name() {
    with_a_scout(|| {
        let dir = tempfile::tempdir().expect("a home");
        let mut ledger =
            open_ledger(&dir.path().join("consensflow.db"), Options::default()).expect("a ledger");
        let project =
            project(&mut ledger, vec![member("zeus", &["worker"], "standard")]).expect("a project");
        let refused = ledger
            .create_task(project.id, &for_scouts("Look around"))
            .unwrap_err();
        assert_eq!(
            said(refused),
            (
                "no-member-of-tier".to_owned(),
                "no field scout is on the staff: ask the human for one, in your terminal"
                    .to_owned()
            )
        );
    });
}

#[test]
fn a_role_with_no_tier_takes_a_task_with_none_and_its_tier_is_not_changed() {
    with_a_scout(|| {
        let dir = tempfile::tempdir().expect("a home");
        let mut ledger =
            open_ledger(&dir.path().join("consensflow.db"), Options::default()).expect("a ledger");
        ledger
            .sqlite()
            .execute_batch("PRAGMA ignore_check_constraints = ON")
            .expect("the CHECKs off");
        let project = project(
            &mut ledger,
            vec![
                member("zeus", &["worker"], "standard"),
                member("argus", &["scout"], "light"),
            ],
        )
        .expect("a project with a scout");
        let scout = project
            .participants
            .iter()
            .find(|participant| participant.role == "scout")
            .expect("the scout");
        assert_eq!(scout.roles, ["scout"]);
        let given = ledger
            .create_task(project.id, &for_scouts("Look around"))
            .expect("a scout's task");
        assert_eq!(
            (given.task.pool.as_deref(), given.task.tier.as_deref()),
            (Some("scout"), None)
        );
        let refused = ledger
            .edit_task_with(
                project.id,
                given.task.number,
                "chief",
                &TaskEdit {
                    tier: Some("complex".into()),
                    ..TaskEdit::default()
                },
            )
            .unwrap_err();
        assert_eq!(
            said(refused).1,
            "cannot change the tier of T-1: the field scout has no tier"
        );
    });
}

/// The words of the CHECK `name` of `table`'s schema, as the ledger has it after every
/// migration: the quoted words after its `IN (`.
fn checked(ledger: &Ledger, table: &str, name: &str) -> Vec<String> {
    let sql: String = ledger
        .sqlite()
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = ?",
            [table],
            |row| row.get(0),
        )
        .expect("the table's schema");
    let check = &sql[sql
        .find(name)
        .unwrap_or_else(|| panic!("no {name} in {sql}"))..];
    let list = &check[check.find("IN (").expect("a list") + 4..];
    list[..list.find(')').expect("its end")]
        .split(',')
        .map(|word| word.trim().trim_matches('\'').to_owned())
        .collect()
}

#[test]
fn the_ledgers_checks_hold_the_tables_roles_and_a_new_row_needs_a_migration() {
    // A role is a row of the table and a word of two CHECKs, which only a migration writes:
    // a row added without its migration fails here before any member of it is refused by
    // SQLite.
    let dir = tempfile::tempdir().expect("a home");
    let ledger =
        open_ledger(&dir.path().join("consensflow.db"), Options::default()).expect("a ledger");
    let roles = cf_ledger::roles::ids();
    let mut members = checked(&ledger, "participant", "participant_role_check");
    members.retain(|role| role != "human" && role != "chief");
    let sorted = |mut words: Vec<String>| {
        words.sort();
        words
    };
    let ours = sorted(roles.iter().map(|role| (*role).to_owned()).collect());
    assert_eq!(sorted(members.clone()), ours);
    assert_eq!(sorted(checked(&ledger, "task", "task_pool_check")), ours);
    // A table with the scout, a row that is no word of the CHECKs, is told apart by the same
    // comparison: this test is what fails when a row comes without its migration.
    with_a_scout(|| {
        let with_scout = sorted(
            cf_ledger::roles::ids()
                .iter()
                .map(|role| (*role).to_owned())
                .collect(),
        );
        assert_ne!(sorted(members.clone()), with_scout);
        assert!(with_scout.contains(&"scout".to_owned()), "{with_scout:?}");
    });
}

/// A member as JSON gives it: on claude-code, as `agent`, in `roles`, with the fields of
/// `more` (a tier, or none).
fn member_json(agent: &str, roles: &[&str], more: serde_json::Value) -> serde_json::Value {
    let mut member = json!({ "harness": "claude-code", "modelName": agent, "roles": roles });
    member
        .as_object_mut()
        .expect("a member")
        .extend(more.as_object().expect("its fields").clone());
    member
}

#[test]
fn a_member_whose_roles_have_no_tier_is_given_none_and_kept_in_the_lowest() {
    with_a_scout(|| {
        // Given no tier, a scout alone is kept in the lowest, as an image designer is:
        // none of its tasks has a tier, so it decides nothing.
        let scout = NewMember::from_json(&member_json("argus", &["scout"], json!({})))
            .expect("a scout takes no tier");
        assert_eq!(scout.tier, "light");
        let dir = tempfile::tempdir().expect("a home");
        let mut ledger =
            open_ledger(&dir.path().join("consensflow.db"), Options::default()).expect("a ledger");
        ledger
            .sqlite()
            .execute_batch("PRAGMA ignore_check_constraints = ON")
            .expect("the CHECKs off");
        let project = project(&mut ledger, vec![scout]).expect("a project with a scout");
        let argus = project
            .participants
            .iter()
            .find(|participant| participant.role == "scout")
            .expect("the scout");
        assert_eq!(argus.tier.as_deref(), Some("light"));
        // A role with tiers beside it asks for one, which is kept for the work of that role.
        for roles in [&["worker"][..], &["scout", "worker"][..]] {
            assert_eq!(
                said(NewMember::from_json(&member_json("zeus", roles, json!({}))).unwrap_err()),
                (
                    "invalid-tier".to_owned(),
                    "a tier is critical, complex, standard, light, not undefined".to_owned()
                ),
                "{roles:?}"
            );
        }
        let both = NewMember::from_json(&member_json(
            "zeus",
            &["scout", "worker"],
            json!({ "tier": "complex" }),
        ))
        .expect("a scout and a worker");
        assert_eq!(
            (both.roles.as_slice(), both.tier.as_str()),
            (&["scout".to_owned(), "worker".to_owned()][..], "complex")
        );
    });
}

#[test]
fn the_image_designer_given_no_tier_is_kept_in_the_lowest_as_its_row_says() {
    // The image designer's row has no tier: read from JSON with none, it is kept where the
    // page's Add member keeps it, in the lowest.
    let designer = NewMember::from_json(&json!({
        "harness": "codex", "roles": ["designer"], "designer": true
    }))
    .expect("an image designer takes no tier");
    assert_eq!((designer.tier.as_str(), designer.designer), ("light", true));
}

//! A role is one row of the roles' table (`cf_ledger::roles`): the scheduler gives a task
//! for a sixth role, put in the table for this test and nowhere else, to the members who
//! hold it, and a role whose row has no tier to any of them whatever theirs is; a task
//! whose role nobody holds any more waits, and says so by the role's name. The SQLite
//! CHECKs still name the five roles of today, so each test turns them off on its own
//! connection before it writes a member or a task of the sixth.

use cf_engine::testing::Context;
use cf_ledger::roles::seam::with_a_scout;
use cf_ledger::{NewTask, ProjectView};
use serde_json::{json, Value};

/// A project at /work/app with the CHECKs off: apollo its chief, and `staff` as
/// `(agent, role, tier)`.
fn project_of(context: &Context, staff: &[(&str, &str, &str)]) -> ProjectView {
    context
        .ledger
        .borrow()
        .sqlite()
        .execute_batch("PRAGMA ignore_check_constraints = ON")
        .unwrap();
    let members: Vec<Value> = staff
        .iter()
        .map(|(agent, role, tier)| {
            json!({ "agent": agent, "harness": "claude-code", "roles": [role], "tier": tier })
        })
        .collect();
    context
        .open_project(json!({
            "directory": "/work/app",
            "name": "app",
            "chief": { "harness": "claude-code", "model": "apollo" },
            "staff": members,
        }))
        .unwrap()
        .expect("the project")
}

/// The chief opens a task for the scouts, which takes no tier.
fn scout_task(context: &Context, project: &ProjectView) -> i64 {
    context
        .create_task(
            project.id,
            NewTask {
                from: "chief".to_owned(),
                pool: Some("scout".to_owned()),
                body: "Look around".to_owned(),
                ..NewTask::default()
            },
        )
        .task
        .number
}

#[test]
fn a_task_for_a_role_with_no_tier_goes_to_a_member_of_it_whatever_its_tier() {
    with_a_scout(|| {
        let context = Context::new();
        let project = project_of(
            &context,
            &[("zeus", "worker", "standard"), ("argus", "scout", "light")],
        );
        let number = scout_task(&context, &project);
        context.pass().unwrap();
        let task = context.task(project.id, number).task;
        assert_eq!(
            (task.pool.as_deref(), task.tier.as_deref()),
            (Some("scout"), None)
        );
        assert!(
            task.assignee
                .as_deref()
                .is_some_and(|assignee| assignee.starts_with("argus-")),
            "the scout took it: {:?}",
            task.assignee
        );
    });
}

#[test]
fn a_task_for_a_role_nobody_holds_any_more_waits_and_says_it_by_the_roles_name() {
    with_a_scout(|| {
        let context = Context::new();
        let project = project_of(
            &context,
            &[("zeus", "worker", "standard"), ("argus", "scout", "light")],
        );
        let number = scout_task(&context, &project);
        context
            .ledger
            .borrow_mut()
            .remove_member(project.id, "argus")
            .unwrap();
        for _ in 0..3 {
            context.pass().unwrap();
        }
        let chief = context.id(project.id, "chief");
        let waits: Vec<String> = context
            .inbox(chief)
            .into_iter()
            .filter(|message| message.kind == "note" && message.body.contains(" waits for "))
            .map(|message| message.body)
            .collect();
        assert_eq!(
            waits,
            [format!(
                "T-{number} waits for a free field scout: no field scout is on the staff (ask the human for one, in your terminal)."
            )]
        );
    });
}

#[test]
fn the_chiefs_staff_table_says_no_tier_for_a_member_whose_roles_have_none() {
    with_a_scout(|| {
        let context = Context::new();
        context
            .ledger
            .borrow()
            .sqlite()
            .execute_batch("PRAGMA ignore_check_constraints = ON")
            .unwrap();
        let project = context
            .open_project(json!({
                "directory": "/work/app",
                "name": "app",
                "chief": { "harness": "claude-code", "model": "apollo" },
                "staff": [
                    // Kept in the lowest tier, as the ledger keeps one given none.
                    { "agent": "argus", "harness": "claude-code", "roles": ["scout"], "tier": "light" },
                    { "agent": "zeus", "harness": "claude-code", "roles": ["worker", "scout"], "tier": "complex" },
                    { "agent": "hera", "harness": "claude-code", "roles": ["worker"], "tier": "standard" },
                ],
            }))
            .unwrap()
            .expect("the project");
        let tiers: Vec<(String, Option<&str>)> = cf_engine::roles::staff_of(&project)
            .into_iter()
            .map(|row| (row.name, row.work_tier.map(|tier| tier.as_str())))
            .collect();
        assert_eq!(
            tiers,
            [
                ("argus".to_owned(), None),
                ("zeus".to_owned(), Some("complex")),
                ("hera".to_owned(), Some("standard")),
            ]
        );
    });
}

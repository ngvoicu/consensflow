//! The tester in the ledger: a member of a role of its own, a task for a pool
//! of its own, taken in a session of that role, and finished with a result to
//! the chief. Migration 0012 is what lets the tables hold it: the role check of
//! `participant` and the pool check of `task` name it (the migration itself is
//! held by `receipt/legacy.rs`), so each test here writes through them.

#![allow(clippy::expect_used)]

use cf_ledger::{
    open_ledger, Ledger, NewChief, NewMember, NewProject, NewTask, Options, ProjectView,
};
use serde_json::json;

/// Session names in a fixed order, so a test can say `tess-amber-pine`.
fn options() -> Options {
    let mut names = ["amber-pine", "brisk-birch"].into_iter().cycle();
    Options {
        names: Box::new(move || names.next().unwrap_or_default().to_owned()),
        ..Options::default()
    }
}

fn member(agent: &str, roles: &[&str], tier: &str) -> NewMember {
    NewMember {
        agent: agent.into(),
        harness: "claude-code".into(),
        designer: false,
        roles: roles.iter().map(|role| (*role).to_owned()).collect(),
        tier: tier.into(),
    }
}

/// A project at /work/app with apollo for its chief and `staff`.
fn project(ledger: &mut Ledger, staff: Vec<NewMember>) -> ProjectView {
    ledger
        .create_project(&NewProject {
            directory: "/work/app".into(),
            name: "app".into(),
            chief: NewChief {
                harness: "claude-code".into(),
                agent: Some("apollo".into()),
            },
            staff,
            gate: false,
        })
        .expect("a project")
}

fn id_of(project: &ProjectView, handle: &str) -> i64 {
    project
        .participants
        .iter()
        .find(|participant| participant.handle == handle)
        .expect("a participant")
        .id
}

/// The chief opens a task for the testers of `tier`.
fn for_testers(tier: &str, body: &str) -> NewTask {
    NewTask {
        from: "chief".into(),
        pool: Some("tester".into()),
        tier: Some(tier.into()),
        body: body.into(),
        ..NewTask::default()
    }
}

fn ledger_in(dir: &tempfile::TempDir) -> Ledger {
    open_ledger(&dir.path().join("consensflow.db"), options()).expect("a ledger")
}

#[test]
fn a_tester_takes_the_task_opened_for_its_pool_in_a_session_of_its_role_and_its_result_goes_to_the_chief(
) {
    let dir = tempfile::tempdir().expect("a home");
    let mut ledger = ledger_in(&dir);
    let project = project(
        &mut ledger,
        vec![
            member("zeus", &["worker"], "standard"),
            member("tess", &["tester"], "standard"),
        ],
    );
    assert_eq!(
        project
            .participants
            .iter()
            .find(|participant| participant.handle == "tess")
            .map(|tess| (tess.role.as_str(), tess.roles.clone())),
        Some(("tester", vec!["tester".to_owned()])),
        "the role a member leads with is the tester"
    );

    let created = ledger
        .create_task(project.id, &for_testers("standard", "Run the suite on T-1"))
        .expect("a task for the testers");
    assert_eq!(
        (
            created.task.pool.as_deref(),
            created.task.tier.as_deref(),
            created.task.state.as_str(),
            created.task.assignee,
        ),
        (Some("tester"), Some("standard"), "open", None),
        "it waits on the board for a tester"
    );
    let candidates = ledger.candidates(project.id, 1).expect("who may take it");
    assert_eq!(
        candidates
            .iter()
            .map(|candidate| candidate.member.handle.as_str())
            .collect::<Vec<_>>(),
        ["tess"],
        "the worker does not take a test"
    );

    let moved = ledger
        .assign_task(project.id, 1, id_of(&project, "tess"))
        .expect("assigned");
    assert_eq!(moved.task.assignee.as_deref(), Some("tess-amber-pine"));
    let session = ledger
        .project(project.id)
        .expect("the project")
        .expect("it is there")
        .participants
        .into_iter()
        .find(|participant| participant.handle == "tess-amber-pine")
        .expect("its session");
    assert_eq!(
        (session.role.as_str(), session.member.as_deref()),
        ("tester", Some("tess")),
        "its window is a tester's"
    );

    let brief = moved.message.expect("its brief").id;
    ledger.begin_delivery(brief).expect("its delivery begins");
    ledger
        .confirm_delivery(brief, Some(&json!({ "item": "i-1" })))
        .expect("its delivery is confirmed");
    let done = ledger
        .record_result(
            project.id,
            1,
            "12 checks passed, 1 failed: steps to reproduce",
        )
        .expect("its result");
    assert_eq!(done.task.state, "done");
    let result = done.message.expect("its result");
    assert_eq!(
        (result.kind.as_str(), result.recipient.as_str()),
        ("result", "chief")
    );
}

#[test]
fn a_member_may_hold_the_tester_role_beside_another_or_lead_with_it() {
    let dir = tempfile::tempdir().expect("a home");
    let mut ledger = ledger_in(&dir);
    let project = project(&mut ledger, vec![member("zeus", &["worker"], "standard")]);
    let handles = |role: &str, ledger: &Ledger| {
        ledger
            .members(project.id, role)
            .expect("the members")
            .into_iter()
            .map(|member| member.handle)
            .collect::<Vec<_>>()
    };
    assert!(handles("tester", &ledger).is_empty());

    let beside = ledger
        .set_roles(project.id, "zeus", &["worker", "tester"])
        .expect("a worker may test too");
    assert_eq!((beside.role.as_str(), beside.roles.len()), ("worker", 2));
    assert_eq!(handles("tester", &ledger), ["zeus"]);
    assert_eq!(handles("worker", &ledger), ["zeus"]);

    let leading = ledger
        .set_roles(project.id, "zeus", &["tester", "worker"])
        .expect("and may lead with it");
    assert_eq!(leading.role, "tester", "the first role leads");
}

#[test]
fn an_image_agent_is_no_tester_and_a_pool_nobody_holds_is_refused_by_name() {
    let dir = tempfile::tempdir().expect("a home");
    let mut ledger = ledger_in(&dir);
    let project = project(&mut ledger, vec![member("zeus", &["worker"], "standard")]);
    let image_agent = NewMember {
        designer: true,
        ..member("pygmalion", &["tester"], "standard")
    };
    let refused = ledger
        .add_member(project.id, &image_agent)
        .expect_err("an image agent is nothing but a designer");
    assert_eq!(
        (refused.code(), refused.to_string()),
        (
            Some("invalid-role"),
            "pygmalion is an image agent, which can only be an image designer".to_owned()
        )
    );

    let refused = ledger
        .create_task(project.id, &for_testers("standard", "Run the suite"))
        .expect_err("nobody on the staff tests");
    assert_eq!(
        (refused.code(), refused.to_string()),
        (
            Some("no-member-of-tier"),
            "no tester is on the staff: ask the human for one, in your terminal".to_owned()
        )
    );
}

#[test]
fn a_task_read_from_json_may_be_for_the_testers_and_a_pool_that_is_none_names_them() {
    let task = NewTask::from_json(&json!({
        "from": "chief", "pool": "tester", "tier": "light", "body": "Run it"
    }))
    .expect("a task for the testers");
    assert_eq!(
        (task.pool.as_deref(), task.tier.as_deref()),
        (Some("tester"), Some("light"))
    );
    let refused = NewTask::from_json(&json!({
        "from": "chief", "pool": "checker", "tier": "light", "body": "Run it"
    }))
    .expect_err("no such pool");
    assert_eq!(
        (refused.code(), refused.to_string()),
        (
            Some("invalid-pool"),
            "a pool is worker, advisor, reviewer, tester, designer, not checker".to_owned()
        )
    );
}

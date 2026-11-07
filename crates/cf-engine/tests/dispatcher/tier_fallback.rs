//! A tier nobody on the staff holds goes to the nearest one somebody does, the
//! next one up first, for every role that has tiers (T-1108): worker, advisor,
//! reviewer and tester. The designer has none. A task still waiting when the
//! staff changes under it (a member left, or its tier moved with the catalog)
//! goes the same way, and a task that waits says why. Node's dispatcher had no
//! such rule at the dispatcher's door, so no trace holds these.

use cf_engine::testing::Context;
use cf_ledger::{NewTask, ProjectView};
use serde_json::{json, Value};

/// The tiers, from the one asked first to the one asked last.
const TIERS: [&str; 4] = ["critical", "complex", "standard", "light"];

/// The roles whose tasks have a tier, with the pool each is asked for.
const POOLS: [&str; 4] = ["worker", "advisor", "reviewer", "tester"];

/// A project at /work/app: apollo its chief, and `staff` as `(agent, role, tier)`.
fn project_of(context: &Context, staff: &[(&str, &str, &str)]) -> ProjectView {
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

/// The chief opens a task for `tier` of `pool`; the work of a critical tier names its purpose.
fn open(context: &Context, project: &ProjectView, pool: &str, tier: &str) -> i64 {
    context
        .create_task(
            project.id,
            NewTask {
                from: "chief".to_owned(),
                pool: Some(pool.to_owned()),
                tier: Some(tier.to_owned()),
                purpose: (tier == "critical").then(|| "hard-problem".to_owned()),
                body: "Do the work".to_owned(),
                ..NewTask::default()
            },
        )
        .task
        .number
}

/// The notes the chief was written that say a task's tier moved.
fn retiered(context: &Context, project: &ProjectView) -> Vec<String> {
    let chief = context.id(project.id, "chief");
    let mut said: Vec<String> = context
        .inbox(chief)
        .into_iter()
        .filter(|message| message.kind == "note" && message.body.contains(" asked for a "))
        .map(|message| message.body)
        .collect();
    said.reverse();
    said
}

/// The notes the chief was written that say a task waits.
fn waits(context: &Context, project: &ProjectView) -> Vec<String> {
    let chief = context.id(project.id, "chief");
    let mut said: Vec<String> = context
        .inbox(chief)
        .into_iter()
        .filter(|message| message.kind == "note" && message.body.contains(" waits for "))
        .map(|message| message.body)
        .collect();
    said.reverse();
    said
}

#[test]
fn a_tier_nobody_holds_goes_to_the_nearest_for_every_role_and_every_tier() {
    for pool in POOLS {
        for held in TIERS {
            for asked in TIERS.into_iter().filter(|asked| *asked != held) {
                let context = Context::new();
                let project = project_of(&context, &[("zeus", pool, held)]);
                let number = open(&context, &project, pool, asked);
                context.pass().unwrap();
                context.pass().unwrap();
                let task = context.task(project.id, number).task;
                assert_eq!(
                    (task.tier.as_deref(), task.assignee.is_some()),
                    (Some(held), true),
                    "a {asked} {pool} task goes to the {held} {pool} the staff has"
                );
                assert_eq!(
                    waits(&context, &project),
                    Vec::<String>::new(),
                    "a {asked} {pool} task is told nothing of a wait when the {held} one is free"
                );
            }
        }
    }
}

#[test]
fn a_task_open_when_the_last_member_of_its_tier_moves_to_another_tier_goes_to_the_nearest() {
    for pool in POOLS {
        let context = Context::new();
        let project = project_of(&context, &[("zeus", pool, "complex")]);
        let number = open(&context, &project, pool, "complex");
        // The human moves zeus to another tier in Edit member: the tiers no longer follow a
        // saved agent's (harness-models).
        let zeus = project
            .participants
            .iter()
            .find(|participant| participant.role != "chief" && participant.role != "human")
            .expect("the member")
            .handle
            .clone();
        context
            .ledger
            .borrow_mut()
            .edit_member(
                project.id,
                &zeus,
                &cf_ledger::MemberEdit {
                    harness: "claude-code".to_owned(),
                    model: None,
                    effort: None,
                    model_name: Some("zeus".to_owned()),
                    tier: "standard".to_owned(),
                },
            )
            .unwrap();
        context.pass().unwrap();
        context.pass().unwrap();
        let task = context.task(project.id, number).task;
        assert_eq!(
            (task.tier.as_deref(), task.assignee.is_some()),
            (Some("standard"), true),
            "the {pool} task waits for no tier nobody holds"
        );
        assert_eq!(waits(&context, &project), Vec::<String>::new());
    }
}

#[test]
fn a_task_open_when_the_last_member_of_its_tier_leaves_goes_to_the_nearest() {
    for pool in POOLS {
        let context = Context::new();
        let project = project_of(
            &context,
            &[("zeus", pool, "complex"), ("hera", pool, "standard")],
        );
        let number = open(&context, &project, pool, "complex");
        context
            .ledger
            .borrow_mut()
            .remove_member(project.id, "zeus")
            .unwrap();
        context.pass().unwrap();
        context.pass().unwrap();
        let task = context.task(project.id, number).task;
        assert_eq!(
            (task.tier.as_deref(), task.assignee.is_some()),
            (Some("standard"), true),
            "the {pool} task waits for no tier nobody holds"
        );
        assert_eq!(waits(&context, &project), Vec::<String>::new());
    }
}

#[test]
fn the_requester_is_told_once_which_tier_took_a_task_that_lost_the_one_it_asked() {
    let context = Context::new();
    let project = project_of(
        &context,
        &[
            ("zeus", "tester", "complex"),
            ("anubis", "tester", "standard"),
        ],
    );
    let number = open(&context, &project, "tester", "complex");
    context
        .ledger
        .borrow_mut()
        .remove_member(project.id, "zeus")
        .unwrap();
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert!(
        context.task(project.id, number).task.assignee.is_some(),
        "the standard tester took it"
    );
    assert_eq!(
        retiered(&context, &project),
        ["T-1 asked for a complex tester; none is on the staff, so it goes to the standard tier: @anubis."],
        "told once, with the member of the tier that took it"
    );
}

#[test]
fn a_task_for_a_role_nobody_holds_any_more_waits_and_says_so() {
    for pool in POOLS {
        let context = Context::new();
        let project = project_of(&context, &[("zeus", pool, "standard")]);
        let number = open(&context, &project, pool, "standard");
        context
            .ledger
            .borrow_mut()
            .remove_member(project.id, "zeus")
            .unwrap();
        for _ in 0..3 {
            context.pass().unwrap();
        }
        assert_eq!(
            waits(&context, &project),
            [format!(
                "T-{number} waits for a free standard {pool}: no {pool} is on the staff (ask the human for one, in your terminal)."
            )],
            "a {pool} task says why it waits, once"
        );
    }
}

#[test]
fn the_nearest_tier_is_the_next_one_up_before_the_next_one_down() {
    // (held, asked, goes to): a tie goes up, and the nearer one wins over the one above.
    for (held, asked, goes_to) in [
        (["critical", "standard"], "complex", "critical"),
        (["complex", "light"], "standard", "complex"),
        (["critical", "light"], "standard", "light"),
        (["standard", "light"], "critical", "standard"),
        (["critical", "complex"], "light", "complex"),
    ] {
        let context = Context::new();
        let project = project_of(
            &context,
            &[("zeus", "advisor", held[0]), ("hera", "advisor", held[1])],
        );
        let number = open(&context, &project, "advisor", asked);
        let task = context.task(project.id, number).task;
        assert_eq!(
            task.tier.as_deref(),
            Some(goes_to),
            "{asked} with {held:?} held goes to {goes_to}"
        );
    }
}

#[test]
fn a_role_nobody_holds_is_refused_at_add_for_every_pool_in_the_words_the_chief_is_told() {
    for pool in ["worker", "advisor", "reviewer", "tester", "designer"] {
        let context = Context::new();
        // A staff that holds every role but this one.
        let others: Vec<(&str, &str, &str)> = [
            ("w", "worker"),
            ("a", "advisor"),
            ("r", "reviewer"),
            ("t", "tester"),
        ]
        .into_iter()
        .filter(|(_, role)| *role != pool)
        .map(|(agent, role)| (agent, role, "standard"))
        .collect();
        let project = project_of(&context, &others);
        let refused = context
            .ledger
            .borrow_mut()
            .create_task(
                project.id,
                &NewTask {
                    from: "chief".to_owned(),
                    pool: Some(pool.to_owned()),
                    tier: (pool != "designer").then(|| "standard".to_owned()),
                    body: "Do the work".to_owned(),
                    ..NewTask::default()
                },
            )
            .expect_err("nobody holds the role");
        let role = if pool == "designer" {
            "image designer"
        } else {
            pool
        };
        assert_eq!(
            (refused.code(), refused.to_string()),
            (
                Some("no-member-of-tier"),
                format!("no {role} is on the staff: ask the human for one, in your terminal")
            )
        );
    }
}

#[test]
fn a_task_for_a_tier_somebody_holds_is_never_moved_though_that_member_is_out_of_quota() {
    let context = Context::new();
    let project = project_of(
        &context,
        &[
            ("zeus", "worker", "complex"),
            ("hera", "worker", "standard"),
        ],
    );
    let number = open(&context, &project, "worker", "complex");
    let until = crate::fixtures::soon(&context, 3);
    let zeus = context.id(project.id, "zeus");
    context
        .ledger
        .borrow_mut()
        .mark_out(zeus, &until, "out of quota")
        .unwrap();
    for _ in 0..3 {
        context.pass().unwrap();
    }
    let task = context.task(project.id, number).task;
    assert_eq!(
        (task.tier.as_deref(), task.assignee.is_some()),
        (Some("complex"), false),
        "it waits for zeus, who holds the tier, and hera's is another"
    );
    assert_eq!(retiered(&context, &project), Vec::<String>::new());
    assert_eq!(
        waits(&context, &project),
        [format!(
            "T-{number} waits for a free complex worker: @zeus is out of quota until {until}."
        )],
        "and says why"
    );
}
